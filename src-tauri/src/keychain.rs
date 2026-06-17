// ---------------------------------------------------------------------------
// Secret store — ONE bundle, fetched in one shot.
//
// Every app secret (AI key, OAuth tokens) and every cookie blob lives in a
// single JSON map stored as a SINGLE keychain item (account = BUNDLE_ACCOUNT)
// with a SINGLE machine-bound encrypted-file fallback. The bundle is read once
// per process into memory and reused, so:
//
//   * "一次全取": one SecItemCopyMatching returns all secrets → at most ONE
//     keychain authorization prompt for the entire app, ever (vs. one prompt
//     per item per access in the old per-key layout).
//   * complete coverage: secrets + cookies (namespaced "cookie.<key>") share
//     the one bundle, so there is no second service that can prompt separately.
//
// Identical in debug and release: both read the keychain bundle (one cached
// read per process → at most one Touch ID prompt per launch, in dev too) and
// fall back to the encrypted file only when the keychain is unavailable.
//
// The machine-bound encrypted file uses AES-256-GCM keyed off a per-machine
// identifier: the ciphertext is useless if copied to another machine and never
// readable in plaintext. A process running as this user can re-derive the key
// from the binary, so that tier is theft/leak resistance, not secrecy against
// local malware — the keychain is for that.
// ---------------------------------------------------------------------------

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

const SERVICE: &str = "com.kgu.selah";
const COOKIE_SERVICE: &str = "com.kgu.selah.cookies";
// Dev and release share one data dir + login keychain, so the bundle slot is
// namespaced by build: a dev (ad-hoc-signed) run never reads, overwrites, or
// hits an ACL conflict with the installed Developer ID build's secrets.
#[cfg(debug_assertions)]
const BUNDLE_ACCOUNT: &str = "secret_bundle_v1_dev";
#[cfg(not(debug_assertions))]
const BUNDLE_ACCOUNT: &str = "secret_bundle_v1";

/// Secrets that lived as their own keychain item / file in earlier versions;
/// folded into the bundle on first launch. Keep complete — anything missing
/// here is silently lost on upgrade.
const LEGACY_SECRET_KEYS: [&str; 4] = [
    "ai_api_key",
    "gcal_client_secret",
    "gcal_token",
    "ms_mail_token",
];
const LEGACY_COOKIE_KEYS: [&str; 4] = [
    "sso_cookie_backup",
    "luna_cookie_jar",
    "kwic_cookie_jar",
    "kgc_cookie_jar",
];

/// In-memory copy of the whole secret bundle. None = not loaded yet.
static BUNDLE: LazyLock<Mutex<Option<HashMap<String, String>>>> = LazyLock::new(|| Mutex::new(None));

/// Set when the keychain bundle existed but could not be read this session
/// (Touch ID cancelled / auth failed / biometric set changed). The in-memory
/// bundle is then empty *but not authoritative*, so persisting it would clobber
/// the real keychain copy — `persist_bundle` refuses to write while this is set.
static BUNDLE_LOAD_FAILED: AtomicBool = AtomicBool::new(false);

/// Outcome of a keychain bundle read, distinguishing "no item yet" (safe to
/// create a fresh bundle) from "item exists but unlock failed" (must not
/// overwrite it).
enum BundleRead {
    Found(String),
    NotFound,
    Failed,
}

fn with_bundle<R>(f: impl FnOnce(&mut HashMap<String, String>) -> R) -> R {
    let mut guard = BUNDLE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(load_bundle_from_store());
    }
    f(guard.as_mut().expect("bundle loaded"))
}

// ---- public API -----------------------------------------------------------

pub fn get_secret(key: &str) -> Option<String> {
    with_bundle(|bundle| bundle.get(key).cloned())
}

pub fn set_secret(key: &str, value: &str) -> Result<(), String> {
    with_bundle(|bundle| {
        bundle.insert(key.to_string(), value.to_string());
        persist_bundle(bundle)
    })
}

pub fn delete_secret(key: &str) {
    with_bundle(|bundle| {
        if bundle.remove(key).is_some() {
            let _ = persist_bundle(bundle);
        }
    });
}

pub fn get_cookie_secret(key: &str) -> Option<String> {
    get_secret(&cookie_ns(key))
}

pub fn set_cookie_secret(key: &str, value: &str) -> Result<(), String> {
    set_secret(&cookie_ns(key), value)
}

pub fn delete_cookie_secret(key: &str) {
    delete_secret(&cookie_ns(key));
}

fn cookie_ns(key: &str) -> String {
    format!("cookie.{key}")
}

// ---- load / persist the whole bundle --------------------------------------

