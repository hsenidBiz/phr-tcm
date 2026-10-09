//! The files a run reads before each step - the sign-in recipe (with the
//! environments file it runs in) and the areas file - read once per run,
//! and read again only when one of them changed.
//!
//! Not a data cache: it is held by the run, in memory, and dropped with it.
//! Each step still looks at every file (a stat: its modified time and
//! size), so a person who changes the recipe or the areas between two steps
//! of a supervised run is seen at the next step, as before. A file whose
//! stat fails is read every time.

use super::nav::{self, NavFile};
use super::recipe::{self, SignInRecipe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

/// What a stat said about one file: not there, or there with this modified
/// time and size.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Stamp {
    Absent,
    Present(SystemTime, u64),
}

/// `None` when the stat failed for any other reason than the file not
/// being there: the file is then read.
fn stamp(path: &Path) -> Option<Stamp> {
    match std::fs::metadata(path) {
        Ok(m) => Some(Stamp::Present(m.modified().ok()?, m.len())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Stamp::Absent),
        Err(_) => None,
    }
}

/// A value read from files, with each file's stamp before it was read.
struct Kept<T> {
    stamps: Vec<Stamp>,
    value: T,
}

/// One run's files. `Default` is a run that has read nothing yet.
#[derive(Default)]
pub struct RunFiles {
    recipe: Mutex<Option<Kept<Option<SignInRecipe>>>>,
    nav: Mutex<Option<Kept<NavFile>>>,
    reads: AtomicU32,
}

impl RunFiles {
    /// `recipe::load_effective_recipe_if_any`, read again only when the
    /// recipe or the environments file changed.
    pub fn recipe(&self, root: &Path, org: &str, project: &str) -> Result<Option<SignInRecipe>, String> {
        let paths = [recipe::recipe_path(root, org, project), crate::environments::file_path(root)];
        self.fresh(&self.recipe, &paths, || recipe::load_effective_recipe_if_any(root, org, project))
    }

    /// `nav::load_nav`, read again only when the areas file changed.
    pub fn nav(&self, root: &Path, org: &str, project: &str) -> Result<NavFile, String> {
        let paths = [nav::nav_path(root, org, project)];
        self.fresh(&self.nav, &paths, || nav::load_nav(root, org, project))
    }

    /// How many times a file was actually read, either kind.
    pub fn reads(&self) -> u32 {
        self.reads.load(Ordering::Relaxed)
    }

    /// The kept value when every file's stamp is what it was, else a fresh
    /// read. The stamps are taken before the read, so a file that changes
    /// during it is read again next time. A failed read is never kept.
    fn fresh<T: Clone>(
        &self,
        slot: &Mutex<Option<Kept<T>>>,
        paths: &[PathBuf],
        read: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let stamps: Option<Vec<Stamp>> = paths.iter().map(|p| stamp(p)).collect();
        let mut kept = slot.lock().unwrap_or_else(|e| e.into_inner());
        if let (Some(now), Some(k)) = (&stamps, kept.as_ref()) {
            if &k.stamps == now {
                return Ok(k.value.clone());
            }
        }
        self.reads.fetch_add(1, Ordering::Relaxed);
        let value = read();
        *kept = match (&value, stamps) {
            (Ok(v), Some(stamps)) => Some(Kept { stamps, value: v.clone() }),
            _ => None,
        };
        value
    }
}
