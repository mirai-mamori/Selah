use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    LazyLock,
};
use tauri::{Manager, State};

use crate::client;
use crate::config;
use crate::kwic_client;
use crate::KwicState;

static KWIC_DETAIL_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Briefly lock KWIC client, check auth and clone http. Releases lock immediately.
async fn kwic_http(state: &KwicState) -> Result<reqwest::Client, String> {
    let kwic = state.client.lock().await;
    if !kwic.authenticated {
        return Err(kwic_client::KWIC_AUTH_REQUIRED_MSG.into());
    }
    Ok(kwic.http.clone())
}

/// KWIC GET: fetch a page without holding the lock.
async fn kwic_get(http: &reqwest::Client, path: &str) -> Result<String, String> {
    let url = format!("{}{}", config::KWIC_BASE, path);
    client::fetch_with_redirect(
        http,
        &url,
        config::KWIC_BASE,
        kwic_client::KWIC_SESSION_EXPIRED_MSG,
        kwic_client::is_kwic_session_expired,
    )
    .await
}

/// KWIC POST: submit a form without holding the lock.
async fn kwic_post(
    http: &reqwest::Client,
    path: &str,
    params: &[(&str, &str)],
) -> Result<String, String> {
    let url = format!("{}{}", config::KWIC_BASE, path);
    client::post_form_with_redirect(
        http,
        &url,
        config::KWIC_BASE,
        kwic_client::KWIC_SESSION_EXPIRED_MSG,
        kwic_client::is_kwic_session_expired,
        params.iter().copied(),
        &[],
    )
    .await
}

// ============ Cached Selectors ============

macro_rules! sel {
    ($name:ident, $s:expr) => {
        static $name: LazyLock<scraper::Selector> =
            LazyLock::new(|| scraper::Selector::parse($s).expect(concat!("bad selector: ", $s)));
    };
}

sel!(SEL_NOTICE_A, ".portal-notice-li a.portal-notice-li-a");
sel!(SEL_MAINLINK_A, ".portal-mainlink-li a");
sel!(SEL_INFO_A, "a.portal-info-content-li-a, a[data1]");
sel!(SEL_INFO_LI, "li.portal-info-content-li");
sel!(SEL_INFO_LIST_ROW, "#information_list .result-list");
sel!(
    SEL_INFO_LIST_TITLE,
    ".portal-information-list-title.sp-contents-hidden span.link-txt[data1], span.link-txt[data1]"
);
sel!(
    SEL_INFO_LIST_DATE,
    ".portal-information-list-date.sp-contents-hidden span, .portal-information-list-date span"
);
sel!(
    SEL_INFO_LIST_DIVISION,
    ".portal-information-list-division.sp-contents-hidden, .portal-information-list-division"
);
sel!(
    SEL_INFO_TYPE_SELECTED,
    r#"select#informationType option[selected]"#
);
sel!(SEL_INFO_DATE, ".portal-subblock-infolist-left-item2 > div");
sel!(
    SEL_INFO_TITLE,
    ".portal-subblock-infolist-left-item2 > span"
);
sel!(SEL_INFO_CATEGORY, ".portal-subblock-infolist-right");

sel!(SEL_CSRF, r#"input[name="_csrf"]"#);
sel!(SEL_BLOCK_TITLE, ".block-title-txt");
sel!(SEL_CONTENTS_HTML, "#contentsHtml");
sel!(SEL_OUTGOING_DIV, ".portal-information-outgoing-division");
sel!(SEL_CONTENTS_DETAIL, ".contents-detail");
sel!(SEL_HEADER_BOLD, ".contents-header-txt .bold-txt");
sel!(SEL_INPUT_AREA, ".contents-input-area");
sel!(SEL_FILE_OBJECT, ".file-object");
sel!(SEL_FILE_NAME, ".downloadFile, .fileName");
sel!(SEL_OBJECT_NAME, ".objectName");
sel!(SEL_SUBPORTAL_TITLE, ".subportal-title-txt");
sel!(
    SEL_SUBPORTAL_LINK,
    "li.subportal-block-relation-list-li a.subportal-block-txtlink-li-b"
);
sel!(SEL_SYSTEM_IMAGE, "img.systemlink-image");
sel!(SEL_SUBPORTAL_LI, "li.subportal-block-info-list-li");
sel!(SEL_SUBPORTAL_CAT, ".subportal-block-list-li-txt-info1");
sel!(
    SEL_SUBPORTAL_TITLE_SPAN,
    ".subportal-block-list-li-txt-info2 span.link-txt"
);
sel!(
    SEL_SUBPORTAL_DATE,
    ".subportal-block-list-li-txt-info3 span"
);
sel!(SEL_SUBPORTAL_DEPT, ".subportal-block-list-li-txt-info4");
sel!(SEL_CABINET_ROW, ".cabinetList .result-list.result-data");
sel!(
    SEL_CABINET_TITLE,
    ".cabinet-view-list-name .cabinetDisplayLink, .cabinet-view-list-name a"
);
sel!(SEL_CABINET_NEW, ".cabinet-view-list-new .cabinet-area-new");
sel!(SEL_CABINET_DATE, ".cabinet-view-list-createdate span");

// Tab-specific notification selectors
sel!(SEL_TAB1_LI, "#portalinfocontent1 li.portal-info-content-li");
sel!(SEL_TAB2_LI, "#portalinfocontent2 li.portal-info-content-li");
sel!(SEL_TAB3_LI, "#portalinfocontent3 li.portal-info-content-li");
sel!(SEL_TAB4_LI, "#portalinfocontent4 li.portal-info-content-li");

// ============ Types ============

/// A notification/information entry from the KWIC Portal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicPortalNotification {
    pub id: String,
    pub title: String,
    pub date: String,
    pub category: String,
    pub important: bool,
    /// data2: informationType (e.g. "10")
    pub information_type: String,
    /// data3: personCategoryCd (e.g. "0")
    pub person_category_cd: String,
    /// data4: categoryCd (e.g. "02")
    pub category_cd: String,
}

/// The home page data from KWIC Portal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicPortalHome {
    /// Category sections on the home page
    pub sections: Vec<KwicPortalSection>,
    /// Raw HTML for debug/exploration (only in debug mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_html_debug: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicPortalSection {
    pub title: String,
    pub items: Vec<KwicPortalItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicPortalItem {
    pub id: String,
    pub title: String,
    pub date: String,
    pub category: String,
    pub url: String,
    pub important: bool,
    #[serde(default)]
    pub information_type: String,
    #[serde(default)]
    pub person_category_cd: String,
    #[serde(default)]
    pub category_cd: String,
}

// ============ Commands ============

