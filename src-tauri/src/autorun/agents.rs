//! Who is calling the bridge: the assistants, by label.
//!
//! The MCP proxy names its caller on every bridge request with up to four
//! `x-tcm-agent*` headers: an id made when the proxy starts, its process id,
//! its client's name, and the calling subagent's `agent` name. This module
//! turns them into a `Caller` and keeps one entry per agent so the rest of
//! the app can say "Assistant 2" instead of a hex id.
//!
//! A request with no valid id (an older proxy, a test, the person) is the
//! default caller. It is never registered and never refused for being one.
//!
//! The registry is coordination state, not a cache: it says who is connected
//! right now, so nothing here may be stored on disk or kept across a restart,
//! and nothing else in the app may keep a second copy of it (the same rule as
//! `IMPORTING` in `commands/autorun.rs`). It is one `static` mutex that is
//! never held across an await.
//!
//! An id with several names is several agents that share one pid: a proxy
//! serving a session and the named subagents it spawned. Each is its own
//! entry with its own label.

use std::sync::Mutex;

/// A gone entry is dropped from the registry once it has been gone this long.
pub const FORGET_AFTER_MS: u64 = 10 * 60 * 1000;
/// How long a gone agent is given to come back before its resources are
/// released (`Sweep::past_grace`).
pub const GONE_GRACE_MS: u64 = 60 * 1000;
/// An entry nothing can ask about (no pid, or a named subagent) is quiet
/// once it has made no call for this long (`Sweep::quiet`). Longer than the
/// extra browsers' idle release (30 minutes), so that release always comes
/// first and says why.
pub const QUIET_AFTER_MS: u64 = 3_600_000;

/// How often the app asks whether each agent's process still runs.
pub const SWEEP_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

/// The longest name or client text kept.
const MAX_TEXT: usize = 32;

/// One agent: a proxy's id and, for a named subagent, its name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentKey {
    pub id: String,
    pub name: Option<String>,
}

/// Who a bridge request comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    Default,
    Agent(AgentKey),
}

impl Caller {
    /// The agent making the call; none for the default caller (an older
    /// proxy, a test, the person), which never holds a browser.
    pub fn key(&self) -> Option<&AgentKey> {
        match self {
            Caller::Default => None,
            Caller::Agent(key) => Some(key),
        }
    }
}

/// The four `x-tcm-agent*` headers as read, each None when absent or
/// unreadable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentHeaders {
    pub id: Option<String>,
    pub pid: Option<u32>,
    pub client: Option<String>,
    pub name: Option<String>,
}

impl AgentHeaders {
    /// An agent only for an id that passes `valid_id`; its name only when
    /// `clean_name` keeps it. Never an error: anything else is the default
    /// caller.
    pub fn caller(&self) -> Caller {
        match self.id.as_deref() {
            Some(id) if valid_id(id) => Caller::Agent(AgentKey {
                id: id.to_string(),
                name: self.name.as_deref().and_then(clean_name),
            }),
            _ => Caller::Default,
        }
    }
}

/// Exactly 32 lower-case hex digits.
pub fn valid_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn text_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-')
}

/// A subagent's name, trimmed: one to 32 characters, each ASCII alphanumeric
/// or one of space, dot, underscore, hyphen. Anything else is dropped whole.
pub fn clean_name(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > MAX_TEXT || !t.chars().all(text_char) {
        return None;
    }
    Some(t.to_string())
}

/// A client's name: only the characters a name may hold are kept, cut to 32
/// and trimmed; nothing left is None.
pub fn clean_client(s: &str) -> Option<String> {
    let kept: String = s.chars().filter(|c| text_char(*c)).take(MAX_TEXT).collect();
    let t = kept.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// What the app knows about one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    pub key: AgentKey,
    pub number: u32,
    pub label: String,
    pub pid: Option<u32>,
    pub client: Option<String>,
    pub first_ms: u64,
    pub last_ms: u64,
    pub gone_since_ms: Option<u64>,
}

