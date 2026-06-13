//! Cookie Bridge: extract cookies from the platform's native webview cookie store
//! and inject them into reqwest cookie jars.
//!
//! - macOS: WKHTTPCookieStore (ObjC API via objc2)
//! - Windows: WebView2 Chrome DevTools Protocol (CDP)

use tauri::Manager;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::{delete_university_cookies, extract_all_cookies, set_all_cookies};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use self::windows::{delete_university_cookies, extract_all_cookies, set_all_cookies};

/// Keychain key holding the JSON backup of the SSO/Okta session cookies. These
/// normally live only in the OS webview store; backing them up lets headless
/// re-auth survive the webview store being cleared (OS cleanup / reinstall),
/// keeping the long-lived device token usable for far longer.
const SSO_COOKIE_BACKUP_KEY: &str = "sso_cookie_backup";
static SSO_RESTORE_ONCE: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

/// All university cookies are deleted by a complete login reset.
fn is_university_cookie(domain: &str) -> bool {
    let d = domain.trim_start_matches('.');
    d == "kwansei.ac.jp" || d.ends_with(".kwansei.ac.jp")
}

/// Only identity-provider cookies are backed up. Service-provider cookies,
/// especially KGC's short-lived cookies, must retain their natural lifetime.
fn is_backupable_sso_cookie(domain: &str) -> bool {
    let d = domain.trim_start_matches('.');
    d == "kwansei.ac.jp" || OKTA_HOSTS.contains(&d)
}

/// Remove all university/SSO cookies from the native webview store and delete
/// the keychain backup so startup restoration cannot silently log back in.
pub async fn clear_university_cookies(app: &tauri::AppHandle) -> Result<usize, String> {
    crate::keychain::delete_secret(SSO_COOKIE_BACKUP_KEY);
    let deleted = delete_university_cookies(app).await?;
    // Platform cookie deletion APIs complete asynchronously. Give the native
    // store a moment to settle before the caller opens a fresh login window.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    log::info!("clear_university_cookies: removed {deleted} university cookies");
    Ok(deleted)
}

/// Plain cookie data extracted from the webview (Send + Sync safe).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CookieData {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub expires_unix: Option<f64>,
}

/// Extract cookies matching a specific domain from the webview.
async fn extract_cookies_for_domain(
    app: &tauri::AppHandle,
    domain: &str,
) -> Result<Vec<CookieData>, String> {
    let all = extract_all_cookies(app).await?;
    let domain_owned = domain.to_string();
    Ok(all
        .into_iter()
        .filter(|c| {
            let cookie_domain = c.domain.trim_start_matches('.');
            cookie_domain == domain_owned || domain_owned.ends_with(&format!(".{}", cookie_domain))
        })
        .collect())
}

/// Inject extracted cookies into a reqwest cookie store.
fn inject_cookies(
    store: &reqwest_cookie_store::CookieStoreMutex,
    cookies: &[CookieData],
    base_url: &str,
) {
    let url = match url::Url::parse(base_url) {
        Ok(u) => u,
        Err(e) => {
            log::warn!("inject_cookies: invalid base URL {}: {}", base_url, e);
            return;
        }
    };

    let mut jar = store.lock().unwrap_or_else(|e| e.into_inner());
    let mut count = 0;
    for c in cookies {
        let mut builder = cookie_store::RawCookie::build((&*c.name, &*c.value))
            .domain(&*c.domain)
            .path(&*c.path);
        if c.secure {
            builder = builder.secure(true);
        }
        if c.http_only {
            builder = builder.http_only(true);
        }
        if let Some(ts) = c.expires_unix {
            if let Ok(odt) = time::OffsetDateTime::from_unix_timestamp(ts as i64) {
                builder = builder.expires(odt);
            }
        }
        let raw = builder.build();
        match jar.insert_raw(&raw, &url) {
            Ok(_) => count += 1,
            Err(e) => log::warn!("inject_cookies: failed to insert '{}': {}", c.name, e),
        }
    }
    log::info!(
        "inject_cookies: injected {}/{} cookies for {}",
        count,
        cookies.len(),
        base_url
    );
}

/// Check if a URL indicates we've arrived at an SP domain after SAML.
pub fn is_post_saml_sp_url(url: &url::Url, sp_host: &str) -> bool {
    let host = url.host_str().unwrap_or("");
    if host != sp_host {
        return false;
    }
    let path = url.path();
    if path.contains("Shibboleth.sso")
        || path.starts_with("/saml/")
        || path.starts_with("/Shibboleth.sso")
    {
        return false;
    }
    true
}

/// Extract cookies for a specific SP domain (+ parent SSO cookies) from the webview
/// and inject them into a reqwest cookie store.
pub async fn extract_and_inject(
    app: &tauri::AppHandle,
    sp_domain: &str,
    cookie_store: &reqwest_cookie_store::CookieStoreMutex,
    base_url: &str,
) -> Result<(), String> {
    let sp_cookies = extract_cookies_for_domain(app, sp_domain).await?;
    let sso_cookies = match extract_cookies_for_domain(app, "kwansei.ac.jp").await {
        Ok(cookies) => cookies,
        Err(e) => {
            log::warn!("Failed to extract SSO parent domain cookies: {e}");
            Vec::new()
        }
    };
    let all: Vec<_> = sp_cookies
        .iter()
        .chain(sso_cookies.iter())
        .cloned()
        .collect();
    inject_cookies(cookie_store, &all, base_url);
    Ok(())
}

const OKTA_HOSTS: &[&str] = &[
    "sso.kwansei.ac.jp",
    "idp.kwansei.ac.jp",
    "sts.kwansei.ac.jp",
];

fn is_okta_login_page(url: &url::Url) -> bool {
    let host = url.host_str().unwrap_or("");
    OKTA_HOSTS.contains(&host)
}

