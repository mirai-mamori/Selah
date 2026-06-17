// ---------------------------------------------------------------------------
// App secrets (API keys, OAuth tokens). One strategy for *both* debug and
// release so a value written by a dev build is readable by the packaged build
// and vice versa:
//
//   1. OS keychain  — primary. Properly signed builds bind the ACL to the
//      signing identity (SecItem on macOS, Credential Manager on Windows) and
//      never re-prompt.
//   2. machine-bound encrypted file — fallback when the keychain is
//      unavailable (unsigned/ad-hoc dev builds, sandbox prompt denied, Linux).
//      AES-256-GCM with a key derived from a per-machine identifier, so the
//      ciphertext is useless if copied to another machine and never readable
//      in plaintext. NOTE: a process running as this user can re-derive the
//      key from the binary — this tier is theft/leak resistance, not secrecy
//      against local malware. Use the keychain for that.
// ---------------------------------------------------------------------------

const SERVICE: &str = "com.kgu.selah";

pub fn set_secret(key: &str, value: &str) -> Result<(), String> {
    // Dev builds never touch the OS keychain (avoids prompts and polluting the
    // developer's login keychain); they live entirely in the encrypted file.
    if cfg!(debug_assertions) {
        return encrypted_secret_set(key, value);
    }
    // Release: keychain is primary, but always keep a current encrypted-file
    // copy as a fallback (kept, never deleted, so reads survive a keychain that
    // later becomes unavailable).
    let encrypted = encrypted_secret_set(key, value);
    match keychain_set(key, value) {
        Ok(()) => Ok(()),
        Err(err) => {
            log::warn!("[keychain] '{key}' keychain write failed, encrypted file fallback in use: {err}");
            encrypted
        }
    }
}

pub fn get_secret(key: &str) -> Option<String> {
    if !cfg!(debug_assertions) {
        if let Some(value) = keychain_get(key) {
            return Some(value);
        }
    }
    encrypted_secret_get(key)
}

pub fn delete_secret(key: &str) {
    if !cfg!(debug_assertions) {
        keychain_delete(key);
    }
    let _ = std::fs::remove_file(encrypted_secret_path(key));
}

// ---- OS keychain (primary) -----------------------------------------------

#[cfg(target_os = "macos")]
fn keychain_set(key: &str, value: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(SERVICE, key, value.as_bytes())
        .map_err(|e| format!("Keychain set error: {}", e))
}

#[cfg(target_os = "macos")]
fn keychain_get(key: &str) -> Option<String> {
    security_framework::passwords::get_generic_password(SERVICE, key)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(target_os = "macos")]
fn keychain_delete(key: &str) {
    let _ = security_framework::passwords::delete_generic_password(SERVICE, key);
}

#[cfg(target_os = "windows")]
fn keychain_entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, key).map_err(|e| format!("Keychain entry error: {}", e))
}

#[cfg(target_os = "windows")]
fn keychain_set(key: &str, value: &str) -> Result<(), String> {
    keychain_entry(key)?
        .set_password(value)
        .map_err(|e| format!("Credential set error: {}", e))
}

#[cfg(target_os = "windows")]
fn keychain_get(key: &str) -> Option<String> {
    keychain_entry(key).ok()?.get_password().ok()
}