struct Entry {
    info: AgentInfo,
    /// The sweep has already reported this entry as past its grace.
    grace_reported: bool,
}

static REGISTRY: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

fn registry() -> std::sync::MutexGuard<'static, Vec<Entry>> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

fn label_for(number: u32) -> String {
    format!("Assistant {number}")
}

/// The smallest number from 1 up that no live entry (other than `except`)
/// holds. A gone entry's number is free to take.
fn free_number(entries: &[Entry], except: Option<usize>) -> u32 {
    let mut n = 1;
    while entries
        .iter()
        .enumerate()
        .any(|(i, e)| Some(i) != except && e.info.gone_since_ms.is_none() && e.info.number == n)
    {
        n += 1;
    }
    n
}

/// The log line for an agent whose process has ended: its label only, never
/// its id or pid.
pub fn ended_line(label: &str) -> String {
    format!("AI bridge: {label}'s session ended")
}

/// Record a call. The default caller is never kept (None). An agent's entry
/// is made at its first call, with the smallest number no live entry holds
/// and the label "Assistant {n}"; later calls update its last-call time, pid
/// and client, and make a gone entry live again. Returns the label.
pub fn note_call(caller: &Caller, headers: &AgentHeaders, now_ms: u64) -> Option<String> {
    let Caller::Agent(key) = caller else {
        return None;
    };
    let client = headers.client.as_deref().and_then(clean_client);
    let mut entries = registry();
    match entries.iter().position(|e| &e.info.key == key) {
        Some(i) => {
            if entries[i].info.gone_since_ms.is_some() {
                // Back from gone: keep the label unless a live agent took
                // its number meanwhile.
                if entries
                    .iter()
                    .enumerate()
                    .any(|(j, e)| j != i && e.info.gone_since_ms.is_none() && e.info.number == entries[i].info.number)
                {
                    let n = free_number(&entries, Some(i));
                    entries[i].info.number = n;
                    entries[i].info.label = label_for(n);
                }
                entries[i].info.gone_since_ms = None;
                entries[i].grace_reported = false;
            }
            let e = &mut entries[i];
            e.info.last_ms = now_ms;
            if headers.pid.is_some() {
                e.info.pid = headers.pid;
            }
            if client.is_some() {
                e.info.client = client;
            }
            Some(e.info.label.clone())
        }
        None => {
            let number = free_number(&entries, None);
            let label = label_for(number);
            entries.push(Entry {
                info: AgentInfo {
                    key: key.clone(),
                    number,
                    label: label.clone(),
                    pid: headers.pid,
                    client,
                    first_ms: now_ms,
                    last_ms: now_ms,
                    gone_since_ms: None,
                },
                grace_reported: false,
            });
            Some(label)
        }
    }
}

/// The entry for one agent, live or gone.
pub fn info(key: &AgentKey) -> Option<AgentInfo> {
    registry().iter().find(|e| &e.info.key == key).map(|e| e.info.clone())
}

/// Entries not gone, by number.
pub fn live() -> Vec<AgentInfo> {
    let mut v: Vec<AgentInfo> = registry()
        .iter()
        .filter(|e| e.info.gone_since_ms.is_none())
        .map(|e| e.info.clone())
        .collect();
    v.sort_by_key(|i| i.number);
    v
}

pub fn is_live(key: &AgentKey) -> bool {
    registry()
        .iter()
        .any(|e| &e.info.key == key && e.info.gone_since_ms.is_none())
}

/// The proxy with this id said goodbye: every entry with the id (the session
/// and its named subagents) is gone from now, unless it already was.
pub fn note_bye(id: &str, now_ms: u64) {
    for e in registry().iter_mut().filter(|e| e.info.key.id == id) {
        if e.info.gone_since_ms.is_none() {
            e.info.gone_since_ms = Some(now_ms);
            e.grace_reported = false;
        }
    }
}