/// Check KWIC Portal session
#[tauri::command]
pub async fn kwic_check_session(state: State<'_, KwicState>) -> Result<bool, String> {
    let (http, authenticated) = {
        let kwic = state.client.lock().await;
        (kwic.http.clone(), kwic.authenticated)
    };
    if !authenticated {
        return Ok(false);
    }
    // Validate against server without holding the lock
    let url = format!("{}/portal/home", crate::config::KWIC_BASE);
    match crate::client::fetch_with_redirect(
        &http,
        &url,
        crate::config::KWIC_BASE,
        crate::kwic_client::KWIC_SESSION_EXPIRED_MSG,
        crate::kwic_client::is_kwic_session_expired,
    )
    .await
    {
        Ok(_) => {
            let kwic = state.client.lock().await;
            kwic.save_session();
            Ok(true)
        }
        Err(e) if e == crate::kwic_client::KWIC_SESSION_EXPIRED_MSG => {
            let mut kwic = state.client.lock().await;
            kwic.authenticated = false;
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

/// Fetch and parse the KWIC Portal home page
#[tauri::command]
pub async fn kwic_fetch_home(
    state: State<'_, KwicState>,
    db: State<'_, crate::db::Database>,
) -> Result<KwicPortalHome, String> {
    match kwic_http(&state).await {
        Ok(http) => match kwic_get(&http, "/portal/home").await {
            Ok(html) => {
                #[cfg(debug_assertions)]
                {
                    if crate::should_dump_debug_html() {
                        let _ = std::fs::write(
                            std::env::temp_dir().join("kwic-portal-home.html"),
                            &html,
                        );
                    }
                }

                let mut sections = parse_portal_home(&html);
                let information_list_pages = [
                    ("/portal/home/information/list", "10"),
                    (
                        "/portal/home/information/list?informationType=12&categoryCd=0",
                        "12",
                    ),
                ];
                for (path, fallback_information_type) in information_list_pages {
                    match kwic_get(&http, path).await {
                        Ok(list_html) => {
                            #[cfg(debug_assertions)]
                            {
                                if crate::should_dump_debug_html() {
                                    let dump_name = format!(
                                        "kwic-portal-information-list-{}.html",
                                        fallback_information_type
                                    );
                                    let _ = std::fs::write(
                                        std::env::temp_dir().join(dump_name),
                                        &list_html,
                                    );
                                }
                            }
                            let (parsed, added) = merge_information_list_sections(
                                &mut sections,
                                &list_html,
                                Some(fallback_information_type),
                            );
                            log::info!(
                                "kwic_home: information list {} parsed {} item(s), added {} item(s)",
                                fallback_information_type,
                                parsed,
                                added
                            );
                        }
                        Err(e) => {
                            log::info!(
                                "kwic_home: information list {} fetch skipped ({})",
                                fallback_information_type,
                                e
                            );
                        }
                    }
                }

                let result = KwicPortalHome {
                    sections,
                    #[cfg(debug_assertions)]
                    raw_html_debug: Some(crate::client::safe_truncate(&html, 5000).to_string()),
                    #[cfg(not(debug_assertions))]
                    raw_html_debug: None,
                };
                if let Ok(json) = serde_json::to_string(&result) {
                    let _ = db.save_data_cache("kwic_home", &json);
                }
                Ok(result)
            }
            Err(e) => {
                if let Ok(Some((json, _))) = db.get_data_cache("kwic_home") {
                    if let Ok(cached) = serde_json::from_str(&json) {
                        log::info!("kwic_home: cache fallback ({})", e);
                        return Ok(cached);
                    }
                }
                Err(e)
            }
        },
        Err(e) => {
            if let Ok(Some((json, _))) = db.get_data_cache("kwic_home") {
                if let Ok(cached) = serde_json::from_str(&json) {
                    log::info!("kwic_home: cache fallback ({})", e);
                    return Ok(cached);
                }
            }
            Err(e)
        }
    }
}

/// Parsed detail content of a KWIC Portal notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicNotificationDetail {
    pub title: String,
    pub date: String,
    pub sender: String,
    pub body_html: String,
    /// Attachment file names / links (if any)
    pub attachments: Vec<KwicAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicAttachment {
    pub name: String,
    pub url: String,
}

/// Agent-accessible variant of `kwic_fetch_detail`. Resolves state from the
/// AppHandle and skips the cache-on-error fallback so the agent always sees
/// the most accurate result (or a real error message).
pub async fn kwic_fetch_detail_internal(
    app: &tauri::AppHandle,
    information_id: &str,
    information_type: &str,
    person_category_cd: &str,
    category_cd: &str,
) -> Result<KwicNotificationDetail, String> {
    use tauri::Manager;
    let state = app.state::<KwicState>();
    let http = kwic_http(&state).await?;
    let home_html = kwic_get(&http, "/portal/home").await?;
    let csrf = extract_csrf_token(&home_html)
        .ok_or_else(|| "CSRFトークンが取得できませんでした".to_string())?;
    let detail_html = kwic_post(
        &http,
        "/portal/home/information/detail",
        &[
            ("_csrf", &csrf),
            ("informationId", information_id),
            ("informationType", information_type),
            ("personCategoryCd", person_category_cd),
            ("categoryCd", category_cd),
            ("selectCategoryCd", category_cd),
            ("pageViewListNum", "10"),
        ],
    )
    .await?;
    Ok(parse_detail_html(&detail_html))
}

/// Fetch and parse a KWIC Portal notification detail inline (no webview).
/// The detail page is fetched via POST to /portal/home/information/detail
/// using the same form parameters as the portal's #PortalinformationDtl form.
#[tauri::command]
pub async fn kwic_fetch_detail(
    state: State<'_, KwicState>,
    db: State<'_, crate::db::Database>,
    information_id: String,
    information_type: String,
    person_category_cd: String,
    category_cd: String,
) -> Result<KwicNotificationDetail, String> {
    let cache_key = format!("kwic_detail:{}", information_id);
    match kwic_http(&state).await {
        Ok(http) => {
            // 1. Get home page to extract CSRF token
            let home_html = match kwic_get(&http, "/portal/home").await {
                Ok(h) => h,
                Err(e) => {
                    if let Ok(Some((json, _))) = db.get_data_cache(&cache_key) {
                        if let Ok(cached) = serde_json::from_str(&json) {
                            log::info!("{}: cache fallback ({})", cache_key, e);
                            return Ok(cached);
                        }
                    }
                    return Err(e);
                }
            };
            let csrf = match extract_csrf_token(&home_html) {
                Some(token) => token,
                None => {
                    if let Ok(Some((json, _))) = db.get_data_cache(&cache_key) {
                        if let Ok(cached) = serde_json::from_str(&json) {
                            log::info!("{}: cache fallback (CSRF extraction failed)", cache_key);
                            return Ok(cached);
                        }
                    }
                    return Err("CSRFトークンが取得できませんでした".to_string());
                }
            };

            // 2. POST to portal detail endpoint
            match kwic_post(
                &http,
                "/portal/home/information/detail",
                &[
                    ("_csrf", &csrf),
                    ("informationId", &information_id),
                    ("informationType", &information_type),
                    ("personCategoryCd", &person_category_cd),
                    ("categoryCd", &category_cd),
                    ("selectCategoryCd", &category_cd),
                    ("pageViewListNum", "10"),
                ],
            )
            .await
            {
                Ok(detail_html) => {
                    #[cfg(debug_assertions)]
                    {
                        if crate::should_dump_debug_html() {
                            let _ = std::fs::write(
                                std::env::temp_dir().join("kwic-portal-detail.html"),
                                &detail_html,
                            );
                        }
                    }

                    let data = parse_detail_html(&detail_html);
                    if let Ok(json) = serde_json::to_string(&data) {
                        let _ = db.save_data_cache(&cache_key, &json);
                    }
                    Ok(data)
                }
                Err(e) => {
                    if let Ok(Some((json, _))) = db.get_data_cache(&cache_key) {
                        if let Ok(cached) = serde_json::from_str(&json) {
                            log::info!("{}: cache fallback ({})", cache_key, e);
                            return Ok(cached);
                        }
                    }
                    Err(e)
                }
            }
        }
        Err(e) => {
            if let Ok(Some((json, _))) = db.get_data_cache(&cache_key) {
                if let Ok(cached) = serde_json::from_str(&json) {
                    log::info!("{}: cache fallback ({})", cache_key, e);
                    return Ok(cached);
                }
            }
            Err(e)
        }
    }
}

/// A link/item from a KWIC Portal subportal page
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicSubportalLink {
    pub title: String,
    pub url: String,
    pub icon_url: String,
    pub description: String,
}

/// Subportal page data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicSubportalData {
    pub title: String,
    pub links: Vec<KwicSubportalLink>,
    /// Notification items on this subportal
    pub notifications: Vec<KwicPortalNotification>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicCabinetItem {
    pub cabinet_id: String,
    pub list_id: String,
    pub name: String,
    pub level: u32,
    pub updated_at: String,
    pub is_new: bool,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwicCabinetReference {
    pub title: String,
    pub items: Vec<KwicCabinetItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_html_debug: Option<String>,
}

/// Fetch and parse a KWIC Portal subportal page (e.g. /portal/subportal?tagCd=1)
#[tauri::command]
pub async fn kwic_fetch_subportal(
    state: State<'_, KwicState>,
    db: State<'_, crate::db::Database>,
    tag_cd: String,
) -> Result<KwicSubportalData, String> {
    if !tag_cd.chars().all(|c| c.is_ascii_digit()) {
        return Err("\u{7121}\u{52b9}\u{306a}tagCd\u{3067}\u{3059}".into());
    }
    let cache_key = format!("kwic_subportal:{}", tag_cd);
    match kwic_http(&state).await {
        Ok(http) => {
            let path = format!("/portal/subportal?tagCd={}", tag_cd);
            match kwic_get(&http, &path).await {
                Ok(html) => {
                    #[cfg(debug_assertions)]
                    {
                        if crate::should_dump_debug_html() {
                            let _ = std::fs::write(
                                std::env::temp_dir()
                                    .join(format!("kwic-portal-subportal-{}.html", tag_cd)),
                                &html,
                            );
                        }
                    }

                    let data = parse_subportal(&html);
                    if let Ok(json) = serde_json::to_string(&data) {
                        let _ = db.save_data_cache(&cache_key, &json);
                    }
                    Ok(data)
                }
                Err(e) => {
                    if let Ok(Some((json, _))) = db.get_data_cache(&cache_key) {
                        if let Ok(cached) = serde_json::from_str(&json) {
                            log::info!("{}: cache fallback ({})", cache_key, e);
                            return Ok(cached);
                        }
                    }
                    Err(e)
                }
            }
        }
        Err(e) => {
            if let Ok(Some((json, _))) = db.get_data_cache(&cache_key) {
                if let Ok(cached) = serde_json::from_str(&json) {
                    log::info!("{}: cache fallback ({})", cache_key, e);
                    return Ok(cached);
                }
            }
            Err(e)
        }
    }
}

/// Fetch and parse the KWIC student cabinet reference page.
#[tauri::command]
pub async fn kwic_fetch_cabinet_reference(
    state: State<'_, KwicState>,
    db: State<'_, crate::db::Database>,
) -> Result<KwicCabinetReference, String> {
    let cache_key = "kwic_cabinet_reference";
    match kwic_http(&state).await {
        Ok(http) => match kwic_get(&http, "/cabinet/reference").await {
            Ok(html) => {
                #[cfg(debug_assertions)]
                {
                    if crate::should_dump_debug_html() {
                        let _ = std::fs::write(
                            std::env::temp_dir().join("kwic-cabinet-reference.html"),
                            &html,
                        );
                    }
                }

                #[cfg(debug_assertions)]
                let data = {
                    let mut data = parse_cabinet_reference(&html);
                    data.raw_html_debug =
                        Some(crate::client::safe_truncate(&html, 5000).to_string());
                    data
                };
                #[cfg(not(debug_assertions))]
                let data = parse_cabinet_reference(&html);
                if let Ok(json) = serde_json::to_string(&data) {
                    let _ = db.save_data_cache(cache_key, &json);
                }
                Ok(data)
            }
            Err(e) => {
                if let Ok(Some((json, _))) = db.get_data_cache(cache_key) {
                    if let Ok(cached) = serde_json::from_str(&json) {
                        log::info!("{}: cache fallback ({})", cache_key, e);
                        return Ok(cached);
                    }
                }
                Err(e)
            }
        },
        Err(e) => {
            if let Ok(Some((json, _))) = db.get_data_cache(cache_key) {
                if let Ok(cached) = serde_json::from_str(&json) {
                    log::info!("{}: cache fallback ({})", cache_key, e);
                    return Ok(cached);
                }
            }
            Err(e)
        }
    }
}

/// Open a KWIC Portal notification detail in a native detail window
#[tauri::command]
pub async fn kwic_open_detail_window(
    app: tauri::AppHandle,
    title: String,
    information_id: String,
    information_type: String,
    person_category_cd: String,
    category_cd: String,
) -> Result<(), String> {
    let existing = app
        .webview_windows()
        .keys()
        .filter(|k| k.starts_with("kwic-detail-"))
        .count();
    if existing >= 10 {
        return Err(config::TOO_MANY_WINDOWS_MSG.into());
    }
    let id = KWIC_DETAIL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let label = format!("kwic-detail-{}", id);

    let encoded_id = urlencoding::encode(&information_id);
    let encoded_type = urlencoding::encode(&information_type);
    let encoded_person = urlencoding::encode(&person_category_cd);
    let encoded_cat = urlencoding::encode(&category_cd);
    let encoded_title = urlencoding::encode(&title);
    let url_str = format!(
        "university-detail.html?mode=kwic&informationId={}&informationType={}&personCategoryCd={}&categoryCd={}&title={}",
        encoded_id, encoded_type, encoded_person, encoded_cat, encoded_title,
    );

    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(url_str.into()))
        .initialization_script(crate::webview_toolbar::browser_bridge_script())
        .title(&title)
        .inner_size(520.0, 600.0)
        .resizable(true)
        .build()
        .map_err(|e| format!("ウィンドウ作成失敗: {}", e))?;
    crate::webview_toolbar::register_readable_window(&app, &label, &label);

    Ok(())
}

#[tauri::command]
pub async fn kwic_open_cabinet_window(
    app: tauri::AppHandle,
    title: Option<String>,
) -> Result<(), String> {
    let existing = app
        .webview_windows()
        .keys()
        .filter(|k| k.starts_with("kwic-detail-"))
        .count();
    if existing >= 10 {
        return Err(config::TOO_MANY_WINDOWS_MSG.into());
    }
    let id = KWIC_DETAIL_COUNTER.fetch_add(1, Ordering::Relaxed);
    let label = format!("kwic-detail-{}", id);
    let title = title.unwrap_or_else(|| "学生キャビネット".to_string());
    let encoded_title = urlencoding::encode(&title);
    let url_str = format!(
        "university-detail.html?mode=kwicCabinet&title={}",
        encoded_title
    );

    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::App(url_str.into()))
        .initialization_script(crate::webview_toolbar::browser_bridge_script())
        .title(&title)
        .inner_size(640.0, 720.0)
        .resizable(true)
        .build()
        .map_err(|e| format!("ウィンドウ作成失敗: {}", e))?;
    crate::webview_toolbar::register_readable_window(&app, &label, &label);

    Ok(())
}

/// Open a link from the KWIC Portal subportal.
/// For kwansei.ac.jp domains, open in a webview window with cookies injected from reqwest.
/// For external domains, open in the system browser.
#[tauri::command]
pub async fn kwic_open_link(
    app: tauri::AppHandle,
    url: String,
    title: String,
) -> Result<(), String> {
    // Only allow http/https
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("無効なURLスキームです".into());
    }

    // Check if this is a kwansei domain → open in webview
    let is_kwansei = url.contains("kwansei.ac.jp");
    let is_kwic = url.contains("kwic.kwansei.ac.jp");

    if is_kwansei {
        let id = KWIC_DETAIL_COUNTER.fetch_add(1, Ordering::Relaxed);
        let label = format!("kwic-detail-{}", id);

        if is_kwic {
            // KWIC Portal: needs special handling because KWIC shows its own login page
            // instead of redirecting to Okta SSO directly.
            // Solution: navigate to KWIC's SAML login URL first (which goes directly to Okta SSO).
            // WKWebView shares Okta SSO cookies from the login flow, so Okta auto-authenticates.
            // After SAML completes, KWIC sets session cookies and redirects to /portal/home.
            // Our initialization_script then redirects to the actual target URL.
            let saml_url: url::Url = config::KWIC_SAML_URL
                .parse()
                .expect("hardcoded KWIC SAML URL is valid");

            // Escape the target URL for safe embedding in JS
            let escaped_url = url
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
                .replace('<', "\\x3c")
                .replace('>', "\\x3e");

            // Script runs on every page load in this webview.
            // When we land on a KWIC portal page (= authenticated), redirect to target.
            // sessionStorage prevents infinite redirect loop.
            let redirect_script = format!(
                r#"(function() {{
                    if (window.location.hostname === 'kwic.kwansei.ac.jp'
                        && window.location.pathname.startsWith('/portal/')
                        && !sessionStorage.getItem('__kwic_nav_done')) {{
                        sessionStorage.setItem('__kwic_nav_done', '1');
                        window.location.replace('{}');
                    }}
                }})();"#,
                escaped_url
            );

            crate::webview_toolbar::create_browser_window(
                &app,
                &label,
                tauri::WebviewUrl::External(saml_url),
                &title,
                1000.0,
                750.0,
                &[&redirect_script],
            )?;
        } else {
            // Other kwansei.ac.jp domains (kg-course, library, etc.)
            // These redirect directly to Okta SSO, which auto-authenticates
            // via shared WKWebView cookies. No special handling needed.
            let parsed: url::Url = url.parse().map_err(|e| format!("URL parse error: {}", e))?;

            crate::webview_toolbar::create_browser_window(
                &app,
                &label,
                tauri::WebviewUrl::External(parsed),
                &title,
                1000.0,
                750.0,
                &[],
            )?;
        }
    } else {
        // External link → in-app browser webview
        crate::commands::open_external_url(app, url, Some(title)).await?;
    }

    Ok(())
}

