use crate::auth;
use crate::client;
use crate::config;
use crate::cookie_bridge;
use crate::kwic_client;
use crate::luna_client;
use crate::parser;
use crate::{KgcState, KwicState, LunaState};
use serde::Serialize;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::LazyLock;
use tauri::{Manager, State};

static SESSION_SYNC_GATE: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));
static SESSION_SYNC_LAST_START: LazyLock<std::sync::Mutex<Option<std::time::Instant>>> =
    LazyLock::new(|| std::sync::Mutex::new(None));
const SESSION_SYNC_MIN_GAP: std::time::Duration = std::time::Duration::from_secs(2);
const KGC_AUTO_RECOVERY_COOLDOWN_SECS: i64 = 30 * 60;
const KGC_AUTO_PREFLIGHT_REUSE_SECS: i64 = 60;
static KGC_AUTO_RECOVERY_LAST_ATTEMPT: AtomicI64 = AtomicI64::new(0);
static KGC_AUTO_PREFLIGHT_LAST_SUCCESS: AtomicI64 = AtomicI64::new(0);
static KGC_AUTO_PREFLIGHT_GATE: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

pub(super) async fn lock_session_sync() -> tokio::sync::MutexGuard<'static, ()> {
    SESSION_SYNC_GATE.lock().await
}

async fn pace_session_sync() {
    let wait = {
        let last = SESSION_SYNC_LAST_START
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        last.and_then(|started| SESSION_SYNC_MIN_GAP.checked_sub(started.elapsed()))
    };
    if let Some(wait) = wait {
        tokio::time::sleep(wait).await;
    }
    let mut last = SESSION_SYNC_LAST_START
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *last = Some(std::time::Instant::now());
}

/// Allow an automatic KGC data task to make one hidden-login attempt when the
/// short-lived KGC session is absent. The shared cooldown prevents the data and
/// notification loops from repeatedly opening competing SAML flows.
pub(crate) async fn auto_recover_kgc_session_once(app: &tauri::AppHandle) -> bool {
    if app
        .state::<KgcState>()
        .client
        .lock()
        .await
        .is_authenticated()
    {
        return true;
    }

    let now = crate::db::epoch_secs();
    let previous = KGC_AUTO_RECOVERY_LAST_ATTEMPT.swap(now, Ordering::SeqCst);
    if previous > 0 && now.saturating_sub(previous) < KGC_AUTO_RECOVERY_COOLDOWN_SECS {
        log::info!("KGC automatic recovery skipped during cooldown");
        return false;
    }

    log::info!("KGC automatic data request: attempting one hidden login");
    match sync_session(
        app.clone(),
        app.state::<KgcState>(),
        app.state::<LunaState>(),
        app.state::<KwicState>(),
        "kgc".to_string(),
    )
    .await
    {
        Ok(true) => true,
        Ok(false) => {
            log::warn!("KGC automatic recovery did not establish a session");
            false
        }
        Err(error) => {
            log::warn!("KGC automatic recovery failed: {}", error);
            false
        }
    }
}

/// Confirm KGC immediately before an automatic KGC-backed data run. Validation
/// is request-driven rather than periodic; a server-confirmed expiry may cause
/// one hidden-login attempt, while transient validation errors retain the
/// existing session and let the real data request decide.
pub(crate) async fn ensure_kgc_session_for_automatic_request(app: &tauri::AppHandle) -> bool {
    let _preflight_gate = KGC_AUTO_PREFLIGHT_GATE.lock().await;
    let now = crate::db::epoch_secs();
    if app
        .state::<KgcState>()
        .client
        .lock()
        .await
        .is_authenticated()
    {
        let last_success = KGC_AUTO_PREFLIGHT_LAST_SUCCESS.load(Ordering::Relaxed);
        if last_success > 0 && now.saturating_sub(last_success) < KGC_AUTO_PREFLIGHT_REUSE_SECS {
            return true;
        }
        match crate::commands::check_session(app.state::<KgcState>()).await {
            Ok(status) if status.valid => {
                KGC_AUTO_PREFLIGHT_LAST_SUCCESS.store(now, Ordering::Relaxed);
                return true;
            }
            Ok(_) => {}
            Err(error) => {
                log::warn!("KGC automatic preflight validation failed: {}", error);
            }
        }

        // check_session only clears the stored session after a server-confirmed
        // expiry. Retain it after transient validation failures.
        if app
            .state::<KgcState>()
            .client
            .lock()
            .await
            .is_authenticated()
        {
            return true;
        }
    }

    let recovered = auto_recover_kgc_session_once(app).await;
    if recovered {
        KGC_AUTO_PREFLIGHT_LAST_SUCCESS.store(crate::db::epoch_secs(), Ordering::Relaxed);
    }
    recovered
}

