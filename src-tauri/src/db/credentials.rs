//! Where each database's login lives. Rust owns it: the webview gets a
//! public view (who signs in, whether a password is saved) and never the
//! password or the full connection string. See the design at
//! docs/superpowers/specs/2026-09-26-ui-polish-db-credentials-design.md.
//!
//! A database is named by an id - a shipped preset's, or one of a person's
//! own (`own`, or `custom-<hex>`; their names live in `catalog`) - and the
//! id is all that crosses the IPC boundary. What it resolves to is an override saved in Windows Credential
//! Manager when there is one, the shipped string when there is not. The
//! store sits behind `SecretStore` so the rules here are tested against
//! memory, never against the real vault of whoever runs the suite.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::db::catalog::{self, Catalog, CustomDb};
use crate::db::guard::normalised_key;
use crate::db::sqlcmd::{hide_password, parse_connection, run_sql, Runner};
use crate::db_defaults::{DbPreset, DB_PRESETS};

pub use crate::db::catalog::OWN_ID;
const NOT_KNOWN: &str = "That database is not one the app knows.";
const SEMICOLON: &str = "The login can't contain a semicolon.";
const EDGE_SPACE: &str = "The password can't start or end with a space.";
const PORT_TWICE: &str = "Put the port in the Port box, not after the server name.";
const NEW_PLACE: &str = "Enter the password for the new server or database.";

/// A place a saved login can be kept, keyed by its full target name.
/// `get` answers `Ok(None)` for "nothing saved" - only a store that could
/// not be read at all is an error.
pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, String>;
    fn put(&self, id: &str, value: &str) -> Result<(), String>;
    fn remove(&self, id: &str) -> Result<(), String>;
    /// The file naming a person's own databases (`catalog`) - ids and
    /// labels, no secret. `None`, the default, reads as the starting list
    /// (`own` alone) and refuses every change to it.
    fn list_file(&self) -> Option<&Path> {
        None
    }
}

/// A store together with the file that names its own databases: the app's
/// Credential Manager and `<app data>/databases.json`, or in the tests a
/// `MemoryStore` and a temp file. Every secret call goes straight through.
pub struct WithList<S> {
    pub store: S,
    pub list: PathBuf,
}

impl<S: SecretStore> SecretStore for WithList<S> {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        self.store.get(id)
    }
    fn put(&self, id: &str, value: &str) -> Result<(), String> {
        self.store.put(id, value)
    }
    fn remove(&self, id: &str) -> Result<(), String> {
        self.store.remove(id)
    }
    fn list_file(&self) -> Option<&Path> {
        Some(&self.list)
    }
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

/// Whether `id` names a database this build knows: a shipped one, or one
/// in the store's list. An id saved by an older release can name a preset
/// since removed, and one saved before a removal a database since removed;
/// a caller that reads that as "nothing chosen" asks here first.
pub fn is_known(store: &dyn SecretStore, id: &str) -> bool {
    if shipped(id).is_some() {
        return true;
    }
    match catalog::read(store.list_file()) {
        Ok(c) => c.find(id).is_some(),
        Err(e) => {
            crate::applog::warn(format!("database logins: {e}"));
            false
        }
    }
}

/// Refused up front so a stray id can never write an entry nothing will
/// ever read or clean up. Answers the database's label.
fn known(store: &dyn SecretStore, id: &str) -> Result<String, String> {
    if let Some(p) = shipped(id) {
        return Ok(p.label.to_string());
    }
    catalog::read(store.list_file())?
        .find(id)
        .map(|d| d.label.clone())
        .ok_or_else(|| NOT_KNOWN.to_string())
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
///
/// A password with a space at either end is refused too: every value is
/// trimmed when the string is read back, so it could never sign in. And a
/// port both after the server name and in the Port box would be written
/// twice into one `Server=` value, which nothing can connect to.
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
    if password != password.trim() {
        return Err(EDGE_SPACE.into());
    }
    if port.is_some() && server.contains(',') {
        return Err(PORT_TWICE.into());
    }
    let mut s = format!("Server={}{}", server.trim(), port.map(|p| format!(",{p}")).unwrap_or_default());
    s.push_str(&format!(";Database={};User Id={};Password={}", database.trim(), user.trim(), password));
    if trust {
        s.push_str(";TrustServerCertificate=True");
    }
    Ok(s)
}

/// The connection string a database id stands for right now: a saved
/// override, else the shipped string, else nothing (an own database with
/// no login saved).
pub fn resolve(store: &dyn SecretStore, id: &str) -> Result<Option<String>, String> {
    known(store, id)?;
    saved_or_shipped(store, id)
}

/// `resolve` for an id already known to be known.
fn saved_or_shipped(store: &dyn SecretStore, id: &str) -> Result<Option<String>, String> {
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
    known(store, id)?;
    let current = saved_or_shipped(store, id)?.and_then(|c| parse_connection(&c).ok());
    form_to_string(shipped(id), current, form)
}