fn load_bundle_from_store() -> HashMap<String, String> {
    // One read covers every secret → at most one Touch ID prompt per process.
    // Same path in debug and release: dev uses the keychain too (the single
    // cached read makes the prompt a once-per-launch cost), falling back to the
    // encrypted file only when the keychain is unavailable.
    match bundle_kc_read() {
        BundleRead::Found(json) => match serde_json::from_str::<HashMap<String, String>>(&json) {
            Ok(map) => return map,
            Err(_) => {
                // Corrupt payload: don't let an empty map overwrite it.
                BUNDLE_LOAD_FAILED.store(true, Ordering::Relaxed);
                return HashMap::new();
            }
        },
        BundleRead::NotFound => {} // no bundle yet → file fallback / migrate
        BundleRead::Failed => {
            BUNDLE_LOAD_FAILED.store(true, Ordering::Relaxed);
            log::warn!(
                "[keychain] secret bundle unlock failed/cancelled — secrets unavailable until restart"
            );
            return HashMap::new();
        }
    }
    if let Some(json) = enc_read(&bundle_enc_path()) {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&json) {
            // Loaded from the file fallback. If the keychain is now usable
            // (e.g. the keychain-access-groups entitlement was added since the
            // file was written), this upgrades the bundle into the Touch ID
            // item and drops the weaker file; a no-op write otherwise.
            let _ = persist_bundle(&map);
            return map;
        }
    }
    // First launch on this layout: fold any legacy per-item secrets in, once.
    let map = migrate_legacy();
    let _ = persist_bundle(&map);
    map
}

fn persist_bundle(map: &HashMap<String, String>) -> Result<(), String> {
    if BUNDLE_LOAD_FAILED.load(Ordering::Relaxed) {
        return Err("secret store locked: biometric unlock failed this session — restart to retry".into());
    }
    let json = serde_json::to_string(map).map_err(|e| format!("serialize bundle: {e}"))?;
    match bundle_kc_write(&json) {
        Ok(()) => {
            // When the biometric keychain item works, drop the machine-bound
            // file copy: keeping it would let an attacker bypass Touch ID by
            // reading the (weaker) file instead. macOS only — the keychain item
            // there is the biometric one; elsewhere the file stays as fallback.
            #[cfg(target_os = "macos")]
            {
                let _ = std::fs::remove_file(bundle_enc_path());
            }
            Ok(())
        }
        Err(err) => {
            log::warn!("[keychain] bundle keychain write failed ({err}); encrypted file fallback in use");
            enc_write(&bundle_enc_path(), &json)
        }
    }
}

/// Force the (possibly Touch-ID-gated) bundle read to happen now, on a
/// background thread, so the prompt appears at a controlled moment (app launch
/// / entering Live) rather than from an arbitrary background task mid-session.
pub fn prewarm() {
    std::thread::spawn(|| {
        let _ = get_secret("__prewarm__");
    });
}

/// Fold pre-bundle secrets (separate keychain items / per-key .enc files) into
/// one map. Read-only on the legacy items: leaving them avoids extra prompts,
/// and nothing reads them once the bundle exists.
fn migrate_legacy() -> HashMap<String, String> {
    let mut map = HashMap::new();

    // Interim per-key encrypted files from a previous iteration.
    for key in LEGACY_SECRET_KEYS {
        if let Some(value) = enc_read(&per_key_enc_path(key)) {
            map.insert(key.to_string(), value);
        }
    }
    for ck in LEGACY_COOKIE_KEYS {
        let ns = cookie_ns(ck);
        if let Some(value) = enc_read(&per_key_enc_path(&ns)) {
            map.insert(ns, value);
        }
    }

    // Legacy per-account OS keychain items.
    for key in LEGACY_SECRET_KEYS {
        if !map.contains_key(key) {
            if let Some(value) = kc_get(SERVICE, key) {
                map.insert(key.to_string(), value);
            }
        }
    }
    for ck in LEGACY_COOKIE_KEYS {
        let ns = cookie_ns(ck);
        if !map.contains_key(&ns) {
            if let Some(value) = kc_get(COOKIE_SERVICE, ck) {
                map.insert(ns, value);
            }
        }
    }

    // Remove the now-folded interim files (local, no prompt).
    for key in LEGACY_SECRET_KEYS {
        let _ = std::fs::remove_file(per_key_enc_path(key));
    }
    for ck in LEGACY_COOKIE_KEYS {
        let _ = std::fs::remove_file(per_key_enc_path(&cookie_ns(ck)));
    }

    if !map.is_empty() {
        log::info!("[keychain] migrated {} legacy secret(s) into the bundle", map.len());
    }
    map
}

// ---- OS keychain (single generic-password item per service+account) -------