// ============ Parsers ============
// Based on actual KWIC Portal HTML structure (kwic.kwansei.ac.jp)
//
// Home page layout:
//   - .portal-notice: pinned important links
//   - .portal-mainlink: 9 category cards (授業・履修・成績, キャンパスライフ, etc.)
//   - .portal-info-tab: 4 notification tabs
//     - #portalinfocontent1: 呼出し・重要なお知らせ
//     - #portalinfocontent2: 学部・研究科からのお知らせ
//     - #portalinfocontent3: 授業のお知らせ
//     - #portalinfocontent4: その他
//   - Each notification item: li.portal-info-content-li
//     - a[data1=informationId]
//     - .portal-subblock-infolist-left-item2 > div (date)
//     - .portal-subblock-infolist-left-item2 > span (title)
//     - .portal-subblock-infolist-right (department/category)
//     - .portal-information-new (NEW badge)

fn parse_portal_home(html: &str) -> Vec<KwicPortalSection> {
    use scraper::Html;

    let document = Html::parse_document(html);
    let mut sections = Vec::new();

    // 1. Parse pinned important links (注目コンテンツ)
    {
        let sel = &*SEL_NOTICE_A;
        let items: Vec<KwicPortalItem> = document
            .select(sel)
            .filter_map(|a| {
                let title: String = a.text().collect::<Vec<_>>().join(" ").trim().to_string();
                let href = a.value().attr("href").unwrap_or_default();
                if title.is_empty() {
                    return None;
                }
                Some(KwicPortalItem {
                    id: String::new(),
                    title,
                    date: String::new(),
                    category: "注目".to_string(),
                    url: href.to_string(),
                    important: true,
                    information_type: String::new(),
                    person_category_cd: String::new(),
                    category_cd: String::new(),
                })
            })
            .collect();
        if !items.is_empty() {
            sections.push(KwicPortalSection {
                title: "注目コンテンツ".to_string(),
                items,
            });
        }
    }

    // 2. Parse notification tabs
    let tabs: [(&scraper::Selector, &str); 4] = [
        (&*SEL_TAB1_LI, "呼出し・重要なお知らせ"),
        (&*SEL_TAB2_LI, "学部・研究科からのお知らせ"),
        (&*SEL_TAB3_LI, "授業のお知らせ"),
        (&*SEL_TAB4_LI, "その他"),
    ];

    for (sel, tab_title) in &tabs {
        let items: Vec<KwicPortalItem> = document
            .select(sel)
            .filter_map(|li| {
                parse_info_item(&li).map(|(mut item, d2, d3, d4)| {
                    item.information_type = d2;
                    item.person_category_cd = d3;
                    item.category_cd = d4;
                    item
                })
            })
            .collect();
        if !items.is_empty() {
            sections.push(KwicPortalSection {
                title: tab_title.to_string(),
                items,
            });
        }
    }

    // 3. Parse main link categories (メインリンク)
    {
        let sel = &*SEL_MAINLINK_A;
        let items: Vec<KwicPortalItem> = document
            .select(sel)
            .filter_map(|a| {
                let title: String = a.text().collect::<Vec<_>>().join(" ").trim().to_string();
                let href = a.value().attr("href").unwrap_or_default();
                if title.is_empty() {
                    return None;
                }
                Some(KwicPortalItem {
                    id: String::new(),
                    title,
                    date: String::new(),
                    category: "リンク".to_string(),
                    url: if href.starts_with("http") {
                        href.to_string()
                    } else {
                        format!("{}{}", config::KWIC_BASE, href)
                    },
                    important: false,
                    information_type: String::new(),
                    person_category_cd: String::new(),
                    category_cd: String::new(),
                })
            })
            .collect();
        if !items.is_empty() {
            sections.push(KwicPortalSection {
                title: "メインリンク".to_string(),
                items,
            });
        }
    }

    sections
}