#[derive(Debug, Serialize)]
pub struct SessionStates {
    pub kgc: bool,
    pub luna: bool,
    pub kwic: bool,
}

#[tauri::command]
pub async fn get_session_states(
    state: State<'_, KgcState>,
    luna_state: State<'_, LunaState>,
    kwic_state: State<'_, KwicState>,
) -> Result<SessionStates, String> {
    let kgc = state.client.lock().await.is_authenticated();
    let luna = luna_state.client.lock().await.authenticated;
    let kwic = kwic_state.client.lock().await.authenticated;
    Ok(SessionStates { kgc, luna, kwic })
}

#[tauri::command]
pub fn get_saved_cookie_summaries() -> Vec<client::SavedCookieSummary> {
    [
        ("kgc", client::KGC_COOKIES_KEY),
        ("luna", luna_client::LUNA_COOKIES_KEY),
        ("kwic", kwic_client::KWIC_COOKIES_KEY),
    ]
    .into_iter()
    .map(|(service, key)| client::saved_cookie_summary(service, key))
    .collect()
}

#[allow(clippy::too_many_arguments)]
async fn headless_saml_refresh(
    app: &tauri::AppHandle,
    label: &str,
    saml_url: &str,
    sp_domain: &str,
    base_url: &str,
    verify_url: &str,
    cookie_store: &reqwest_cookie_store::CookieStoreMutex,
    http: &reqwest::Client,
    session_expired_msg: &str,
    is_session_expired: fn(&str) -> bool,
) -> Result<bool, String> {
    log::info!("headless_{}: starting (Cookie Bridge)", label);

    let win = match cookie_bridge::headless_saml_window(app, label, saml_url, sp_domain, 20).await?
    {
        Some(w) => w,
        None => return Ok(false),
    };

    cookie_bridge::extract_and_inject(app, sp_domain, cookie_store, base_url).await?;

    let result = client::fetch_with_redirect(
        http,
        verify_url,
        base_url,
        session_expired_msg,
        is_session_expired,
    )
    .await;
    let _ = win.close();

    match result {
        Ok(_) => {
            log::info!("headless_{}: succeeded (verified)", label);
            Ok(true)
        }
        Err(e) => {
            log::warn!(
                "headless_{}: cookie injection succeeded but session invalid: {}",
                label,
                e
            );
            Err(e)
        }
    }
}

async fn headless_kgc_refresh(app: &tauri::AppHandle, state: &KgcState) -> Result<bool, String> {
    log::info!("headless_kgc_refresh: starting (Cookie Bridge)");
    let _kgc_gate = state.gate.lock().await;

    let entry_url = format!("{}/uniasv2/UnSSOLoginControl2", config::KG_COURSE_BASE);
    let win = match cookie_bridge::headless_saml_window(
        app,
        "kgc-headless",
        &entry_url,
        "kg-course.kwansei.ac.jp",
        20,
    )
    .await?
    {
        Some(w) => w,
        None => return Ok(false),
    };

    let cookie_store = state.client.lock().await.cookie_store.clone();
    cookie_bridge::extract_and_inject(
        app,
        "kg-course.kwansei.ac.jp",
        &cookie_store,
        config::KG_COURSE_BASE,
    )
    .await?;

    let http = state.client.lock().await.http.clone();
    let verify_url = format!(
        "{}/uniasv2/ARF010.do?REQ_PRFR_MNU_ID=MNUIDSTD0102014",
        config::KG_COURSE_BASE
    );
    match crate::client::fetch_page_with(&http, &verify_url).await {
        Ok(html) => {
            let info = parser::parse_student_info(&html);
            if info.student_id.is_empty() && info.name.is_empty() {
                log::warn!(
                    "headless_kgc_refresh: page returned empty student info (stale session)"
                );
                let _ = win.close();
                return Ok(false);
            }
            let mut client = state.client.lock().await;
            client.session = Some(auth::AuthSession {
                username: info.student_id.clone(),
                display_name: if info.name.is_empty() {
                    "ユーザー".to_string()
                } else {
                    info.name
                },
                student_id: info.student_id,
                faculty: info.faculty,
                department: info.department,
            });
            client.save_session();
            log::info!("headless_kgc_refresh: succeeded");
            let _ = win.close();
            Ok(true)
        }
        Err(e) => {
            let mut client = state.client.lock().await;
            client.clear_session();
            log::warn!("headless_kgc_refresh: session verification failed: {}", e);
            let _ = win.close();
            Err(e)
        }
    }
}