/// The connection string a form for a database not added yet would
/// produce - what Add database's Test connection signs in with. There is
/// no saved password to fall back on, so the form must carry one.
pub fn apply_new_form(form: &DbCredentialsForm) -> Result<String, String> {
    form_to_string(None, None, form)
}

fn form_to_string(
    preset: Option<&DbPreset>,
    current: Option<crate::db::Connection>,
    form: &DbCredentialsForm,
) -> Result<String, String> {
    let typed = form.password.as_deref().filter(|p| !p.is_empty());

    if form.user.trim().is_empty() {
        return Err("Enter the user name.".into());
    }

    if let Some(preset) = preset {
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
        None => {
            let saved = current.ok_or_else(|| "Enter a password.".to_string())?;
            // "Keep the saved password" is only safe for the place it was
            // saved for: carried to another server it would be handed to a
            // machine it was never meant for.
            if !same_place(&saved, form) {
                return Err(NEW_PLACE.into());
            }
            saved.password
        }
    };
    build(&form.server, form.port, &form.database, &form.user, &password, form.trust_cert)
}

/// Whether a form still points where a saved login does: same server,
/// port and database. Server and database names compare as SQL Server
/// compares them by default, ignoring case; a port written after the server
/// name counts the same as one in the Port box.
fn same_place(saved: &crate::db::Connection, form: &DbCredentialsForm) -> bool {
    let (saved_server, saved_port) = split_server(&saved.server);
    let (server, port_in_name) = split_server(&form.server);
    server.eq_ignore_ascii_case(&saved_server)
        && form.port.or(port_in_name) == saved_port
        && form.database.trim().eq_ignore_ascii_case(saved.database.trim())
}

/// Save a form as this database's login and answer with its new public view.
///
/// A form that comes out as the shipped login itself removes the override
/// instead of storing a copy: a copy would keep today's shipped password
/// after a release rotates it, and mark the database as changed when it is
/// not.
pub fn save(store: &dyn SecretStore, id: &str, form: &DbCredentialsForm) -> Result<DbDatabase, String> {
    // Held so a removal cannot take the database off the list between the
    // check and the write - which would leave a login no list names.
    let _held = catalog::lock();
    let conn = apply_form(store, id, form)?;
    if find_shipped(&conn) == Some(id) {
        store.remove(&target(id))?;
    } else {
        store.put(&target(id), &conn)?;
    }
    view(store, id)
}

/// Back to the shipped login. A person's own database has no default, so
/// there is nothing to reset it to - removing it is `remove_custom`'s job.
pub fn reset(store: &dyn SecretStore, id: &str) -> Result<DbDatabase, String> {
    known(store, id)?;
    if shipped(id).is_none() {
        return Err("Only a shipped database has a default to go back to.".into());
    }
    store.remove(&target(id))?;
    view(store, id)
}

/// Add one of a person's own databases: a fresh id, its login saved under
/// it, and its name appended to the list. A login that cannot be saved
/// adds nothing; a list that cannot be saved takes the login back out, so
/// a failure never leaves a secret behind that no list names.
pub fn add_custom(store: &dyn SecretStore, label: &str, form: &DbCredentialsForm) -> Result<DbDatabase, String> {
    let file = store.list_file().ok_or_else(|| catalog::NO_LIST.to_string())?;
    let _held = catalog::lock();
    let mut list = catalog::read(Some(file))?;
    let label = catalog::check_label(label, taken_labels(&list, None))?;
    let conn = apply_new_form(form)?;
    let id = catalog::new_id(&label, &list);
    store.put(&target(&id), &conn)?;
    list.databases.push(CustomDb { id: id.clone(), label: label.clone() });
    if let Err(e) = catalog::write(Some(file), &list) {
        if let Err(back) = store.remove(&target(&id)) {
            crate::applog::warn(format!("database logins: the login of a database not added stayed behind: {back}"));
        }
        return Err(e);
    }
    crate::applog::info("database logins: added one of the person's own databases");
    view_as(store, &id, &label)
}

/// Give one of a person's own databases a new name. A shipped one keeps
/// the name it ships with.
pub fn rename_custom(store: &dyn SecretStore, id: &str, label: &str) -> Result<DbDatabase, String> {
    if shipped(id).is_some() {
        return Err("A shipped database keeps its name.".into());
    }
    let _held = catalog::lock();
    let mut list = catalog::read(store.list_file())?;
    if list.find(id).is_none() {
        return Err(NOT_KNOWN.into());
    }
    let label = catalog::check_label(label, taken_labels(&list, Some(id)))?;
    if let Some(d) = list.databases.iter_mut().find(|d| d.id == id) {
        d.label = label.clone();
    }
    catalog::write(store.list_file(), &list)?;
    view_as(store, id, &label)
}

