//! The company's Azure DevOps repo as a Velopack update source.
//!
//! DevOps has no release page. What it has is a git repo whose files can be
//! read over HTTPS with the bearer token this app already holds for
//! everything else it does against DevOps - so the feed and the packages
//! live in `PHR-TCM`, and a user gets updates from there exactly when they
//! can read that repo.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use velopack::bundle::Manifest;
use velopack::sources::UpdateSource;
use velopack::{Error, VelopackAsset, VelopackAssetFeed};

/// The Items API for the releases repo: every request below hangs off this.
pub const ADO_ITEMS_BASE: &str = "https://dev.azure.com/PeoplesHR/HRM/_apis/git/repositories/PHR-TCM";

/// The branch releases are published to. `TCM_UPDATE_BRANCH` overrides it
/// at runtime, which is how a release is rehearsed on a throwaway branch
/// without touching this one - see `AdoSource::new`.
///
/// NOT `main`, and not by preference: `main` carries a branch policy that
/// requires a pull request, so the 1.23.0 release was rejected with
/// TF402455 the first time it tried. A release replaces this branch with a
/// single orphan commit, which a policy-protected branch can never accept
/// and which would read as a total rewrite in a pull request. `main` keeps
/// the README that tells a person landing on the repo how to install.
pub const ADO_BRANCH: &str = "releases";

const API_VERSION: &str = "7.1";

/// Raised when DevOps answered 401 or 403 - this user cannot read the
/// releases repo. Shared with the caller by handle, because the trait's
/// error type is Velopack's and carries no status code: a string match on
/// the message would work until the day the wording changed.
#[derive(Clone, Default, Debug)]
pub struct AccessDenied(Arc<AtomicBool>);

impl AccessDenied {
    pub fn get(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    fn set(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// The releases repo, read through the Items API with the user's own token.
pub struct AdoSource {
    base: String,
    branch: String,
    token: String,
    denied: AccessDenied,
    /// The commit the last feed was read from. Every package is fetched
    /// from THIS commit, not from the branch tip, so a release pushed
    /// between the two requests cannot make them disagree.
    pinned: Mutex<Option<String>>,
    /// Built lazily, on first use rather than in `at()`: `UpdateSource` is
    /// called from inside `spawn_blocking`, but callers may construct an
    /// `AdoSource` from ordinary async code. `reqwest::blocking::Client`
    /// spins up its own background runtime, and building or dropping one
    /// from inside a Tokio runtime's async context panics ("Cannot drop a
    /// runtime in a context where blocking is not allowed") - so this stays
    /// unbuilt until a blocking call site actually needs it.
    http: std::sync::OnceLock<reqwest::blocking::Client>,
}

impl AdoSource {
    /// The real repo. `TCM_UPDATE_BRANCH` in the environment picks another
    /// branch - a rehearsal on a throwaway branch, never on `main`.
    pub fn new(token: String) -> (Self, AccessDenied) {
        let branch = std::env::var("TCM_UPDATE_BRANCH").ok().filter(|b| !b.is_empty());
        Self::at(ADO_ITEMS_BASE, branch.as_deref().unwrap_or(ADO_BRANCH), token)
    }

    /// Any base and branch - the tests point this at a mock server.
    pub fn at(base: &str, branch: &str, token: String) -> (Self, AccessDenied) {
        let denied = AccessDenied::default();
        let src = AdoSource {
            base: base.trim_end_matches('/').to_owned(),
            branch: branch.to_owned(),
            token,
            denied: denied.clone(),
            pinned: Mutex::new(None),
            http: std::sync::OnceLock::new(),
        };
        (src, denied)
    }

    fn http(&self) -> &reqwest::blocking::Client {
        self.http.get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client")
        })
    }

    /// A GET that turns 401/403 into the no-access flag and everything
    /// else non-2xx into an ordinary error.
    fn get_text(&self, url: &str) -> Result<String, Error> {
        let resp = self
            .http()
            .get(url)
            .bearer_auth(&self.token)
            .send()
            .map_err(|e| Error::Other(format!("devops: {e}")))?;
        let status = resp.status();
        if status == 401 || status == 403 {
            self.denied.set();
            // The status code is diagnostic, not something a user can act
            // on - keep it in the log (where `name` from the caller's
            // `applog::warn` already says which source this was) and hand
            // the caller only what a person can actually do about it. This
            // string can end up in a user-facing toast (via `resolve`'s
            // `blocked`), so no "devops:" prefix and no raw status code.
            crate::applog::warn(format!("devops: access denied fetching {url} ({status})"));
            return Err(Error::Other(
                "you don't have access yet - ask for access to PHR-TCM in Azure DevOps".into(),
            ));
        }
        if !status.is_success() {
            return Err(Error::Other(format!("devops: http {status}")));
        }
        resp.text().map_err(|e| Error::Other(format!("devops: {e}")))
    }

    /// The commit `branch` points at right now.
    ///
    /// DevOps' `filter` query param is a prefix match, not an exact one: a
    /// filter of `heads/main` also matches `refs/heads/main-hotfix`, and the
    /// API is free to list that sibling first. So the response is scanned
    /// for the ref whose name equals `refs/heads/<branch>` exactly, never
    /// just the first row - taking the first row would risk pinning to the
    /// wrong branch's tip, which defeats the whole point of this source.
    fn commit_id(&self) -> Result<String, Error> {
        #[derive(serde::Deserialize)]
        struct Refs {
            value: Vec<RefRow>,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RefRow {
            name: String,
            object_id: String,
        }
        let url = format!(
            "{}/refs?filter=heads/{}&api-version={API_VERSION}",
            self.base, self.branch
        );
        let refs: Refs = serde_json::from_str(&self.get_text(&url)?)?;
        let wanted = format!("refs/heads/{}", self.branch);
        refs.value
            .into_iter()
            .find(|r| r.name == wanted)
            .map(|r| r.object_id)
            .ok_or_else(|| Error::Other(format!("devops: branch {} has no commits", self.branch)))
    }

    /// The download URL of one file at one commit.
    fn item_url(&self, file: &str, commit: &str) -> String {
        format!(
            "{}/items?path={}&download=true&versionDescriptor.versionType=commit&versionDescriptor.version={}&api-version={API_VERSION}",
            self.base,
            urlencoding::encode(&format!("/{file}")),
            commit
        )
    }
}

impl UpdateSource for AdoSource {
    fn get_release_feed(&self, channel: &str, _app: &Manifest, _staged_user_id: &str) -> Result<VelopackAssetFeed, Error> {
        let commit = self.commit_id()?;
        let url = self.item_url(&format!("releases.{channel}.json"), &commit);
        let json = self.get_text(&url)?;
        let feed: VelopackAssetFeed = serde_json::from_str(&json)?;
        *self.pinned.lock().unwrap() = Some(commit);
        Ok(feed)
    }

    fn download_release_entry(&self, asset: &VelopackAsset, local_file: &Path, progress: Option<Sender<i16>>) -> Result<(), Error> {
        // The commit the feed came from; without one (no check in this
        // process yet), read the tip now - a moving pointer is still
        // better than nothing, and the feed re-check in `download_and_apply`
        // normally means this branch is never taken.
        let commit = match self.pinned.lock().unwrap().clone() {
            Some(c) => c,
            None => self.commit_id()?,
        };
        let url = self.item_url(&asset.FileName, &commit);
        let auth = format!("Bearer {}", self.token);
        velopack::download::download_url_to_file_with_headers(&url, local_file, &[("Authorization", &auth)], |p| {
            if let Some(tx) = &progress {
                let _ = tx.send(p);
            }
        })
    }
}