/// What one sweep found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sweep {
    /// Became gone in this sweep (their process has ended).
    pub newly_gone: Vec<AgentKey>,
    /// Reached the end of the grace in this sweep; reported once per time gone.
    pub past_grace: Vec<AgentKey>,
    /// Removed from the registry in this sweep.
    pub forgotten: Vec<AgentKey>,
    /// Made no call for `QUIET_AFTER_MS` and nothing can say whether they
    /// still run (no pid, or a named subagent): no longer listed. Their
    /// session was not seen to end, so these are never in `past_grace`.
    pub quiet: Vec<AgentKey>,
}

/// Entries with a pid that `alive` says has ended become gone (`newly_gone`).
/// Entries without a pid, and named subagents, become gone (reported in
/// `quiet`, not in `newly_gone` or `past_grace`) once they have made no call
/// for `QUIET_AFTER_MS`.
/// Gone entries reach `past_grace` once, `GONE_GRACE_MS` after they went.
/// Entries gone `FORGET_AFTER_MS` are removed (`forgotten`), but only in a
/// sweep after the one that reported them in `past_grace`, so the label can
/// still be read with `info` while the agent's resources are released.
///
/// `alive` may be slow and is never called with the registry locked.
pub fn sweep(now_ms: u64, alive: &dyn Fn(u32) -> bool) -> Sweep {
    // Ask about the processes first, without the lock.
    let to_check: Vec<(AgentKey, u32)> = registry()
        .iter()
        .filter(|e| e.info.gone_since_ms.is_none())
        .filter_map(|e| e.info.pid.map(|p| (e.info.key.clone(), p)))
        .collect();
    let dead: Vec<(AgentKey, u32)> = to_check.into_iter().filter(|(_, pid)| !alive(*pid)).collect();

    let mut out = Sweep::default();
    let mut entries = registry();
    for (key, pid) in dead {
        // Skip an entry that changed meanwhile: made gone already, or taken
        // over by a proxy with another pid.
        if let Some(e) = entries.iter_mut().find(|e| e.info.key == key) {
            if e.info.gone_since_ms.is_none() && e.info.pid == Some(pid) {
                e.info.gone_since_ms = Some(now_ms);
                e.grace_reported = false;
                out.newly_gone.push(key);
            }
        }
    }
    // An entry nothing can ask about: no pid, or a named subagent (it shares
    // its proxy's pid for as long as the proxy lives). Quiet for
    // `QUIET_AFTER_MS` since its last call, it is no longer listed and is
    // forgotten later like any gone entry. No session was seen to end, so it
    // is reported in `quiet` and never in `newly_gone` or `past_grace`
    // (`grace_reported` is set at once): nothing is released for it. A call
    // brings it back.
    for e in entries.iter_mut() {
        let unaskable = e.info.pid.is_none() || e.info.key.name.is_some();
        if e.info.gone_since_ms.is_none() && unaskable && now_ms.saturating_sub(e.info.last_ms) >= QUIET_AFTER_MS {
            e.info.gone_since_ms = Some(now_ms);
            e.grace_reported = true;
            out.quiet.push(e.info.key.clone());
        }
    }
    let mut forget: Vec<AgentKey> = Vec::new();
    for e in entries.iter_mut() {
        let Some(since) = e.info.gone_since_ms else { continue };
        let age = now_ms.saturating_sub(since);
        if e.grace_reported {
            // Reported by an earlier sweep.
            if age >= FORGET_AFTER_MS {
                forget.push(e.info.key.clone());
            }
        } else if age >= GONE_GRACE_MS {
            e.grace_reported = true;
            out.past_grace.push(e.info.key.clone());
        }
    }
    entries.retain(|e| !forget.contains(&e.info.key));
    out.forgotten = forget;
    out
}

/// Every entry (tests).
pub fn clear() {
    registry().clear();
}
