//! Where each database's login lives. Rust owns it: the webview gets a
//! public view (who signs in, whether a password is saved) and never the
//! password or the full connection string. See the design at
//! docs/superpowers/specs/2026-09-26-ui-polish-db-credentials-design.md.
//!
//! A database is named by an id - a shipped preset's, or `own` for the one
//! a person points at themselves - and the id is all that crosses the IPC
//! boundary. What it resolves to is an override saved in Windows Credential
//! Manager when there is one, the shipped string when there is not. The
//! store sits behind `SecretStore` so the rules here are tested against
//! memory, never against the real vault of whoever runs the suite.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::db::guard::normalised_key;
use crate::db::sqlcmd::{hide_password, parse_connection, run_sql, Runner};
use crate::db_defaults::{DbPreset, DB_PRESETS};

pub const OWN_ID: &str = "own";
const OWN_LABEL: &str = "Your own database";
const SEMICOLON: &str = "The login can't contain a semicolon.";

/// A place a saved login can be kept, keyed by its full target name.
/// `get` answers `Ok(None)` for "nothing saved" - only a store that could
/// not be read at all is an error.
pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, String>;
    fn put(&self, id: &str, value: &str) -> Result<(), String>;
    fn remove(&self, id: &str) -> Result<(), String>;
}

/// The store the tests use, and nothing else: it forgets everything when
/// the process ends, which is exactly wrong for a person's login.
#[derive(Default)]
pub struct MemoryStore {
    values: Mutex<HashMap<String, String>>,
}

impl MemoryStore {
    fn values(&self) -> Result<MutexGuard<'_, HashMap<String, String>>, String> {
        self.values.lock().map_err(|_| "The saved logins are unavailable.".to_string())
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        Ok(self.values()?.get(id).cloned())
    }
    fn put(&self, id: &str, value: &str) -> Result<(), String> {
        self.values()?.insert(id.to_string(), value.to_string());
        Ok(())
    }
    fn remove(&self, id: &str) -> Result<(), String> {
        self.values()?.remove(id);
        Ok(())
    }
}

/// One database as the webview sees it. There is deliberately no password
/// field, not even an empty one: a field that exists is a field someone
/// fills in later.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct DbDatabase {
    pub id: String,
    pub label: String,
    pub shipped: bool,
    pub server: String,
    pub port: Option<u16>,
    pub database: String,
    pub user: String,
    pub trust_cert: bool,
    pub has_password: bool,
    /// A saved override exists - for a shipped database, "Reset" has
    /// something to undo.
    pub customised: bool,
}

/// What the edit form sends back. `password: None` (or blank) keeps the
/// password already in force, so the form never needs to be shown it.
#[derive(Clone, serde::Deserialize, specta::Type)]
pub struct DbCredentialsForm {
    pub server: String,
    pub port: Option<u16>,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
    pub trust_cert: bool,
}

/// Hand-written so the password cannot reach a log line, a panic message or
/// a bug report through a stray `{:?}` - same as `sqlcmd::Connection`.
impl std::fmt::Debug for DbCredentialsForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbCredentialsForm")
            .field("server", &self.server)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &"(hidden)")
            .field("trust_cert", &self.trust_cert)
            .finish()
    }
}

fn target(id: &str) -> String {
    format!("tcm-v2/db/{id}")
}

fn shipped(id: &str) -> Option<&'static DbPreset> {
    DB_PRESETS.iter().find(|p| p.id == id)
}

/// Refused up front so a stray id can never write an entry nothing will
/// ever read or clean up.
fn known(id: &str) -> Result<(), String> {
    if id == OWN_ID || shipped(id).is_some() {
        Ok(())
    } else {
        Err("That database is not one the app knows.".into())
    }
}

/// "host,port" -> ("host", Some(port)); a bad port stays part of the host.
fn split_server(server: &str) -> (String, Option<u16>) {
    match server.rsplit_once(',') {
        Some((h, p)) => match p.trim().parse() {
            Ok(n) => (h.trim().into(), Some(n)),
            Err(_) => (server.trim().into(), None),
        },
        None => (server.trim().into(), None),
    }
}

