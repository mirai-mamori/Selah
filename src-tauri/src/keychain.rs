// ---------------------------------------------------------------------------
// Debug builds: plain files in data_dir/secrets/ instead of OS keychain
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
fn secrets_dir() -> std::path::PathBuf {
    let dir = crate::client::data_dir().join("secrets");
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(unix)]
    {
        let _ = std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    dir
}

#[cfg(debug_assertions)]
pub fn set_secret(key: &str, value: &str) -> Result<(), String> {
    let path = secrets_dir().join(key);
    std::fs::write(&path, value).map_err(|e| format!("Failed to write secret file: {}", e))?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .map_err(|e| format!("Failed to protect secret file: {}", e))?;
    }
    Ok(())
}

#[cfg(debug_assertions)]
pub fn get_secret(key: &str) -> Option<String> {
    std::fs::read_to_string(secrets_dir().join(key)).ok()
}

#[cfg(debug_assertions)]
pub fn delete_secret(key: &str) {
    let _ = std::fs::remove_file(secrets_dir().join(key));
}

// ---------------------------------------------------------------------------
// Release macOS: SecItem API via security-framework v3
// Unlike the legacy SecKeychainAddGenericPassword (used by keyring crate),
// SecItemAdd/SecItemCopyMatching/SecItemUpdate bind ACL to the code signing
// identity, so a properly signed app won't get repeated keychain prompts.
// ---------------------------------------------------------------------------

#[cfg(all(not(debug_assertions), target_os = "macos"))]
const SERVICE: &str = "com.kgu.selah";

#[cfg(all(not(debug_assertions), target_os = "macos"))]
pub fn set_secret(key: &str, value: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(SERVICE, key, value.as_bytes())
        .map_err(|e| format!("Keychain set error: {}", e))
}

#[cfg(all(not(debug_assertions), target_os = "macos"))]
pub fn get_secret(key: &str) -> Option<String> {
    security_framework::passwords::get_generic_password(SERVICE, key)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(all(not(debug_assertions), target_os = "macos"))]
pub fn delete_secret(key: &str) {
    let _ = security_framework::passwords::delete_generic_password(SERVICE, key);
}

// ---------------------------------------------------------------------------
// Release Windows: keyring crate (Windows Credential Manager)
// ---------------------------------------------------------------------------

#[cfg(all(not(debug_assertions), target_os = "windows"))]
const SERVICE: &str = "com.kgu.selah";

#[cfg(all(not(debug_assertions), target_os = "windows"))]
fn entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, key).map_err(|e| format!("Keychain entry error: {}", e))
}

#[cfg(all(not(debug_assertions), target_os = "windows"))]
pub fn set_secret(key: &str, value: &str) -> Result<(), String> {
    entry(key)?
        .set_password(value)
        .map_err(|e| format!("Credential set error: {}", e))
}

#[cfg(all(not(debug_assertions), target_os = "windows"))]
pub fn get_secret(key: &str) -> Option<String> {
    entry(key).ok()?.get_password().ok()
}

#[cfg(all(not(debug_assertions), target_os = "windows"))]
pub fn delete_secret(key: &str) {
    if let Ok(e) = entry(key) {
        let _ = e.delete_credential();
    }
}

// ---------------------------------------------------------------------------
// Cookie credentials: always use the OS credential store, including debug
// builds. This prevents development runs from moving cookies into debug-only
// plaintext files that release builds cannot read.
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "macos", target_os = "windows"))]
const COOKIE_SERVICE: &str = "com.kgu.selah.cookies";

#[cfg(target_os = "macos")]
fn set_cookie_secret_platform(key: &str, value: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(COOKIE_SERVICE, key, value.as_bytes())
        .map_err(|e| format!("Cookie Keychain set error: {}", e))
}

#[cfg(target_os = "macos")]
fn get_cookie_secret_platform(key: &str) -> Option<String> {
    security_framework::passwords::get_generic_password(COOKIE_SERVICE, key)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(target_os = "macos")]
fn delete_cookie_secret_platform(key: &str) {
    let _ = security_framework::passwords::delete_generic_password(COOKIE_SERVICE, key);
}

#[cfg(target_os = "windows")]
fn cookie_entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(COOKIE_SERVICE, key).map_err(|e| format!("Cookie credential error: {}", e))
}

#[cfg(target_os = "windows")]
fn set_cookie_secret_platform(key: &str, value: &str) -> Result<(), String> {
    cookie_entry(key)?
        .set_password(value)
        .map_err(|e| format!("Cookie credential set error: {}", e))
}

#[cfg(target_os = "windows")]
fn get_cookie_secret_platform(key: &str) -> Option<String> {
    cookie_entry(key).ok()?.get_password().ok()
}

#[cfg(target_os = "windows")]
fn delete_cookie_secret_platform(key: &str) {
    if let Ok(entry) = cookie_entry(key) {
        let _ = entry.delete_credential();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn set_cookie_secret_platform(key: &str, value: &str) -> Result<(), String> {
    set_secret(key, value)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn get_cookie_secret_platform(key: &str) -> Option<String> {
    get_secret(key)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn delete_cookie_secret_platform(key: &str) {
    delete_secret(key);
}

pub fn set_cookie_secret(key: &str, value: &str) -> Result<(), String> {
    set_cookie_secret_platform(key, value)
}

pub fn get_cookie_secret(key: &str) -> Option<String> {
    get_cookie_secret_platform(key)
}

pub fn delete_cookie_secret(key: &str) {
    delete_cookie_secret_platform(key);
}