#[cfg(target_os = "macos")]
fn kc_get(service: &str, account: &str) -> Option<String> {
    security_framework::passwords::get_generic_password(service, account)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

#[cfg(target_os = "windows")]
fn kc_get(service: &str, account: &str) -> Option<String> {
    keyring::Entry::new(service, account).ok()?.get_password().ok()
}

#[cfg(target_os = "windows")]
fn kc_set(service: &str, account: &str, value: &str) -> Result<(), String> {
    keyring::Entry::new(service, account)
        .map_err(|e| format!("Keychain entry error: {}", e))?
        .set_password(value)
        .map_err(|e| format!("Credential set error: {}", e))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn kc_get(_service: &str, _account: &str) -> Option<String> {
    None
}

// ---- bundle item: biometric-gated on macOS --------------------------------
//
// The bundle is stored as a single keychain item protected by Touch ID
// (BIOMETRY_CURRENT_SET) + AccessibleWhenUnlockedThisDeviceOnly. Reading it
// presents the Touch ID sheet; with the in-memory cache that is one prompt per
// process. If biometric enrollment changes, BIOMETRY_CURRENT_SET invalidates
// the item and the user re-enters their secrets. On Macs without Touch ID the
// write fails and we fall back to the machine-bound encrypted file.

const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_os = "macos")]
fn bundle_kc_read() -> BundleRead {
    use security_framework::passwords::{generic_password, PasswordOptions};
    let mut options = PasswordOptions::new_generic_password(SERVICE, BUNDLE_ACCOUNT);
    // Biometric items live in the data-protection keychain; the read must target
    // it too or the item won't be found. This is what triggers the Touch ID sheet.
    options.use_protected_keychain();
    match generic_password(options) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(json) => BundleRead::Found(json),
            Err(_) => BundleRead::Failed,
        },
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => BundleRead::NotFound,
        Err(e) => {
            log::warn!(
                "[keychain] bundle read failed (code {}): {}",
                e.code(),
                e.message().unwrap_or_default()
            );
            BundleRead::Failed
        }
    }
}

#[cfg(target_os = "macos")]
fn bundle_kc_write(json: &str) -> Result<(), String> {
    use security_framework::access_control::{ProtectionMode, SecAccessControl};
    use security_framework::passwords::{
        delete_generic_password_options, set_generic_password_options, AccessControlOptions,
        PasswordOptions,
    };
    let access = SecAccessControl::create_with_protection(
        Some(ProtectionMode::AccessibleWhenUnlockedThisDeviceOnly),
        AccessControlOptions::BIOMETRY_CURRENT_SET.bits(),
    )
    .map_err(|e| format!("access control create: {e}"))?;

    // Delete the existing item first so this is always an add — updating an
    // existing biometric item would itself trigger a Touch ID prompt on write.
    // Target the data-protection keychain (where the biometric item lives).
    let mut delete = PasswordOptions::new_generic_password(SERVICE, BUNDLE_ACCOUNT);
    delete.use_protected_keychain();
    let _ = delete_generic_password_options(delete);

    let mut options = PasswordOptions::new_generic_password(SERVICE, BUNDLE_ACCOUNT);
    options.use_protected_keychain();
    options.set_access_control(access);
    set_generic_password_options(json.as_bytes(), options)
        .map_err(|e| format!("biometric keychain set: {e}"))
}

#[cfg(target_os = "windows")]
fn bundle_kc_read() -> BundleRead {
    match kc_get(SERVICE, BUNDLE_ACCOUNT) {
        Some(json) => BundleRead::Found(json),
        None => BundleRead::NotFound,
    }
}

#[cfg(target_os = "windows")]
fn bundle_kc_write(json: &str) -> Result<(), String> {
    kc_set(SERVICE, BUNDLE_ACCOUNT, json)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn bundle_kc_read() -> BundleRead {
    BundleRead::NotFound
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn bundle_kc_write(_json: &str) -> Result<(), String> {
    Err("no OS keychain on this platform".into())
}

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

fn bundle_enc_path() -> std::path::PathBuf {
    // Namespaced by build for the same reason as BUNDLE_ACCOUNT — dev and
    // release share the data dir, so the file fallback must not clobber either.
    #[cfg(debug_assertions)]
    let name = "bundle.v1.dev.enc";
    #[cfg(not(debug_assertions))]
    let name = "bundle.v1.enc";
    secrets_dir().join(name)
}

fn per_key_enc_path(key: &str) -> std::path::PathBuf {
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

fn enc_write(path: &std::path::Path, value: &str) -> Result<(), String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use rand::RngCore;

    let cipher =
        Aes256Gcm::new_from_slice(&machine_key()).map_err(|e| format!("cipher init failed: {e}"))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), value.as_bytes())
        .map_err(|e| format!("encrypt failed: {e}"))?;

    // File layout: nonce(12) || ciphertext+tag.
    let mut blob = Vec::with_capacity(12 + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    std::fs::write(path, &blob).map_err(|e| format!("Failed to write secret file: {}", e))?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .map_err(|e| format!("Failed to protect secret file: {}", e))?;
    }
    Ok(())
}

fn enc_read(path: &std::path::Path) -> Option<String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

    let blob = std::fs::read(path).ok()?;
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
