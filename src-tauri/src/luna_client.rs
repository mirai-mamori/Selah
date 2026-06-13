use reqwest::Client;
use std::sync::Arc;

use crate::client::{
    build_http_client, data_dir, load_cookie_jar, new_cookie_client, save_cookie_jar,
};

const LUNA_COOKIES_FILE: &str = "luna_cookies.json";

/// Check if Luna response body indicates session expired
pub(crate) fn is_luna_session_expired(body: &str) -> bool {
    // Redirected to login page
    if body.contains("linkCommonLogin") && body.contains("class=\"login-body\"") {
        return true;
    }
    // SAML redirect
    if body.contains("sso.kwansei.ac.jp") && body.contains("SAMLRequest") {
        return true;
    }
    false
}

pub const LUNA_SESSION_EXPIRED_MSG: &str = "Lunaセッションが期限切れです。再ログインしてください。";
pub const LUNA_AUTH_REQUIRED_MSG: &str = "Lunaにログインしてください";

/// HTTP client for Luna LMS
pub struct LunaClient {
    pub http: Client,
    pub cookie_store: Arc<reqwest_cookie_store::CookieStoreMutex>,
    pub authenticated: bool,
}

impl LunaClient {
    pub fn new() -> Self {
        let (cookie_store, http) = new_cookie_client();
        Self {
            http,
            cookie_store,
            authenticated: false,
        }
    }

    /// Save Luna cookies to disk
    pub fn save_session(&self) {
        if !self.authenticated {
            log::warn!("Luna save_session skipped: not authenticated");
            return;
        }
        save_cookie_jar(&self.cookie_store, LUNA_COOKIES_FILE);
        log::info!("Luna cookies saved");
    }

    /// Try to restore Luna session from disk.
    /// Returns true if cookies were loaded (session still needs server validation).
    pub fn try_restore_session(&mut self) -> bool {
        match load_cookie_jar(LUNA_COOKIES_FILE) {
            Some(store) => {
                let cookie_store = Arc::new(reqwest_cookie_store::CookieStoreMutex::new(store));
                self.http = build_http_client(cookie_store.clone());
                self.cookie_store = cookie_store;
                self.authenticated = true;
                log::info!("Luna session restored from disk");
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        self.authenticated = false;
        if let Err(e) = std::fs::remove_file(data_dir().join(LUNA_COOKIES_FILE)) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("Luna clear: failed to delete cookies file: {}", e);
            }
        }
        let (cookie_store, http) = new_cookie_client();
        self.http = http;
        self.cookie_store = cookie_store;
    }
}