#[cfg(target_os = "windows")]
fn keychain_delete(key: &str) {
    if let Ok(entry) = keychain_entry(key) {
        let _ = entry.delete_credential();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn keychain_set(_key: &str, _value: &str) -> Result<(), String> {
    Err("no OS keychain on this platform".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn keychain_get(_key: &str) -> Option<String> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn keychain_delete(_key: &str) {}

// ---- machine-bound encrypted file (fallback) ------------------------------

fn secrets_dir() -> std::path::PathBuf {
    let dir = crate::client::data_dir().join("secrets");
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(unix)]
    {
        let _ = std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    dir
}

fn encrypted_secret_path(key: &str) -> std::path::PathBuf {
    secrets_dir().join(format!("{key}.enc"))
}

/// 32-byte AES key derived from a stable per-machine identifier plus a static
/// salt. Not stored anywhere — re-derived on each call.
fn machine_key() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"com.kgu.selah/secret-box/v1");
    hasher.update(machine_entropy());
    hasher.finalize().into()
}

#[cfg(target_os = "macos")]
fn machine_entropy() -> Vec<u8> {
    // gethostuuid(): per-machine UUID, available inside the App Sandbox (no
    // subprocess, unlike `ioreg`). Declared directly to avoid a libc dep.
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    extern "C" {
        fn gethostuuid(id: *mut u8, wait: *const Timespec) -> std::os::raw::c_int;
    }
    let mut uuid = [0u8; 16];
    let wait = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let rc = unsafe { gethostuuid(uuid.as_mut_ptr(), &wait) };
    if rc == 0 {
        uuid.to_vec()
    } else {
        // Fall back to the per-user data dir if the syscall fails.
        crate::client::data_dir()
            .to_string_lossy()
            .into_owned()
            .into_bytes()
    }
}

#[cfg(not(target_os = "macos"))]
fn machine_entropy() -> Vec<u8> {
    // Per-user install path; binds the ciphertext to this account/machine.
    crate::client::data_dir()
        .to_string_lossy()
        .into_owned()
        .into_bytes()
}

fn encrypted_secret_set(key: &str, value: &str) -> Result<(), String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use rand::RngCore;

    let cipher = Aes256Gcm::new_from_slice(&machine_key())
        .map_err(|e| format!("cipher init failed: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), value.as_bytes())
        .map_err(|e| format!("encrypt failed: {e}"))?;

    // File layout: nonce(12) || ciphertext+tag.
    let mut blob = Vec::with_capacity(12 + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    let path = encrypted_secret_path(key);
    std::fs::write(&path, &blob).map_err(|e| format!("Failed to write secret file: {}", e))?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .map_err(|e| format!("Failed to protect secret file: {}", e))?;
    }
    Ok(())
}

fn encrypted_secret_get(key: &str) -> Option<String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

    let blob = std::fs::read(encrypted_secret_path(key)).ok()?;
    if blob.len() < 12 {
        return None;
    }
    let (nonce_bytes, ciphertext) = blob.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(&machine_key()).ok()?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .ok()?;
    String::from_utf8(plaintext).ok()
}

// ---------------------------------------------------------------------------
// Cookie credentials: same strategy as app secrets above — OS keychain in
// release (separate COOKIE_SERVICE), machine-bound encrypted file as fallback.
// Dev builds stay off the keychain so development never triggers a keychain
// authorization prompt; the encrypted file is the shared store both builds
// read. Cookie blobs are namespaced in the file store to avoid colliding with
// secret keys.
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "macos", target_os = "windows"))]
const COOKIE_SERVICE: &str = "com.kgu.selah.cookies";

#[cfg(target_os = "macos")]
fn cookie_keychain_set(key: &str, value: &str) -> Result<(), String> {
    security_framework::passwords::set_generic_password(COOKIE_SERVICE, key, value.as_bytes())
        .map_err(|e| format!("Cookie Keychain set error: {}", e))
}

#[cfg(target_os = "macos")]
fn cookie_keychain_get(key: &str) -> Option<String> {
    security_framework::passwords::get_generic_password(COOKIE_SERVICE, key)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(target_os = "macos")]
fn cookie_keychain_delete(key: &str) {
    let _ = security_framework::passwords::delete_generic_password(COOKIE_SERVICE, key);
}

#[cfg(target_os = "windows")]
fn cookie_entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(COOKIE_SERVICE, key).map_err(|e| format!("Cookie credential error: {}", e))
}

#[cfg(target_os = "windows")]
fn cookie_keychain_set(key: &str, value: &str) -> Result<(), String> {
    cookie_entry(key)?
        .set_password(value)
        .map_err(|e| format!("Cookie credential set error: {}", e))
}

#[cfg(target_os = "windows")]
fn cookie_keychain_get(key: &str) -> Option<String> {
    cookie_entry(key).ok()?.get_password().ok()
}

#[cfg(target_os = "windows")]
fn cookie_keychain_delete(key: &str) {
    if let Ok(entry) = cookie_entry(key) {
        let _ = entry.delete_credential();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn cookie_keychain_set(_key: &str, _value: &str) -> Result<(), String> {
    Err("no OS keychain on this platform".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn cookie_keychain_get(_key: &str) -> Option<String> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn cookie_keychain_delete(_key: &str) {}

fn cookie_secret_ns(key: &str) -> String {
    format!("cookie.{key}")
}

pub fn set_cookie_secret(key: &str, value: &str) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return encrypted_secret_set(&cookie_secret_ns(key), value);
    }
    let encrypted = encrypted_secret_set(&cookie_secret_ns(key), value);
    match cookie_keychain_set(key, value) {
        Ok(()) => Ok(()),
        Err(err) => {
            log::warn!("[keychain] cookie '{key}' keychain write failed, encrypted file fallback in use: {err}");
            encrypted
        }
    }
}

pub fn get_cookie_secret(key: &str) -> Option<String> {
    if !cfg!(debug_assertions) {
        if let Some(value) = cookie_keychain_get(key) {
            return Some(value);
        }
    }
    encrypted_secret_get(&cookie_secret_ns(key))
}

pub fn delete_cookie_secret(key: &str) {
    if !cfg!(debug_assertions) {
        cookie_keychain_delete(key);
    }
    let _ = std::fs::remove_file(encrypted_secret_path(&cookie_secret_ns(key)));
}