fn notification_tab_selectors() -> [(&'static scraper::Selector, &'static str); 4] {
    [
        (&*SEL_TAB1_LI, "呼出し・重要なお知らせ"),
        (&*SEL_TAB2_LI, "学部・研究科からのお知らせ"),
        (&*SEL_TAB3_LI, "授業のお知らせ"),
        (&*SEL_TAB4_LI, "その他"),
    ]
}

fn section_allows_kwic_list_merge(title: &str) -> bool {
    matches!(
        title,
        "呼出し・重要なお知らせ" | "学部・研究科からのお知らせ" | "その他"
    )
}

fn information_type_section_title(information_type: &str) -> Option<&'static str> {
    match information_type {
        "10" => Some("呼出し・重要なお知らせ"),
        "12" => Some("その他"),
        _ => None,
    }
}

fn apply_info_item_data(
    mut item: KwicPortalItem,
    data2: String,
    data3: String,
    data4: String,
) -> KwicPortalItem {
    item.information_type = data2;
    item.person_category_cd = data3;
    item.category_cd = data4;
    item
}

fn notification_item_key(item: &KwicPortalItem) -> String {
    if !item.id.trim().is_empty() {
        return format!("id:{}", item.id.trim());
    }
    format!("text:{}|{}", item.title.trim(), item.date.trim())
}