/// The one place a connection string is written. A semicolon anywhere
/// would start a new key - a password of `x;Server=elsewhere` must not be
/// able to move the connection - so it is refused rather than escaped.
fn build(
    server: &str,
    port: Option<u16>,
    database: &str,
    user: &str,
    password: &str,
    trust: bool,
) -> Result<String, String> {
    if [server, database, user, password].iter().any(|v| v.contains(';')) {
        return Err(SEMICOLON.into());
    }
    let mut s = format!("Server={}{}", server.trim(), port.map(|p| format!(",{p}")).unwrap_or_default());
    s.push_str(&format!(";Database={};User Id={};Password={}", database.trim(), user.trim(), password));
    if trust {
        s.push_str(";TrustServerCertificate=True");
    }
    Ok(s)
}

/// The connection string a database id stands for right now: a saved
/// override, else the shipped string, else nothing (`own` never saved).
pub fn resolve(store: &dyn SecretStore, id: &str) -> Result<Option<String>, String> {
    known(id)?;
    if let Some(v) = store.get(&target(id))? {
        return Ok(Some(v));
    }
    Ok(shipped(id).map(|p| p.connection_string.to_string()))
}

/// The connection string a saved form would produce, without saving it.
///
/// A shipped database keeps its server, port, database and certificate
/// setting whatever the form says: what a person may change there is who
/// signs in, not where the app connects.
pub fn apply_form(store: &dyn SecretStore, id: &str, form: &DbCredentialsForm) -> Result<String, String> {
    known(id)?;
    let current = resolve(store, id)?.and_then(|c| parse_connection(&c).ok());
    let typed = form.password.as_deref().filter(|p| !p.is_empty());

    if form.user.trim().is_empty() {
        return Err("Enter the user name.".into());
    }

    if let Some(preset) = shipped(id) {
        let base = parse_connection(preset.connection_string)?;
        let (server, port) = split_server(&base.server);
        // An override that no longer parses still has the shipped
        // password to fall back on.
        let password = match typed {
            Some(p) => p.to_string(),
            None => current.map_or_else(|| base.password.clone(), |c| c.password),
        };
        return build(&server, port, &base.database, &form.user, &password, base.trust_cert);
    }

    if form.server.trim().is_empty() || form.database.trim().is_empty() {
        return Err("Enter the server and database.".into());
    }
    let password = match typed {
        Some(p) => p.to_string(),
        None => current.map(|c| c.password).ok_or_else(|| "Enter a password.".to_string())?,
    };
    build(&form.server, form.port, &form.database, &form.user, &password, form.trust_cert)
}

/// Save a form as this database's login and answer with its new public view.
pub fn save(store: &dyn SecretStore, id: &str, form: &DbCredentialsForm) -> Result<DbDatabase, String> {
    let conn = apply_form(store, id, form)?;
    store.put(&target(id), &conn)?;
    view(store, id)
}

/// Back to the shipped login. `own` has no default, so there is nothing to
/// reset it to - forgetting it is `forget_all`'s job.
pub fn reset(store: &dyn SecretStore, id: &str) -> Result<DbDatabase, String> {
    known(id)?;
    if id == OWN_ID {
        return Err("Only a shipped database has a default to go back to.".into());
    }
    store.remove(&target(id))?;
    view(store, id)
}