/// Back up the current SSO/Okta cookies (kwansei.ac.jp family) from the webview
/// store to the keychain. Called after a successful login or headless refresh
/// so the stored device token always reflects the latest session. A read that
/// yields nothing is ignored so we never clobber a good backup with an empty one.
pub async fn persist_sso_cookies(app: &tauri::AppHandle) {
    let all = match extract_all_cookies(app).await {
        Ok(c) => c,
        Err(e) => {
            log::warn!("persist_sso_cookies: extraction failed: {e}");
            return;
        }
    };
    let sso: Vec<CookieData> = all
        .into_iter()
        .filter(|c| is_backupable_sso_cookie(&c.domain))
        .collect();
    if sso.is_empty() {
        log::info!("persist_sso_cookies: no SSO cookies to back up (skipped)");
        return;
    }
    match serde_json::to_string(&sso) {
        Ok(json) => {
            if let Err(e) = crate::keychain::set_secret(SSO_COOKIE_BACKUP_KEY, &json) {
                log::warn!("persist_sso_cookies: keychain write failed: {e}");
            } else {
                log::info!("persist_sso_cookies: backed up {} SSO cookies", sso.len());
            }
        }
        Err(e) => log::warn!("persist_sso_cookies: serialize failed: {e}"),
    }
}

/// Restore the keychain-backed SSO/Okta cookies into the webview store. Called
/// once at startup (before any headless re-auth) so a wiped webview store can
/// still present a valid SSO session / device token. Expired cookies are
/// dropped. Best-effort: any failure just means we fall back to normal login.
pub async fn restore_sso_cookies(app: &tauri::AppHandle) {
    SSO_RESTORE_ONCE
        .get_or_init(|| async {
            restore_sso_cookies_inner(app).await;
        })
        .await;
}

async fn restore_sso_cookies_inner(app: &tauri::AppHandle) {
    let Some(json) = crate::keychain::get_secret(SSO_COOKIE_BACKUP_KEY) else {
        return;
    };
    let cookies: Vec<CookieData> = match serde_json::from_str(&json) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("restore_sso_cookies: parse failed: {e}");
            return;
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let live: Vec<CookieData> = cookies
        .into_iter()
        .filter(|c| c.expires_unix.is_none_or(|exp| exp > now))
        .collect();
    if live.is_empty() {
        return;
    }
    match set_all_cookies(app, &live).await {
        Ok(n) => log::info!("restore_sso_cookies: restored {n} SSO cookies"),
        Err(e) => log::warn!("restore_sso_cookies: webview write failed: {e}"),
    }
}

pub async fn headless_saml_window(
    app: &tauri::AppHandle,
    window_label: &str,
    saml_url: &str,
    sp_domain: &str,
    timeout_secs: u64,
) -> Result<Option<tauri::WebviewWindow>, String> {
    if let Some(w) = app.get_webview_window(window_label) {
        let _ = w.close();
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<bool>(1);

    let parsed_url: url::Url = saml_url
        .parse()
        .map_err(|e| format!("URL parse error: {}", e))?;

    let sp_domain_owned = sp_domain.to_string();
    let label_for_log = window_label.to_string();
    let win = tauri::WebviewWindowBuilder::new(
        app,
        window_label,
        tauri::WebviewUrl::External(parsed_url),
    )
    .visible(false)
    .on_navigation(|_| true)
    .on_page_load(move |_win, payload| {
        use tauri::webview::PageLoadEvent;
        if !matches!(payload.event(), PageLoadEvent::Finished) {
            return;
        }
        let url = payload.url();
        if is_post_saml_sp_url(url, &sp_domain_owned) {
            log::info!("{}: page loaded on SP domain", label_for_log);
            let _ = tx.try_send(true);
        } else if is_okta_login_page(url) {
            log::info!(
                "{}: Okta login page detected - session expired",
                label_for_log
            );
            let _ = tx.try_send(false);
        }
    })
    .build()
    .map_err(|e| format!("Failed to build headless window '{}': {}", window_label, e))?;

    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx.recv()).await {
        Ok(Some(true)) => {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            Ok(Some(win))
        }
        Ok(Some(false)) => {
            let _ = win.close();
            Ok(None)
        }
        Ok(None) => {
            log::info!("{}: window closed without completing", window_label);
            Ok(None)
        }
        Err(_) => {
            log::info!("{}: timed out - Okta session likely expired", window_label);
            let _ = win.close();
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_backupable_sso_cookie, is_university_cookie};

    #[test]
    fn sso_backup_excludes_service_provider_cookies() {
        assert!(is_backupable_sso_cookie(".kwansei.ac.jp"));
        assert!(is_backupable_sso_cookie("sso.kwansei.ac.jp"));
        assert!(is_backupable_sso_cookie("idp.kwansei.ac.jp"));
        assert!(is_backupable_sso_cookie("sts.kwansei.ac.jp"));

        assert!(!is_backupable_sso_cookie("kg-course.kwansei.ac.jp"));
        assert!(!is_backupable_sso_cookie("luna.kwansei.ac.jp"));
        assert!(!is_backupable_sso_cookie("kwic.kwansei.ac.jp"));
        assert!(!is_backupable_sso_cookie("example.com"));
    }

    #[test]
    fn complete_reset_includes_all_university_cookies() {
        assert!(is_university_cookie(".kwansei.ac.jp"));
        assert!(is_university_cookie("sso.kwansei.ac.jp"));
        assert!(is_university_cookie("kg-course.kwansei.ac.jp"));
        assert!(is_university_cookie("luna.kwansei.ac.jp"));
        assert!(is_university_cookie("kwic.kwansei.ac.jp"));
        assert!(!is_university_cookie("example.com"));
    }
}
