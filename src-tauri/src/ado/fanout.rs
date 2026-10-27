//! The one way the app asks every project of an organization the same
//! question ("treat all projects as one").
//!
//! Azure DevOps has an organization-level route for some reads (a WIQL
//! query, the repositories list) and none for others (the area tree, a
//! project's test plans). For the second kind the app asks the projects one
//! by one - and an organization can hold hundreds of them, so the walk is
//! kept honest in three ways:
//!
//! - **Capped**: at most `MAX_FANOUT_PROJECTS` projects, the first ones by
//!   name, and the screen is told how many it left out (`ProjectCoverage`).
//! - **Bounded**: `FANOUT_CONCURRENCY` projects at a time, never all at once.
//!   Every request still goes through the client's pacer.
//! - **Never a silent partial list**: a project the person may not read (or
//!   whose repositories are not there) is skipped, logged and counted out of
//!   `shown`; anything else - an expired sign-in, a throttle, a server
//!   error - fails the whole call.

use super::{AdoClient, AdoError};
use crate::cache::{self, keys};
use futures::stream::{self, StreamExt};
use serde::Serialize;
use std::future::Future;

/// The most projects a fan-out reads. An organization with more is read for
/// its first projects by name and says so.
pub const MAX_FANOUT_PROJECTS: usize = 100;

/// How many projects are read at the same time.
pub const FANOUT_CONCURRENCY: usize = 4;

/// How many projects a fan-out showed, out of how many the organization has.
/// `shown < total` means the screen must say so.
///
/// Named for projects so it never reads as the spec-coverage feature.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, specta::Type)]
pub struct ProjectCoverage {
    pub shown: u32,
    pub total: u32,
}

/// The projects a fan-out reads: sorted by name (case-insensitively, so the
/// cut is the same on every run), the first `MAX_FANOUT_PROJECTS`, and the
/// full count.
pub fn capped(mut projects: Vec<String>) -> (Vec<String>, u32) {
    let total = projects.len() as u32;
    projects.sort_by_key(|p| p.to_lowercase());
    projects.truncate(MAX_FANOUT_PROJECTS);
    (projects, total)
}

/// Run `f` for each project, `FANOUT_CONCURRENCY` at a time, and return the
/// answers in the order the projects were given.
pub async fn per_project<T, F, Fut>(
    projects: &[String],
    f: F,
) -> Vec<(String, Result<T, AdoError>)>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<T, AdoError>>,
{
    let f = &f;
    stream::iter(projects.iter().cloned())
        .map(|project| async move {
            let result = f(project.clone()).await;
            (project, result)
        })
        .buffered(FANOUT_CONCURRENCY)
        .collect()
        .await
}

/// Fold the per-project answers into what was read and how much of the
/// organization that is. A project the person cannot read is skipped (the
/// log says which, the screen says how many); any other failure fails the
/// whole call.
pub fn gather<T>(
    results: Vec<(String, Result<T, AdoError>)>,
    total: u32,
) -> Result<(Vec<(String, T)>, ProjectCoverage), AdoError> {
    let mut read = Vec::with_capacity(results.len());
    for (project, result) in results {
        match result {
            Ok(value) => read.push((project, value)),
            Err(e @ (AdoError::Forbidden | AdoError::NotFound | AdoError::RepositoryUnavailable)) => {
                crate::applog::warn(format!("all projects: skipped {project}: {e}"));
            }
            Err(e) => return Err(e),
        }
    }
    let shown = read.len() as u32;
    Ok((read, ProjectCoverage { shown, total }))
}

impl AdoClient {
    /// The names of the organization's projects, read once and kept for
    /// `ORG_PROJECTS_TTL` (session tier only). Every fan-out starts here, so
    /// a screen that fans out twice does not list the projects twice.
    pub async fn org_project_names(&self, org: &str) -> Result<Vec<String>, AdoError> {
        let key = keys::org_projects(&self.base_url, org);
        if let Some(hit) = cache::session_fresh::<Vec<String>>(&key, keys::ORG_PROJECTS_TTL) {
            return Ok(hit);
        }
        let names: Vec<String> = self
            .get_projects(org)
            .await?
            .into_iter()
            .map(|p| p.name)
            .collect();
        cache::session_put(&key, names.clone());
        Ok(names)
    }
}