/// Remove every saved login. Every entry is attempted even when one fails,
/// so a single stuck entry does not keep the rest on the machine.
pub fn forget_all(store: &dyn SecretStore) -> Result<(), String> {
    let mut first_error = None;
    for id in std::iter::once(OWN_ID).chain(DB_PRESETS.iter().map(|p| p.id)) {
        if let Err(e) = store.remove(&target(id)) {
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Every database the app knows: the shipped ones in `DB_PRESETS` order,
/// then `own`. One that cannot be read is logged and left out, so a broken
/// entry never hides the rest.
pub fn databases(store: &dyn SecretStore) -> Vec<DbDatabase> {
    DB_PRESETS
        .iter()
        .map(|p| p.id)
        .chain(std::iter::once(OWN_ID))
        .filter_map(|id| match view(store, id) {
            Ok(d) => Some(d),
            Err(e) => {
                crate::applog::warn(format!("database logins: could not read {id}: {e}"));
                None
            }
        })
        .collect()
}

fn view(store: &dyn SecretStore, id: &str) -> Result<DbDatabase, String> {
    let preset = shipped(id);
    // A string that does not parse shows empty fields rather than failing
    // the whole list: the person can still see it and save over it.
    let parsed = resolve(store, id)?.and_then(|c| parse_connection(&c).ok());
    let (server, port) = parsed.as_ref().map(|c| split_server(&c.server)).unwrap_or_default();
    Ok(DbDatabase {
        id: id.to_string(),
        label: preset.map_or(OWN_LABEL, |p| p.label).to_string(),
        shipped: preset.is_some(),
        server,
        port,
        database: parsed.as_ref().map(|c| c.database.clone()).unwrap_or_default(),
        user: parsed.as_ref().map(|c| c.user.clone()).unwrap_or_default(),
        trust_cert: parsed.as_ref().is_some_and(|c| c.trust_cert),
        has_password: parsed.as_ref().is_some_and(|c| !c.password.is_empty()),
        customised: store.get(&target(id))?.is_some(),
    })
}

/// A connection string folded so two spellings of the same one compare
/// equal: key order, key case and spacing around the separators ignored.
fn normalise(connection_string: &str) -> String {
    let mut parts: Vec<String> = connection_string
        .split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => format!("{}={}", normalised_key(k), v.trim()),
            None => normalised_key(p),
        })
        .collect();
    parts.sort();
    parts.join(";")
}

/// The id of the shipped database a connection string IS, if any - how a
/// string saved before databases had ids is recognised as a preset rather
/// than copied into the vault as someone's own.
pub fn find_shipped(connection_string: &str) -> Option<&'static str> {
    let wanted = normalise(connection_string);
    DB_PRESETS.iter().find(|p| normalise(p.connection_string) == wanted).map(|p| p.id)
}

/// Sign in with `conn` and run `SELECT 1`, through the same `run_sql` every
/// database tool uses, guard included. The answer is a sentence for the
/// person either way: who it signed in as, or the server's own reason with
/// the password taken out.
pub async fn test_connection_with<R: Runner>(r: &R, exe: &Path, conn: &str) -> Result<String, String> {
    let c = parse_connection(conn)?;
    // `run_sql` already hides the password in everything it returns; this
    // second pass means a failure it learns to build later cannot be the
    // one that forgets.
    run_sql(r, exe, &c, "SELECT 1")
        .await
        .map(|_| format!("Connected to {} on {} as {}.", c.database, c.server, c.user))
        .map_err(|e| hide_password(&e, &c.password))
}

/// The one-time move of a connection string the webview saved before
/// databases had ids. A shipped string, however it was spaced, selects its
/// database and stores nothing, so the shipped login stays the live one.
/// Anything else is the person's own and is kept as they wrote it: it may
/// carry keys the form cannot express. Answers the id to select.
pub fn import_legacy(store: &dyn SecretStore, cs: &str) -> Result<String, String> {
    if let Some(id) = find_shipped(cs) {
        return Ok(id.into());
    }
    // A string nothing could sign in with is refused rather than saved as
    // a login that fails on first use.
    parse_connection(cs)?;
    store.put(&target(OWN_ID), cs.trim())?;
    Ok(OWN_ID.into())
}

/// The store the app runs against, held as Tauri state. Shared rather than
/// owned so the AI bridge's context can carry the same store and resolve a
/// login at the moment a database tool runs.
pub struct DbSecrets(pub Arc<dyn SecretStore>);

/// The real store: Windows Credential Manager, per user, on this machine
/// only. It is what the OS already offers for exactly this, encrypted to
/// the signed-in account, and it survives an uninstall-reinstall.
pub struct CredentialManager;

#[cfg(windows)]
impl SecretStore for CredentialManager {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        match wincred::read(id) {
            Ok(v) => Ok(v.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())),
            Err(code) => {
                crate::applog::warn(format!("Credential Manager read failed (Win32 error {code})"));
                Err("Could not read the saved login.".into())
            }
        }
    }
    fn put(&self, id: &str, value: &str) -> Result<(), String> {
        wincred::write(id, value.as_bytes()).map_err(|code| {
            crate::applog::warn(format!("Credential Manager write failed (Win32 error {code})"));
            "Could not save the login in Windows Credential Manager.".to_string()
        })
    }
    fn remove(&self, id: &str) -> Result<(), String> {
        wincred::delete(id).map_err(|code| {
            crate::applog::warn(format!("Credential Manager delete failed (Win32 error {code})"));
            "Could not remove the saved login.".to_string()
        })
    }
}