async fn headless_luna_refresh(app: &tauri::AppHandle, state: &LunaState) -> Result<bool, String> {
    let luna = state.client.lock().await;
    let cookie_store = luna.cookie_store.clone();
    let http = luna.http.clone();
    drop(luna);

    let verify_url = format!("{}/lms/timetable", config::LUNA_BASE);
    let ok = headless_saml_refresh(
        app,
        "luna-headless",
        config::LUNA_SAML_URL,
        "luna.kwansei.ac.jp",
        config::LUNA_BASE,
        &verify_url,
        &cookie_store,
        &http,
        luna_client::LUNA_SESSION_EXPIRED_MSG,
        luna_client::is_luna_session_expired,
    )
    .await?;
    if ok {
        let mut luna = state.client.lock().await;
        luna.authenticated = true;
        luna.save_session();
    }
    Ok(ok)
}

async fn headless_kwic_refresh(app: &tauri::AppHandle, state: &KwicState) -> Result<bool, String> {
    let kwic = state.client.lock().await;
    let cookie_store = kwic.cookie_store.clone();
    let http = kwic.http.clone();
    drop(kwic);

    let verify_url = format!("{}/portal/home", config::KWIC_BASE);
    let ok = headless_saml_refresh(
        app,
        "kwic-headless",
        config::KWIC_SAML_URL,
        "kwic.kwansei.ac.jp",
        config::KWIC_BASE,
        &verify_url,
        &cookie_store,
        &http,
        kwic_client::KWIC_SESSION_EXPIRED_MSG,
        kwic_client::is_kwic_session_expired,
    )
    .await?;
    if ok {
        let mut kwic = state.client.lock().await;
        kwic.authenticated = true;
        kwic.save_session();
    }
    Ok(ok)
}

#[tauri::command]
pub async fn sync_session(
    app: tauri::AppHandle,
    kgc_state: State<'_, KgcState>,
    luna_state: State<'_, LunaState>,
    kwic_state: State<'_, KwicState>,
    service: String,
) -> Result<bool, String> {
    // Startup restoration and the first SAML sync can race. Ensure the
    // identity-provider backup has been restored exactly once before opening
    // any hidden authentication window.
    cookie_bridge::restore_sso_cookies(&app).await;
    let _sync_gate = lock_session_sync().await;
    pace_session_sync().await;
    log::info!("sync_session: service={}", service);
    let result = match service.as_str() {
        "kgc" => headless_kgc_refresh(&app, kgc_state.inner()).await,
        "luna" => headless_luna_refresh(&app, luna_state.inner()).await,
        "kwic" => headless_kwic_refresh(&app, kwic_state.inner()).await,
        "all" => {
            // "all" means all core services. KGC is excluded from proactive
            // batch renewal because its cookies are sensitive to timing.
            let luna_ok = headless_luna_refresh(&app, luna_state.inner())
                .await
                .unwrap_or(false);
            tokio::time::sleep(SESSION_SYNC_MIN_GAP).await;
            let kwic_ok = headless_kwic_refresh(&app, kwic_state.inner())
                .await
                .unwrap_or(false);
            log::info!("sync_session(all core): luna={}, kwic={}", luna_ok, kwic_ok);
            Ok(luna_ok || kwic_ok)
        }
        _ => Err(format!("Unknown service: {}", service)),
    };
    // A successful headless refresh means a valid SSO session is in the webview
    // store — back it up (incl. the long-lived device token) so it survives a
    // webview-store wipe and stays fresh.
    if matches!(&result, Ok(true)) {
        cookie_bridge::persist_sso_cookies(&app).await;
    }
    result
}
