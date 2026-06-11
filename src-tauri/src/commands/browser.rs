use crate::config;
use tauri::Manager;

/// Open an external URL in a new webview window with browser toolbar
#[tauri::command]
pub async fn open_external_url(
    app: tauri::AppHandle,
    url: String,
    title: Option<String>,
) -> Result<crate::webview_toolbar::BrowserWindowInfo, String> {
    crate::document_tabs::open_external_tab(&app, url, title)
}

/// Open a URL in the system default browser (Safari, Chrome, etc.)
#[tauri::command]
pub async fn open_in_system_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let parsed: url::Url = url.parse().map_err(|e| format!("URL parse error: {}", e))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("Unsupported URL scheme: {}", scheme));
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| format!("ブラウザを開けませんでした: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn open_profile_edit_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_window("profile-edit") {
        let _ = win.set_focus();
        return Ok(());
    }

    let url: url::Url = format!("{}/uniasv2/UnSSOLoginControl2?REQ_LOGIN_NO=2&REQ_ACTION_DO=/GGA110.do&REQ_PRFR_MNU_ID=MNUIDSTD0104011", config::KG_COURSE_BASE)
        .parse()
        .map_err(|e| format!("URL parse error: {}", e))?;

    crate::document_tabs::open_external_tab(&app, url.to_string(), Some("個人情報編集".into()))?;

    Ok(())
}

#[tauri::command]
pub async fn open_facility_reservation(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_window("facility-rsv") {
        let _ = win.set_focus();
        return Ok(());
    }

    let url: url::Url = "https://facility-rsv.kwansei.ac.jp/ss/top"
        .parse()
        .map_err(|e| format!("URL parse error: {}", e))?;

    crate::document_tabs::open_external_tab(&app, url.to_string(), Some("施設予約".into()))?;

    Ok(())
}

#[tauri::command]
pub async fn open_registration_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_window("registration") {
        let _ = win.set_focus();
        return Ok(());
    }

    let url: url::Url = format!("{}/uniasv2/UnSSOLoginControl2?REQ_LOGIN_NO=2&REQ_ACTION_DO=/ARD010.do&REQ_PRFR_MNU_ID=MNUIDSTD0102012&SE_LANGUAGE=", config::KG_COURSE_BASE)
        .parse()
        .map_err(|e| format!("URL parse error: {}", e))?;

    crate::document_tabs::open_external_tab(&app, url.to_string(), Some("履修登録".into()))?;

    Ok(())
}

#[tauri::command]
pub async fn open_detective_tab(app: tauri::AppHandle) -> Result<(), String> {
    crate::document_tabs::open_detective_tab(&app)?;
    Ok(())
}

#[tauri::command]
pub async fn open_files_tab(
    app: tauri::AppHandle,
    focus_course: Option<String>,
) -> Result<(), String> {
    use tauri::Emitter;
    let course = focus_course
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| super::downloads::simplify_course_name(s).trim().to_string())
        .filter(|s| !s.is_empty());

    let info = crate::document_tabs::open_files_tab(&app, course.clone(), "ファイル".to_string())?;

    // The course is encoded in the tab URL on first creation, but when the tab
    // already exists open_files_tab just re-focuses it — re-emit focus-course so
    // the surface narrows to (or clears) the requested course either way.
    let _ = app.emit_to(
        tauri::EventTarget::AnyLabel {
            label: info.target,
        },
        "focus-course",
        course.unwrap_or_default(),
    );

    Ok(())
}