#[cfg(not(windows))]
impl SecretStore for CredentialManager {
    fn get(&self, _id: &str) -> Result<Option<String>, String> {
        Err("Windows Credential Manager is not available.".into())
    }
    fn put(&self, _id: &str, _value: &str) -> Result<(), String> {
        Err("Windows Credential Manager is not available.".into())
    }
    fn remove(&self, _id: &str) -> Result<(), String> {
        Err("Windows Credential Manager is not available.".into())
    }
}

/// The Win32 calls, each wrapped small enough that its safety argument
/// fits in one comment. Errors are the raw Win32 code: the caller logs the
/// code and shows a sentence, and neither ever carries the value.
#[cfg(windows)]
mod wincred {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_INVALID_PARAMETER, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    /// Shown as the entry's user name in Credential Manager, so a person
    /// browsing it can tell whose entries these are.
    const USER_NAME: &str = "tcm-v2";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error() -> u32 {
        // SAFETY: reads the calling thread's last-error value; no pointers.
        unsafe { GetLastError() }
    }

    pub(super) fn read(target: &str) -> Result<Option<Vec<u8>>, u32> {
        let name = wide(target);
        let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `name` is NUL-terminated and outlives the call; `cred` is
        // an out-pointer the API fills only when it returns non-zero.
        let ok = unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) };
        if ok == 0 {
            let code = last_error();
            return if code == ERROR_NOT_FOUND { Ok(None) } else { Err(code) };
        }
        // SAFETY: CredReadW returned non-zero, so `cred` points at a
        // CREDENTIALW the API allocated, and nothing has freed it yet.
        Ok(Some(unsafe { copy_blob_and_free(cred) }))
    }

    /// Copies the blob out, then frees the credential.
    ///
    /// # Safety
    /// `cred` must come from a successful CredReadW and not have been freed;
    /// its blob is then CredentialBlobSize readable bytes (or null). The
    /// caller must not use `cred` afterwards.
    unsafe fn copy_blob_and_free(cred: *mut CREDENTIALW) -> Vec<u8> {
        // SAFETY: the contract above; the bytes are copied out before
        // CredFree and `cred` is not touched after it.
        unsafe {
            let c = &*cred;
            let bytes = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
            };
            CredFree(cred as *const _);
            bytes
        }
    }

    pub(super) fn write(target: &str, blob: &[u8]) -> Result<(), u32> {
        let mut name = wide(target);
        let mut user = wide(USER_NAME);
        // A blob too large for a u32 is far past the API's own size limit,
        // so it gets the code the API would have answered with.
        let size = u32::try_from(blob.len()).map_err(|_| ERROR_INVALID_PARAMETER)?;
        // SAFETY: CREDENTIALW is plain data; all-zero is its documented
        // "unset" value for every field set below and every one left alone.
        let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
        cred.Type = CRED_TYPE_GENERIC;
        cred.TargetName = name.as_mut_ptr();
        cred.CredentialBlobSize = size;
        cred.CredentialBlob = blob.as_ptr() as *mut u8;
        cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
        cred.UserName = user.as_mut_ptr();
        // SAFETY: every pointer in `cred` points into `name`, `user` or
        // `blob`, all alive for the call; CredWriteW copies what it keeps
        // and does not write through the blob pointer.
        let ok = unsafe { CredWriteW(&cred, 0) };
        if ok == 0 {
            Err(last_error())
        } else {
            Ok(())
        }
    }

    /// Removing an entry that is not there is success: the caller wanted
    /// it gone, and it is.
    pub(super) fn delete(target: &str) -> Result<(), u32> {
        let name = wide(target);
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let ok = unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            let code = last_error();
            if code != ERROR_NOT_FOUND {
                return Err(code);
            }
        }
        Ok(())
    }
}