fn push_unique_notification_item(section: &mut KwicPortalSection, item: KwicPortalItem) -> bool {
    let key = notification_item_key(&item);
    if section
        .items
        .iter()
        .any(|existing| notification_item_key(existing) == key)
    {
        return false;
    }
    section.items.push(item);
    true
}

fn merge_items_into_section(
    sections: &mut Vec<KwicPortalSection>,
    title: &str,
    items: Vec<KwicPortalItem>,
) -> usize {
    if items.is_empty() {
        return 0;
    }
    if let Some(section) = sections.iter_mut().find(|section| section.title == title) {
        let mut added = 0;
        for item in items {
            if push_unique_notification_item(section, item) {
                added += 1;
            }
        }
        added
    } else {
        let count = items.len();
        sections.push(KwicPortalSection {
            title: title.to_string(),
            items,
        });
        count
    }
}

fn selected_information_type(document: &scraper::Html, fallback: Option<&str>) -> String {
    document
        .select(&SEL_INFO_TYPE_SELECTED)
        .next()
        .and_then(|option| option.value().attr("value"))
        .filter(|value| !value.trim().is_empty())
        .or(fallback)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn parse_information_list_items(
    document: &scraper::Html,
    fallback_information_type: Option<&str>,
) -> Option<(&'static str, Vec<KwicPortalItem>)> {
    let information_type = selected_information_type(document, fallback_information_type);
    let section_title = information_type_section_title(&information_type)?;
    let mut items = Vec::new();

    for row in document.select(&SEL_INFO_LIST_ROW) {
        let Some(title_el) = row.select(&SEL_INFO_LIST_TITLE).next() else {
            continue;
        };
        let id = title_el
            .value()
            .attr("data1")
            .unwrap_or_default()
            .trim()
            .to_string();
        let category_cd = title_el
            .value()
            .attr("data2")
            .unwrap_or_default()
            .trim()
            .to_string();
        let title = normalize_text(&title_el.text().collect::<Vec<_>>().join(" "));
        if id.is_empty() || title.is_empty() {
            continue;
        }

        let date = row
            .select(&SEL_INFO_LIST_DATE)
            .next()
            .map(|el| normalize_text(&el.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();
        let category = row
            .select(&SEL_INFO_LIST_DIVISION)
            .next()
            .map(|el| normalize_text(&el.text().collect::<Vec<_>>().join(" ")))
            .unwrap_or_default();

        items.push(KwicPortalItem {
            id: id.clone(),
            title,
            date,
            category,
            url: format!(
                "{}/portal/home/information/detail?informationId={}&directLink=1",
                config::KWIC_BASE,
                id
            ),
            important: false,
            information_type: information_type.clone(),
            person_category_cd: "0".to_string(),
            category_cd,
        });
    }

    Some((section_title, items))
}

fn merge_information_list_sections(
    sections: &mut Vec<KwicPortalSection>,
    html: &str,
    fallback_information_type: Option<&str>,
) -> (usize, usize) {
    use scraper::Html;
    let document = Html::parse_document(html);

    let mut merged_from_tabs = false;
    let mut parsed_count = 0;
    let mut merged_count = 0;
    for (selector, title) in notification_tab_selectors() {
        if !section_allows_kwic_list_merge(title) {
            continue;
        }
        let items: Vec<KwicPortalItem> = document
            .select(selector)
            .filter_map(|li| {
                parse_info_item(&li)
                    .map(|(item, d2, d3, d4)| apply_info_item_data(item, d2, d3, d4))
            })
            .collect();
        if items.is_empty() {
            continue;
        }
        merged_from_tabs = true;
        parsed_count += items.len();
        if let Some(section) = sections.iter_mut().find(|section| section.title == title) {
            for item in items {
                if push_unique_notification_item(section, item) {
                    merged_count += 1;
                }
            }
        } else {
            merged_count += items.len();
            sections.push(KwicPortalSection {
                title: title.to_string(),
                items,
            });
        }
    }
    if merged_from_tabs {
        return (parsed_count, merged_count);
    }

    if let Some((section_title, items)) =
        parse_information_list_items(&document, fallback_information_type)
    {
        let parsed = items.len();
        let merged = merge_items_into_section(sections, section_title, items);
        if parsed > 0 {
            return (parsed, merged);
        }
    }

    let category_to_section = sections
        .iter()
        .filter(|section| section_allows_kwic_list_merge(&section.title))
        .flat_map(|section| {
            section
                .items
                .iter()
                .filter(|item| !item.category_cd.is_empty())
                .map(|item| (item.category_cd.clone(), section.title.clone()))
        })
        .collect::<std::collections::HashMap<_, _>>();

    for li in document.select(&SEL_INFO_LI) {
        let Some((item, d2, d3, d4)) = parse_info_item(&li) else {
            continue;
        };
        let Some(section_title) = category_to_section.get(&d4).cloned() else {
            continue;
        };
        let item = apply_info_item_data(item, d2, d3, d4);
        if let Some(section) = sections
            .iter_mut()
            .find(|section| section.title == section_title)
        {
            if push_unique_notification_item(section, item) {
                merged_count += 1;
            }
        }
    }
    (merged_count, merged_count)
}

/// Parse a single notification item from li.portal-info-content-li
/// Returns (KwicPortalItem, data2, data3, data4)
fn parse_info_item(li: &scraper::ElementRef) -> Option<(KwicPortalItem, String, String, String)> {
    // Extract informationId and data attributes from `a[data1]`
    let a = li.select(&SEL_INFO_A).next()?;
    let id = a.value().attr("data1").unwrap_or_default().to_string();
    let data2 = a.value().attr("data2").unwrap_or_default().to_string();
    let data3 = a.value().attr("data3").unwrap_or_default().to_string();
    let data4 = a.value().attr("data4").unwrap_or_default().to_string();

    // Date: .portal-subblock-infolist-left-item2 > div
    let date = li
        .select(&SEL_INFO_DATE)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
        .unwrap_or_default();

    // Title: .portal-subblock-infolist-left-item2 > span
    let mut title = li
        .select(&SEL_INFO_TITLE)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
        .unwrap_or_default();
    if title.is_empty() {
        title = normalize_text(&a.text().collect::<Vec<_>>().join(" "));
    }

    if title.is_empty() {
        return None;
    }

    // Category/department: .portal-subblock-infolist-right
    let category = li
        .select(&SEL_INFO_CATEGORY)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
        .unwrap_or_default();

    Some((
        KwicPortalItem {
            id: id.clone(),
            title,
            date,
            category,
            url: format!(
                "{}/portal/home/information/detail?informationId={}&directLink=1",
                config::KWIC_BASE,
                id
            ),
            important: false,
            information_type: String::new(),
            person_category_cd: String::new(),
            category_cd: String::new(),
        },
        data2,
        data3,
        data4,
    ))
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn hidden_value(row: &scraper::ElementRef, class_name: &str) -> String {
    let selector = match scraper::Selector::parse(&format!("input.{}", class_name)) {
        Ok(sel) => sel,
        Err(_) => return String::new(),
    };
    row.select(&selector)
        .next()
        .and_then(|el| el.value().attr("value"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn absolute_kwic_url(path_or_url: &str) -> String {
    if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        path_or_url.to_string()
    } else if path_or_url.starts_with('/') {
        format!("{}{}", config::KWIC_BASE, path_or_url)
    } else {
        format!("{}/{}", config::KWIC_BASE, path_or_url)
    }
}

fn cabinet_direct_url(list_url: &str, cabinet_id: &str) -> String {
    let base_path = if list_url.trim().is_empty() {
        "/cabinet/reference?typeCd=0"
    } else {
        list_url.trim()
    };
    let absolute = absolute_kwic_url(base_path);
    match url::Url::parse(&absolute) {
        Ok(mut url) => {
            let has_cabinet = url.query_pairs().any(|(key, _)| key == "cabinetId");
            let has_direct = url.query_pairs().any(|(key, _)| key == "directLink");
            {
                let mut pairs = url.query_pairs_mut();
                if !has_cabinet && !cabinet_id.is_empty() {
                    pairs.append_pair("cabinetId", cabinet_id);
                }
                if !has_direct {
                    pairs.append_pair("directLink", "1");
                }
            }
            url.to_string()
        }
        Err(_) => absolute,
    }
}

fn parse_cabinet_reference(html: &str) -> KwicCabinetReference {
    use scraper::Html;
    let doc = Html::parse_document(html);
    let mut items = Vec::new();

    for row in doc.select(&SEL_CABINET_ROW) {
        let cabinet_id = hidden_value(&row, "listCabinetId");
        let mut name = hidden_value(&row, "listCabinetName");
        let level = hidden_value(&row, "listCabinetLevel")
            .parse::<u32>()
            .unwrap_or(0);
        let list_url = hidden_value(&row, "listUrl");

        if name.is_empty() {
            name = row
                .select(&SEL_CABINET_TITLE)
                .next()
                .map(|el| normalize_text(&el.text().collect::<Vec<_>>().join(" ")))
                .unwrap_or_default();
        }
        if name.is_empty() || cabinet_id.is_empty() {
            continue;
        }

        let updated_at = row
            .select(&SEL_CABINET_NEW)
            .next()
            .and_then(|el| el.value().attr("data-value"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                row.select(&SEL_CABINET_DATE)
                    .next()
                    .map(|el| normalize_text(&el.text().collect::<Vec<_>>().join(" ")))
            })
            .unwrap_or_default();
        let is_new = row
            .select(&SEL_CABINET_NEW)
            .next()
            .map(|el| !el.value().classes().any(|class| class == "not-new"))
            .unwrap_or(false);
        let list_id = row.value().attr("id").unwrap_or_default().to_string();
        let url = cabinet_direct_url(&list_url, &cabinet_id);

        items.push(KwicCabinetItem {
            cabinet_id,
            list_id,
            name,
            level,
            updated_at,
            is_new,
            url,
        });
    }

    KwicCabinetReference {
        title: "学生キャビネット".to_string(),
        items,
        raw_html_debug: None,
    }
}

/// Extract CSRF token from KWIC Portal HTML
fn extract_csrf_token(html: &str) -> Option<String> {
    use scraper::Html;
    let doc = Html::parse_document(html);
    if let Some(el) = doc.select(&SEL_CSRF).next() {
        return el.value().attr("value").map(|v| v.to_string());
    }
    None
}

/// Parse the detail HTML fragment returned by /lms/course/information/listdetail.
/// This is typically a dialog fragment containing info_preview with title, body, sender, date, attachments.
fn parse_detail_html(html: &str) -> KwicNotificationDetail {
    use scraper::Html;
    let doc = Html::parse_document(html);

    let text_of = |sel: &scraper::Selector| -> String {
        doc.select(sel)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default()
    };

    let html_of = |sel: &scraper::Selector| -> String {
        doc.select(sel)
            .next()
            .map(|el| el.inner_html().trim().to_string())
            .unwrap_or_default()
    };

    // Real KWIC detail structure:
    // Title: .block-title-txt
    // Body:  #contentsHtml (quill editor content)
    // Sender: .portal-information-outgoing-division (contains "配信部署:" + dept name)
    // Date:  掲載期間 section — we extract from the first .contents-input-area with date-like text
    let title = text_of(&SEL_BLOCK_TITLE);
    let body_html = html_of(&SEL_CONTENTS_HTML);

    // Sender: extract department from .portal-information-outgoing-division
    let sender = {
        let raw = text_of(&SEL_OUTGOING_DIV);
        raw.replace("配信部署:", "").trim().to_string()
    };

    // Date: look for 掲載期間 section, then get the spans inside its .contents-input-area
    let date = {
        let mut found = String::new();
        for detail in doc.select(&SEL_CONTENTS_DETAIL) {
            if let Some(header) = detail.select(&SEL_HEADER_BOLD).next() {
                let header_text = header.text().collect::<Vec<_>>().join("");
                if header_text.contains("掲載期間") {
                    if let Some(input) = detail.select(&SEL_INPUT_AREA).next() {
                        found = input.text().collect::<Vec<_>>().join("").trim().to_string();
                    }
                    break;
                }
            }
        }
        found
    };

    // Attachments: .file-object elements → .downloadFile (name), .objectName (object path)
    let mut attachments = Vec::new();
    for fo in doc.select(&SEL_FILE_OBJECT) {
        let name = fo
            .select(&SEL_FILE_NAME)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();
        let object_name = fo
            .select(&SEL_OBJECT_NAME)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let url = format!(
            "{}/portal/home/information/detail/download?downloadFileName={}&objectName={}&downloadMode=1",
            config::KWIC_BASE,
            urlencoding::encode(&name),
            urlencoding::encode(&object_name),
        );
        attachments.push(KwicAttachment { name, url });
    }

    // Strip <script> tags from body for safety
    let body_clean = {
        static RE_SCRIPT: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("valid regex")
        });
        RE_SCRIPT.replace_all(&body_html, "").to_string()
    };

    KwicNotificationDetail {
        title,
        date,
        sender,
        body_html: body_clean,
        attachments,
    }
}

/// Parse a KWIC Portal subportal page.
/// Subportal pages contain link lists and notification items similar to the home page.
fn parse_subportal(html: &str) -> KwicSubportalData {
    use scraper::Html;
    let doc = Html::parse_document(html);

    // Page title: .subportal-title-txt
    let page_title = doc
        .select(&SEL_SUBPORTAL_TITLE)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
        .unwrap_or_default();

    // Links: li.subportal-block-relation-list-li a.subportal-block-txtlink-li-b
    // Each <a> contains <img class="systemlink-image"> (icon) + <span> (title)
    let mut links = Vec::new();
    for a in doc.select(&SEL_SUBPORTAL_LINK) {
        let title: String = a.text().collect::<Vec<_>>().join("").trim().to_string();
        let href = a.value().attr("href").unwrap_or_default();
        if title.is_empty() || href.is_empty() || href == "#" {
            continue;
        }
        if href.starts_with("javascript:") {
            continue;
        }
        let url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{}{}", config::KWIC_BASE, href)
        };
        let icon_url = a
            .select(&SEL_SYSTEM_IMAGE)
            .next()
            .and_then(|img| img.value().attr("src"))
            .map(|src| {
                if src.starts_with("http") {
                    src.to_string()
                } else {
                    format!("{}{}", config::KWIC_BASE, src)
                }
            })
            .unwrap_or_default();
        if links.iter().any(|l: &KwicSubportalLink| l.url == url) {
            continue;
        }
        links.push(KwicSubportalLink {
            title,
            url,
            icon_url,
            description: String::new(),
        });
    }

    // Notifications: li.subportal-block-info-list-li
    // Structure per item:
    //   .subportal-block-list-li-txt-info1 = category
    //   .subportal-block-list-li-txt-info2 span.link-txt[data1][data2] = title + id + type
    //   .subportal-block-list-li-txt-info3 span:first = date
    //   .subportal-block-list-li-txt-info4 = department
    let mut notifications = Vec::new();
    for li in doc.select(&SEL_SUBPORTAL_LI) {
        let title_el = match li.select(&SEL_SUBPORTAL_TITLE_SPAN).next() {
            Some(el) => el,
            None => continue,
        };

        let id = title_el
            .value()
            .attr("data1")
            .unwrap_or_default()
            .to_string();
        let data2 = title_el
            .value()
            .attr("data2")
            .unwrap_or_default()
            .to_string();
        let title = title_el
            .text()
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string();
        if title.is_empty() {
            continue;
        }

        let category = li
            .select(&SEL_SUBPORTAL_CAT)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();

        let date = li
            .select(&SEL_SUBPORTAL_DATE)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();

        let dept = li
            .select(&SEL_SUBPORTAL_DEPT)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join("").trim().to_string())
            .unwrap_or_default();

        notifications.push(KwicPortalNotification {
            id,
            title,
            date,
            category: if !dept.is_empty() { dept } else { category },
            important: false,
            information_type: data2,
            // Subportal notifications only have data1/data2 in onclick
            person_category_cd: String::new(),
            category_cd: String::new(),
        });
    }

    KwicSubportalData {
        title: page_title,
        links,
        notifications,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kwic_information_list_rows() {
        let html = r#"
        <div id="information_list">
          <select id="informationType" name="informationType">
            <option value="10" selected="selected">呼び出し・重要なお知らせ</option>
            <option value="11">授業のお知らせ</option>
            <option value="12">その他</option>
          </select>
          <div class="contents-list">
            <div class="contents-display-flex-exchange-sp contents-display-flex-padding-sp result-list">
              <div class="portal-information-list-title sp-contents-hidden">
                <span id="title_1667108" class="link-txt break" data1="1667108" data2="02">6月3日（水）のシャトルバスの運行について</span>
                <span class="portal-information-priority portal-information-priority-urgency-color">NEW</span>
              </div>
              <div class="portal-information-list-date sp-contents-hidden">
                <span>2026/06/02 17:05</span>
                <span class="contents-time-to"></span>
                <span>2026/06/04 00:00</span>
              </div>
              <div class="portal-information-list-division sp-contents-hidden">学生課</div>
            </div>
            <div class="contents-display-flex-exchange-sp contents-display-flex-padding-sp result-list">
              <div class="portal-information-list-title sp-contents-hidden">
                <span id="title_1662480" class="link-txt break" data1="1662480" data2="04">【保健館より】尿の再検査が必要です</span>
              </div>
              <div class="portal-information-list-date sp-contents-hidden">
                <span>2026/05/28 09:00</span>
                <span class="contents-time-to"></span>
                <span>2026/06/30 00:00</span>
              </div>
              <div class="portal-information-list-division sp-contents-hidden">保健館</div>
            </div>
          </div>
        </div>
        "#;

        let document = scraper::Html::parse_document(html);
        let (section, items) = parse_information_list_items(&document, Some("10")).unwrap();

        assert_eq!(section, "呼出し・重要なお知らせ");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "1667108");
        assert_eq!(items[0].information_type, "10");
        assert_eq!(items[0].category_cd, "02");
        assert_eq!(items[0].date, "2026/06/02 17:05");
        assert_eq!(items[0].category, "学生課");
        assert_eq!(items[1].category_cd, "04");
    }

    #[test]
    fn parses_kwic_other_information_list_rows() {
        let html = r#"
        <div id="information_list">
          <select id="informationType" name="informationType">
            <option value="10">呼び出し・重要なお知らせ</option>
            <option value="11">授業のお知らせ</option>
            <option value="12" selected="selected">その他</option>
          </select>
          <div class="contents-list">
            <div class="contents-display-flex-exchange-sp contents-display-flex-padding-sp result-list">
              <div class="portal-information-list-title sp-contents-hidden">
                <span id="title_1660000" class="link-txt break" data1="1660000" data2="0">その他のお知らせ</span>
              </div>
              <div class="portal-information-list-date sp-contents-hidden">
                <span>2026/06/03 10:00</span>
                <span class="contents-time-to"></span>
                <span>2026/06/30 00:00</span>
              </div>
              <div class="portal-information-list-division sp-contents-hidden">学生課</div>
            </div>
          </div>
        </div>
        "#;

        let document = scraper::Html::parse_document(html);
        let (section, items) = parse_information_list_items(&document, Some("12")).unwrap();

        assert_eq!(section, "その他");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].information_type, "12");
        assert_eq!(items[0].category_cd, "0");
    }

    #[test]
    fn merges_information_list_without_duplicate_ids() {
        let html = r#"
        <div id="information_list">
          <select id="informationType" name="informationType">
            <option value="10" selected="selected">呼び出し・重要なお知らせ</option>
          </select>
          <div class="contents-list">
            <div class="contents-display-flex-exchange-sp contents-display-flex-padding-sp result-list">
              <div class="portal-information-list-title sp-contents-hidden">
                <span id="title_1667108" class="link-txt break" data1="1667108" data2="02">既存のお知らせ</span>
              </div>
              <div class="portal-information-list-date sp-contents-hidden"><span>2026/06/02 17:05</span></div>
              <div class="portal-information-list-division sp-contents-hidden">学生課</div>
            </div>
            <div class="contents-display-flex-exchange-sp contents-display-flex-padding-sp result-list">
              <div class="portal-information-list-title sp-contents-hidden">
                <span id="title_1667109" class="link-txt break" data1="1667109" data2="02">新しいお知らせ</span>
              </div>
              <div class="portal-information-list-date sp-contents-hidden"><span>2026/06/03 10:00</span></div>
              <div class="portal-information-list-division sp-contents-hidden">学生課</div>
            </div>
          </div>
        </div>
        "#;
        let mut sections = vec![KwicPortalSection {
            title: "呼出し・重要なお知らせ".to_string(),
            items: vec![KwicPortalItem {
                id: "1667108".to_string(),
                title: "既存のお知らせ".to_string(),
                date: "2026/06/02 17:05".to_string(),
                category: "学生課".to_string(),
                url: String::new(),
                important: false,
                information_type: "10".to_string(),
                person_category_cd: "0".to_string(),
                category_cd: "02".to_string(),
            }],
        }];

        let (parsed, added) = merge_information_list_sections(&mut sections, html, Some("10"));

        assert_eq!(parsed, 2);
        assert_eq!(added, 1);
        assert_eq!(sections[0].items.len(), 2);
        assert!(sections[0].items.iter().any(|item| item.id == "1667109"));
    }

    #[test]
    fn skips_kwic_class_information_list_rows() {
        let html = r#"
        <div id="information_list">
          <select id="informationType" name="informationType">
            <option value="11" selected="selected">授業のお知らせ</option>
          </select>
          <div class="contents-list">
            <div class="contents-display-flex-exchange-sp contents-display-flex-padding-sp result-list">
              <div class="portal-information-list-title sp-contents-hidden">
                <span id="title_1667110" class="link-txt break" data1="1667110" data2="0">授業のお知らせ</span>
              </div>
              <div class="portal-information-list-date sp-contents-hidden"><span>2026/06/03 11:00</span></div>
              <div class="portal-information-list-division sp-contents-hidden">教務課</div>
            </div>
          </div>
        </div>
        "#;
        let mut sections = Vec::new();

        let (parsed, added) = merge_information_list_sections(&mut sections, html, Some("11"));

        assert_eq!(parsed, 0);
        assert_eq!(added, 0);
        assert!(sections.is_empty());
    }

    #[test]
    fn parses_kwic_cabinet_reference_rows() {
        let html = r#"
        <div class="block block-area clearfix cabinetList">
          <div class="contents-list">
            <div class="result-list contents-display-flex result-data type-list" id="cabinetList_126">
              <input type="hidden" value="216" name="cabinetId" class="listCabinetId">
              <input type="hidden" value="教務機構" name="cabinetName" class="listCabinetName">
              <input type="hidden" value="1" name="cabinetLevel" class="listCabinetLevel">
              <input type="hidden" value="/cabinet/reference?typeCd=0" class="listUrl">
              <div class="cabinet-view-list-item">
                <div class="cabinet-view-list-name">
                  <a class="cabinet-area-title-txt cabinetDisplayLink cabinet-title-omit-sp">教務機構</a>
                </div>
                <div class="cabinet-view-list-new">
                  <span class="cabinet-area-new not-new" data-value="2026/05/19">NEW</span>
                </div>
                <div class="cabinet-view-list-createdate"><span>2026/05/19</span></div>
              </div>
            </div>
            <div class="result-list contents-display-flex result-data type-list" id="cabinetList_1415">
              <input type="hidden" value="264" name="cabinetId" class="listCabinetId">
              <input type="hidden" value="国際教育・協力センター（CIEC）: 海外への留学" name="cabinetName" class="listCabinetName">
              <input type="hidden" value="1" name="cabinetLevel" class="listCabinetLevel">
              <input type="hidden" value="/cabinet/reference?typeCd=0" class="listUrl">
              <div class="cabinet-view-list-item">
                <div class="cabinet-view-list-name"><a class="cabinetDisplayLink">国際教育・協力センター（CIEC）: 海外への留学</a></div>
                <div class="cabinet-view-list-new"><span class="cabinet-area-new" data-value="2026/05/27">NEW</span></div>
                <div class="cabinet-view-list-createdate"><span>2026/05/27</span></div>
              </div>
            </div>
          </div>
        </div>
        "#;

        let parsed = parse_cabinet_reference(html);

        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].cabinet_id, "216");
        assert_eq!(parsed.items[0].name, "教務機構");
        assert_eq!(parsed.items[0].updated_at, "2026/05/19");
        assert!(!parsed.items[0].is_new);
        assert_eq!(parsed.items[1].cabinet_id, "264");
        assert!(parsed.items[1].is_new);
        assert!(parsed.items[1].url.contains("/cabinet/reference?"));
        assert!(parsed.items[1].url.contains("typeCd=0"));
        assert!(parsed.items[1].url.contains("cabinetId=264"));
        assert!(parsed.items[1].url.contains("directLink=1"));
    }
}
