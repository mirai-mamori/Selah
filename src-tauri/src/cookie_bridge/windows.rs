//! Windows: extract cookies from WebView2 via Chrome DevTools Protocol.
//!
//! Uses `Network.getAllCookies` CDP method through `ICoreWebView2::CallDevToolsProtocolMethod`.
//! The callback plumbing relies on `webview2-com`'s handler helper types.
//!
//! NOTE: The exact `webview2-com` version must be compatible with the version
//! that Tauri's `wry` uses internally.  If a version mismatch causes a build
//! error, adjust the version in `Cargo.toml` to match `wry`'s dependency.

use super::CookieData;
use tauri::Manager;

// ── CDP JSON response structs ───────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct CdpCookiesResponse {
    cookies: Vec<CdpCookie>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CdpCookie {
    name: String,
    value: String,
    domain: String,
    path: String,
    expires: f64,
    http_only: bool,
    secure: bool,
    session: bool,
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Extract all cookies from the WebView2 cookie store using CDP.
///
/// Finds any active `WebviewWindow`, dispatches to its main thread via
/// `with_webview`, calls the DevTools protocol, and parses the JSON result.
pub(super) async fn extract_all_cookies(app: &tauri::AppHandle) -> Result<Vec<CookieData>, String> {
    // Try to find an active webview window (in priority order).
    let win = app
        .get_webview_window("login")
        .or_else(|| app.get_webview_window("kgc-headless"))
        .or_else(|| app.get_webview_window("luna-headless"))
        .or_else(|| app.get_webview_window("kwic-headless"))
        .or_else(|| app.get_webview_window("main"))
        .ok_or("No webview window available for cookie extraction")?;

    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let tx = std::sync::Mutex::new(Some(tx));

    win.with_webview(move |webview| {
        unsafe {
            use webview2_com::CallDevToolsProtocolMethodCompletedHandler;

            let core_webview = webview
                .controller()
                .CoreWebView2()
                .expect("CoreWebView2 must be available after SAML loading");

            // Build wide-string parameters for the CDP call.
            let method: Vec<u16> = "Network.getAllCookies\0".encode_utf16().collect();
            let params: Vec<u16> = "{}\0".encode_utf16().collect();

            let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                move |error_code, return_json| {
                    let result = if error_code.is_ok() {
                        Ok(return_json)
                    } else {
                        Err(format!("CDP call failed: {:?}", error_code))
                    };
                    if let Some(sender) = tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        let _ = sender.send(result);
                    }
                    Ok(())
                },
            ));

            // PCWSTR from windows-core 0.61 matches webview2-com-sys 0.38's expected types.
            core_webview
                .CallDevToolsProtocolMethod(
                    windows_core::PCWSTR(method.as_ptr()),
                    windows_core::PCWSTR(params.as_ptr()),
                    &handler,
                )
                .expect("CallDevToolsProtocolMethod dispatch failed");
        }
    })
    .map_err(|e| format!("with_webview failed: {}", e))?;

    let json = rx
        .await
        .map_err(|_| "Cookie extraction channel closed".to_string())?
        .map_err(|e| format!("CDP cookie extraction failed: {}", e))?;

    let response: CdpCookiesResponse = serde_json::from_str(&json)
        .map_err(|e| format!("Failed to parse CDP cookie response: {}", e))?;

    Ok(response
        .cookies
        .into_iter()
        .map(|c| CookieData {
            name: c.name,
            value: c.value,
            domain: c.domain,
            path: c.path,
            secure: c.secure,
            http_only: c.http_only,
            expires_unix: if c.session { None } else { Some(c.expires) },
        })
        .collect())
}

/// Delete every cookie under the Kwansei Gakuin domain family via CDP.
pub(super) async fn delete_university_cookies(app: &tauri::AppHandle) -> Result<usize, String> {
    let cookies: Vec<CookieData> = extract_all_cookies(app)
        .await?
        .into_iter()
        .filter(|c| super::is_university_cookie(&c.domain))
        .collect();
    let count = cookies.len();

    let win = app
        .get_webview_window("login")
        .or_else(|| app.get_webview_window("kgc-headless"))
        .or_else(|| app.get_webview_window("luna-headless"))
        .or_else(|| app.get_webview_window("kwic-headless"))
        .or_else(|| app.get_webview_window("main"))
        .ok_or("No webview window available for cookie deletion")?;

    win.with_webview(move |webview| unsafe {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;

        let core_webview = webview
            .controller()
            .CoreWebView2()
            .expect("CoreWebView2 must be available");

        for cookie in &cookies {
            let params = serde_json::json!({
                "name": cookie.name,
                "domain": cookie.domain,
                "path": cookie.path,
            });
            let params_str = format!("{}\0", params);
            let method: Vec<u16> = "Network.deleteCookies\0".encode_utf16().collect();
            let params_w: Vec<u16> = params_str.encode_utf16().collect();
            let handler =
                CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_code, _json| Ok(())));

            let _ = core_webview.CallDevToolsProtocolMethod(
                windows_core::PCWSTR(method.as_ptr()),
                windows_core::PCWSTR(params_w.as_ptr()),
                &handler,
            );
        }
    })
    .map_err(|e| format!("with_webview failed: {}", e))?;

    Ok(count)
}

/// Write cookies into the WebView2 cookie store via CDP `Network.setCookie`.
/// Best-effort, fire-and-forget per cookie. Returns the count attempted.
pub(super) async fn set_all_cookies(
    app: &tauri::AppHandle,
    cookies: &[CookieData],
) -> Result<usize, String> {
    let win = app
        .get_webview_window("login")
        .or_else(|| app.get_webview_window("kgc-headless"))
        .or_else(|| app.get_webview_window("luna-headless"))
        .or_else(|| app.get_webview_window("kwic-headless"))
        .or_else(|| app.get_webview_window("main"))
        .ok_or("No webview window available for cookie injection")?;

    let cookies = cookies.to_vec();
    let count = cookies.len();

    win.with_webview(move |webview| unsafe {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;

        let core_webview = webview
            .controller()
            .CoreWebView2()
            .expect("CoreWebView2 must be available");

        for c in &cookies {
            let mut params = serde_json::json!({
                "name": c.name,
                "value": c.value,
                "domain": c.domain.trim_start_matches('.'),
                "path": c.path,
                "secure": c.secure,
                "httpOnly": c.http_only,
            });
            if let Some(exp) = c.expires_unix {
                params["expires"] = serde_json::json!(exp);
            }
            let params_str = format!("{}\0", params);
            let method: Vec<u16> = "Network.setCookie\0".encode_utf16().collect();
            let params_w: Vec<u16> = params_str.encode_utf16().collect();

            let handler =
                CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_code, _json| Ok(())));

            let _ = core_webview.CallDevToolsProtocolMethod(
                windows_core::PCWSTR(method.as_ptr()),
                windows_core::PCWSTR(params_w.as_ptr()),
                &handler,
            );
        }
    })
    .map_err(|e| format!("with_webview failed: {}", e))?;

    Ok(count)
}
