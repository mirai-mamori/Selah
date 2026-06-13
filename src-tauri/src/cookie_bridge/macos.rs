//! macOS: extract cookies from WKWebView's WKHTTPCookieStore via ObjC API.

use std::ptr::NonNull;

use objc2::MainThreadMarker;
use objc2_foundation::{NSArray, NSDictionary, NSHTTPCookie, NSString, NSURL};
use objc2_web_kit::WKWebsiteDataStore;

use super::CookieData;

/// Extract all cookies from the default WKWebsiteDataStore.
/// Dispatches to the main thread since WKWebKit APIs are main-thread-only.
pub(super) async fn extract_all_cookies(app: &tauri::AppHandle) -> Result<Vec<CookieData>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<CookieData>>();
    let tx = std::sync::Mutex::new(Some(tx));

    app.run_on_main_thread(move || {
        // SAFETY: run_on_main_thread guarantees we're on the main thread
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let data_store = unsafe { WKWebsiteDataStore::defaultDataStore(mtm) };
        let http_cookie_store = unsafe { data_store.httpCookieStore() };

        let block = block2::RcBlock::new(move |cookies_ptr: NonNull<NSArray<NSHTTPCookie>>| {
            let cookies = unsafe { cookies_ptr.as_ref() };
            let count = cookies.count();
            let mut result = Vec::with_capacity(count);
            for i in 0..count {
                let c = cookies.objectAtIndex(i);
                let expires_unix = c.expiresDate().map(|d| d.timeIntervalSince1970());
                result.push(CookieData {
                    name: c.name().to_string(),
                    value: c.value().to_string(),
                    domain: c.domain().to_string(),
                    path: c.path().to_string(),
                    secure: c.isSecure(),
                    http_only: c.isHTTPOnly(),
                    expires_unix,
                });
            }
            if let Some(sender) = tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = sender.send(result);
            }
        });

        unsafe { http_cookie_store.getAllCookies(&block) };
    })
    .map_err(|e| format!("Main thread dispatch failed: {}", e))?;

    rx.await
        .map_err(|_| "Cookie extraction failed: channel closed".to_string())
}

/// Delete every cookie under the Kwansei Gakuin domain family.
pub(super) async fn delete_university_cookies(app: &tauri::AppHandle) -> Result<usize, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<usize>();
    let tx = std::sync::Mutex::new(Some(tx));

    app.run_on_main_thread(move || {
        // SAFETY: run_on_main_thread guarantees we're on the main thread.
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let data_store = unsafe { WKWebsiteDataStore::defaultDataStore(mtm) };
        let store = unsafe { data_store.httpCookieStore() };
        let delete_store = store.clone();

        let block = block2::RcBlock::new(move |cookies_ptr: NonNull<NSArray<NSHTTPCookie>>| {
            let cookies = unsafe { cookies_ptr.as_ref() };
            let mut count = 0usize;
            for i in 0..cookies.count() {
                let cookie = cookies.objectAtIndex(i);
                let domain = cookie.domain().to_string();
                if super::is_university_cookie(&domain) {
                    // SAFETY: store is a valid WKHTTPCookieStore on the main thread.
                    unsafe { delete_store.deleteCookie_completionHandler(&cookie, None) };
                    count += 1;
                }
            }
            if let Some(sender) = tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = sender.send(count);
            }
        });

        unsafe { store.getAllCookies(&block) };
    })
    .map_err(|e| format!("Main thread dispatch failed: {}", e))?;

    rx.await
        .map_err(|_| "delete cookies: channel closed".to_string())
}

/// Write cookies into the default WKHTTPCookieStore. Each `CookieData` is turned
/// into a `Set-Cookie` header and parsed back into an `NSHTTPCookie` (avoids
/// hand-building the property dictionary). Returns the number stored.
pub(super) async fn set_all_cookies(
    app: &tauri::AppHandle,
    cookies: &[CookieData],
) -> Result<usize, String> {
    let cookies = cookies.to_vec();
    let (tx, rx) = tokio::sync::oneshot::channel::<usize>();
    let tx = std::sync::Mutex::new(Some(tx));

    app.run_on_main_thread(move || {
        // SAFETY: run_on_main_thread guarantees we're on the main thread
        let mtm = unsafe { MainThreadMarker::new_unchecked() };
        let data_store = unsafe { WKWebsiteDataStore::defaultDataStore(mtm) };
        let store = unsafe { data_store.httpCookieStore() };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let key = NSString::from_str("Set-Cookie");

        let mut count = 0usize;
        for c in &cookies {
            let bare_domain = c.domain.trim_start_matches('.');
            let mut header = format!(
                "{}={}; Domain={}; Path={}",
                c.name, c.value, c.domain, c.path
            );
            if c.secure {
                header.push_str("; Secure");
            }
            if c.http_only {
                header.push_str("; HttpOnly");
            }
            if let Some(exp) = c.expires_unix {
                let max_age = (exp - now).max(0.0) as i64;
                header.push_str(&format!("; Max-Age={}", max_age));
            }
            let header_ns = NSString::from_str(&header);
            let dict = NSDictionary::<NSString, NSString>::from_slices(&[&*key], &[&*header_ns]);
            let url_ns = NSString::from_str(&format!("https://{}/", bare_domain));
            let Some(url) = NSURL::URLWithString(&url_ns) else {
                continue;
            };
            let parsed = NSHTTPCookie::cookiesWithResponseHeaderFields_forURL(&dict, &url);
            for i in 0..parsed.count() {
                let cookie = parsed.objectAtIndex(i);
                // SAFETY: store is a valid WKHTTPCookieStore on the main thread.
                unsafe { store.setCookie_completionHandler(&cookie, None) };
                count += 1;
            }
        }

        if let Some(sender) = tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = sender.send(count);
        }
    })
    .map_err(|e| format!("Main thread dispatch failed: {}", e))?;

    rx.await
        .map_err(|_| "set cookies: channel closed".to_string())
}