/// Remove one of a person's own databases: its login out of the store
/// first, then its name off the list and its id retired for good. A login
/// that will not go stops the removal, so a secret is never left behind
/// with no list naming it. Whether an environment still uses the database
/// is the command's question (`commands::ai_tools::remove_custom_with`).
pub fn remove_custom(store: &dyn SecretStore, id: &str) -> Result<(), String> {
    if shipped(id).is_some() {
        return Err("A shipped database can't be removed.".into());
    }
    let _held = catalog::lock();
    let mut list = catalog::read(store.list_file())?;
    if list.find(id).is_none() {
        return Err(NOT_KNOWN.into());
    }
    store.remove(&target(id))?;
    list.databases.retain(|d| d.id != id);
    if id != OWN_ID && !list.retired.iter().any(|r| r == id) {
        list.retired.push(id.to_string());
    }
    catalog::write(store.list_file(), &list)?;
    crate::applog::info("database logins: removed one of the person's own databases");
    Ok(())
}

/// Every name a label must differ from: the shipped databases' and the
/// list's, except the one being renamed.
fn taken_labels<'a>(list: &'a Catalog, except: Option<&'a str>) -> impl Iterator<Item = &'a str> {
    DB_PRESETS.iter().map(|p| p.label).chain(
        list.databases.iter().filter(move |d| Some(d.id.as_str()) != except).map(|d| d.label.as_str()),
    )
}

/// Remove every saved login and every own database. Every entry is
/// attempted even when one fails, so a single stuck entry does not keep
/// the rest on the machine. The list goes back to how it starts - `own`
/// alone, nothing saved for it - and every id it held is retired.
///
/// A list that cannot be read is left exactly as it is: which logins it
/// names is the one thing that would have said what else to remove.
pub fn forget_all(store: &dyn SecretStore) -> Result<(), String> {
    let _held = catalog::lock();
    let mut first_error = None;
    let list = catalog::read(store.list_file());
    let own: Vec<String> = match &list {
        Ok(c) => c.databases.iter().map(|d| d.id.clone()).filter(|id| id != OWN_ID).collect(),
        Err(e) => {
            first_error = Some(e.clone());
            vec![]
        }
    };
    let ids = std::iter::once(OWN_ID.to_string())
        .chain(DB_PRESETS.iter().map(|p| p.id.to_string()))
        .chain(own.iter().cloned());
    for id in ids {
        if let Err(e) = store.remove(&target(&id)) {
            first_error.get_or_insert(e);
        }
    }
    if let (Ok(old), Some(file)) = (list, store.list_file()) {
        let mut fresh = Catalog::starting();
        fresh.retired = old.retired;
        for id in own {
            if !fresh.retired.contains(&id) {
                fresh.retired.push(id);
            }
        }
        if let Err(e) = catalog::write(Some(file), &fresh) {
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// Every database the app knows: the shipped ones in `DB_PRESETS` order,
/// then the person's own in the list's order. One that cannot be read is
/// logged and left out, so a broken entry never hides the rest - and a
/// list that cannot be read still leaves the shipped ones showing.
pub fn databases(store: &dyn SecretStore) -> Vec<DbDatabase> {
    let own = match catalog::read(store.list_file()) {
        Ok(c) => c.databases,
        Err(e) => {
            crate::applog::warn(format!("database logins: {e}"));
            vec![]
        }
    };
    DB_PRESETS
        .iter()
        .map(|p| (p.id.to_string(), p.label.to_string()))
        .chain(own.into_iter().map(|d| (d.id, d.label)))
        .filter_map(|(id, label)| match view_as(store, &id, &label) {
            Ok(d) => Some(d),
            Err(e) => {
                crate::applog::warn(format!("database logins: could not read {id}: {e}"));
                None
            }
        })
        .collect()
}

fn view(store: &dyn SecretStore, id: &str) -> Result<DbDatabase, String> {
    let label = known(store, id)?;
    view_as(store, id, &label)
}

fn view_as(store: &dyn SecretStore, id: &str, label: &str) -> Result<DbDatabase, String> {
    let preset = shipped(id);
    // A string that does not parse shows empty fields rather than failing
    // the whole list: the person can still see it and save over it.
    let parsed = saved_or_shipped(store, id)?.and_then(|c| parse_connection(&c).ok());
    let (server, port) = parsed.as_ref().map(|c| split_server(&c.server)).unwrap_or_default();
    Ok(DbDatabase {
        id: id.to_string(),
        label: label.to_string(),
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
    let _held = catalog::lock();
    let mut list = catalog::read(store.list_file())?;
    // `own` was removed from the list since: it comes back to hold the
    // import, under a name nothing else has.
    if list.find(OWN_ID).is_none() {
        let mut label = catalog::OWN_LABEL.to_string();
        let mut n = 2;
        while catalog::check_label(&label, taken_labels(&list, None)).is_err() {
            label = format!("{} {n}", catalog::OWN_LABEL);
            n += 1;
        }
        list.databases.insert(0, CustomDb { id: OWN_ID.into(), label });
        catalog::write(store.list_file(), &list)?;
    }
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
