//! The route the Boards "Add Test" button takes, and the two ids its URL
//! needs.
//!
//! This is an INTERNAL portal controller (`_api`, not `_apis`) -
//! undocumented, versioned by `__v`, watched on 2026-09-22 (see
//! docs/superpowers/specs/2026-09-22-requirement-suite-boards-fallback-design.md).
//! It exists here for one reason: creating a test suite through the
//! documented API needs the Basic + Test Plans access level, and an
//! account without it gets a 403 on every plan in the project - while the
//! same account, pressing the same button in Boards, creates the suite
//! fine. So this is the fallback, used ONLY after the documented create
//! was needed and refused.
//!
//! Because it is undocumented it can change or vanish with any Azure
//! DevOps deployment. That is what all the logging is for: every request
//! and every reply is written whole, so the day it stops answering 200 the
//! log says what it answered instead. There are no retries - a fallback
//! that loops is a fallback nobody can diagnose.
//!
//! Only GET and POST leave this file, like everywhere else in the client.

use serde_json::json;

use super::EnsuredSuite;
use crate::ado::endpoints::percent_encode_segment;
use crate::ado::{tidy, AdoClient, AdoError};
use crate::cache::{self, keys};

/// The controller version seen in the browser on 2026-09-22. Kept as a
/// constant with that date beside it: if Azure DevOps bumps it
/// server-side, the reply is logged whole and this is the one line to
/// change.
pub const BOARDS_ROUTE_VERSION: &str = "5";

/// The Boards "Add Test" body as far as it is known.
///
/// UNCONFIRMED: the request body itself was never observed (the page
/// blocks cross-origin reads of its script bundles), so the field names
/// here are inferred from the sibling call the same controller DID make -
/// `GetWitTestsForKanbanBoard` with `{"userStoryIds":"[145386]"}`, JSON
/// whose array values are JSON strings. The shape follows that
/// convention; a 400 naming a field is how a wrong guess announces
/// itself, and `boards_add_to_requirement_suite` keeps that body in the
/// log for exactly that reason. Design §4.1.
pub fn boards_body(pbi_id: i32, case_ids: &[i32]) -> serde_json::Value {
    let ids: Vec<String> = case_ids.iter().map(|id| id.to_string()).collect();
    json!({
        "requirementId": pbi_id,
        "testCaseIds": format!("[{}]", ids.join(",")),
    })
}

/// What the fallback resolved on its way to the suite. The two ids are
/// returned rather than kept private so the caller can name them in the
/// log without resolving them a second time.
#[derive(Debug, Clone, PartialEq)]
pub struct BoardsOutcome {
    pub suite: EnsuredSuite,
    pub project_id: String,
    pub team_id: String,
}

/// What the development-build probe says back after taking the route
/// once by hand (design §4.5).
///
/// The body above is a guess until this has answered 200, so each answer
/// is reported as the next edit it asks for rather than as a bare status:
/// 200 means wire it, 400 names the field to rename in `boards_body`, 403
/// is the design's one assumption failing, and anything else is repeated
/// as it stands, because there is nothing to read into a token that
/// expired or a host that never answered.
pub fn probe_report(out: &Result<BoardsOutcome, AdoError>) -> String {
    match out {
        Ok(out) => format!(
            "200: plan {} \"{}\", suite {}, project id {}, team id {} - wire it",
            out.suite.plan_id, out.suite.plan_name, out.suite.suite_id, out.project_id, out.team_id
        ),
        // A 400 is the useful failure: its body names the field this
        // request got wrong. Kept in the report rather than left in the
        // log, trimmed to what `refused()` keeps of every other refusal so
        // a page of controller HTML cannot fill the panel.
        Err(AdoError::Http { status: 400, body }) => {
            let said: String = body.chars().take(600).collect();
            format!("400: the body's field names are wrong - Azure DevOps said: {said}")
        }
        // The first real call answered 500, not 400, to the guessed body: an
        // internal controller does not validate politely. Its body is still
        // the only clue, so it is shown the same way.
        Err(AdoError::Http { status, body }) if *status != 0 => {
            let said: String = body.chars().take(600).collect();
            format!("{status}: Azure DevOps said: {said}")
        }
        // Status 0 is this app's own sentence (no plan id in the reply, no
        // suite in the named plan); it reads better than "http 0".
        Err(AdoError::Http { status: 0, body }) => body.clone(),
        Err(AdoError::Forbidden) => {
            "403: the route refused a bearer token - the design stops here (§4.1 assumption)"
                .to_string()
        }
        Err(e) => format!("{e}"),
    }
}

/// The project's GUID and its default team's GUID - both read from the one
/// projects call, because the fallback needs the second only when no
/// team's area covers the PBI and a second request for it would be a
/// request for something already in hand.
#[derive(Clone)]
struct ProjectIds {
    id: String,
    default_team_id: String,
}

/// Area paths arrive with either slash and in whatever case someone typed
/// them; `HRM/Gamma Guardians` and `HRM\Gamma Guardians` are one area.
fn normalize_area(area: &str) -> String {
    area.trim().replace('/', "\\").to_lowercase()
}

/// True when a team's team-field value owns `area`. Equal always counts;
/// a parent only counts when the team subscribed to its children, which
/// is what `includeChildren` says.
fn covers(value: &str, include_children: bool, area: &str) -> bool {
    let value = normalize_area(value);
    let area = normalize_area(area);
    if value.is_empty() || area.is_empty() {
        return false;
    }
    area == value || (include_children && area.starts_with(&format!("{value}\\")))
}

fn no_case_ids() -> AdoError {
    AdoError::Http {
        status: 0,
        body: "the Boards route needs at least one test case id".to_string(),
    }
}

impl AdoClient {
    /// The project's GUID. The Boards URL takes the id, not the name.
    pub async fn project_id(&self, org: &str, project: &str) -> Result<String, AdoError> {
        Ok(self.project_ids(org, project).await?.id)
    }

    /// Both ids from the projects call, remembered for the session: they
    /// are org configuration, so re-reading them per upload would be a
    /// request that can only ever answer the same thing.
    async fn project_ids(&self, org: &str, project: &str) -> Result<ProjectIds, AdoError> {
        let key = keys::project_id(&self.base_url, org, project);
        if let Some(hit) = cache::session_fresh::<ProjectIds>(&key, keys::BOARDS_IDS_TTL) {
            return Ok(hit);
        }
        let url = format!(
            "{}/{}/_apis/projects/{}?api-version=7.1",
            self.base_url,
            percent_encode_segment(org),
            percent_encode_segment(project)
        );
        let data = self.get_json(url).await?;
        let id = data["id"].as_str().unwrap_or_default().to_string();
        // An empty id would go into the route's URL as nothing at all -
        // `{base}/{org}//_api/...` - and come back a 404 that describes
        // none of this. Say what could not be read instead.
        if id.is_empty() {
            return Err(AdoError::Http {
                status: 0,
                body: "the project could not be read - Azure DevOps answered without an id"
                    .to_string(),
            });
        }
        let ids = ProjectIds {
            id,
            default_team_id: data["defaultTeam"]["id"].as_str().unwrap_or_default().to_string(),
        };
        cache::session_put(&key, ids.clone());
        Ok(ids)
    }

    /// The team whose area path owns the PBI - the team context the Boards
    /// route runs in.
    ///
    /// Areas nest and so do team scopes: a project-wide team covers every
    /// PBI in the project, so the longest covering scope wins or the
    /// answer would always be whichever team happened to come back first.
    /// Nothing covering means the project's default team, which is what
    /// Boards itself falls back to.
    ///
    /// A team whose settings this account cannot read is SKIPPED, not
    /// fatal: the fallback only runs at all after Azure DevOps has
    /// already refused something for this account's access level, so a
    /// 403 on one team's settings is the expected shape of its day - and
    /// the team that does own the area is still in the list. Every skip
    /// is logged by name, because a wrong team is a suite on the wrong
    /// board and the log is where that gets explained.
    pub async fn team_for_area(
        &self,
        org: &str,
        project: &str,
        project_id: &str,
        area_path: &str,
    ) -> Result<String, AdoError> {
        // Keyed by the NORMALISED area: `HRM\Gamma Guardians` and
        // `HRM/Gamma Guardians` are one area, so they are one entry.
        let key = keys::area_team(&self.base_url, org, project, &normalize_area(area_path));
        if let Some(hit) = cache::session_fresh::<String>(&key, keys::BOARDS_IDS_TTL) {
            return Ok(hit);
        }
        // A refused team LIST is the same day as a refused team scope
        // below, one level up: the fallback only runs after this account
        // was already told no, so 403 and 404 here are answers, not
        // faults, and the project's default team is what Boards itself
        // falls back to. Anything else - no network, a rate limit, a 500 -
        // is not an answer about permission and must not be dressed up as
        // one, so it still aborts.
        let teams = match self.list_teams(org, project_id).await {
            Ok(teams) => teams,
            Err(e) if matches!(e, AdoError::Forbidden | AdoError::NotFound) => {
                crate::applog::warn(format!(
                    "boards suite route: could not list the project's teams ({e}) - falling back to the project's default team"
                ));
                vec![]
            }
            Err(e) => return Err(e),
        };
        let mut best: Option<(usize, String)> = None;
        for team in &teams {
            let values = match self.get_team_scope(org, project_id, &team.id).await {
                Ok((_field, values)) => values,
                Err(e) => {
                    crate::applog::warn(format!(
                        "boards suite route: could not read the area scope of team {} ({}): {e} - skipping it",
                        team.name, team.id
                    ));
                    continue;
                }
            };
            for (value, include_children) in values {
                if !covers(&value, include_children, area_path) {
                    continue;
                }
                let depth = normalize_area(&value).chars().count();
                if best.as_ref().map_or(true, |(best_depth, _)| depth > *best_depth) {
                    best = Some((depth, team.id.clone()));
                }
            }
        }
        let team_id = match best {
            Some((_, id)) => id,
            None => {
                let default = self.project_ids(org, project).await?.default_team_id;
                // Checked HERE and not where it is read, because a project
                // with no readable default team is only a problem for a PBI
                // no team's area covers. An empty id would go into the
                // route's URL as `?teamId=` and come back something that
                // explains none of this.
                if default.is_empty() {
                    return Err(AdoError::Http {
                        status: 0,
                        body:
                            "the project could not be read - Azure DevOps answered without a default team"
                                .to_string(),
                    });
                }
                crate::applog::info(format!(
                    "boards suite route: no team's area covers {area_path} - using the project's default team {default}"
                ));
                default
            }
        };
        cache::session_put(&key, team_id.clone());
        Ok(team_id)
    }

    /// The route the Boards "Add Test" button takes. INTERNAL portal
    /// controller (`_api`, not `_apis`) - see this file's header. Adds
    /// `case_ids` to the PBI's requirement suite, creating the team's
    /// current-sprint plan and the suite when they do not exist. Returns
    /// the plan id the reply names; the suite id is NOT in the reply and
    /// is found by listing that plan's suites.
    pub async fn boards_add_to_requirement_suite(
        &self,
        org: &str,
        project_id: &str,
        team_id: &str,
        pbi_id: i32,
        case_ids: &[i32],
    ) -> Result<i32, AdoError> {
        if case_ids.is_empty() {
            return Err(no_case_ids());
        }
        let url = format!(
            "{}/{}/{}/_api/_testManagement/AddWitTestCasesToRequirementSuite?teamId={}&__v={}",
            self.base_url,
            percent_encode_segment(org),
            project_id,
            team_id,
            BOARDS_ROUTE_VERSION
        );
        let body = boards_body(pbi_id, case_ids);
        crate::applog::info(format!("boards suite route: POST {} body {}", tidy(&url), body));
        // A 401/403/404 body is already logged by the transport's
        // `refused()`. Every other status keeps its body in `AdoError::Http`
        // and its Display drops it - so it is written here, whole: the first
        // real call answered 500 with nothing in the log to say why.
        let data = match self.post_json(url, &body).await {
            Ok(d) => d,
            Err(AdoError::Http { status, body: said }) => {
                let said: String = said.chars().take(2000).collect();
                crate::applog::warn(format!("boards suite route answered {status}: {said}"));
                return Err(AdoError::Http { status, body: said });
            }
            Err(e) => return Err(e),
        };
        let raw: String = data.to_string().chars().take(2000).collect();
        crate::applog::info(format!("boards suite route answered: {raw}"));
        data["testPlanId"]
            .as_i64()
            .map(|plan| plan as i32)
            .ok_or_else(|| AdoError::Http {
                status: 0,
                body: format!("the Boards route answered without a testPlanId: {raw}"),
            })
    }

    /// The whole fallback: resolve the two ids, take the route, then find
    /// the suite the plan it named now holds. The caller remembers the
    /// result and tells the user about it - this only reports what
    /// happened.
    pub async fn boards_fallback(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        case_ids: &[i32],
    ) -> Result<BoardsOutcome, AdoError> {
        // Checked before anything is resolved: with nothing uploaded there
        // is nothing to add, and the ids would be looked up only to be
        // thrown away.
        if case_ids.is_empty() {
            return Err(no_case_ids());
        }
        let project_id = self.project_id(org, project).await?;
        let team_id = self.team_for_area(org, project, &project_id, area_path).await?;
        let plan_id = self
            .boards_add_to_requirement_suite(org, &project_id, &team_id, pbi_id, case_ids)
            .await?;
        let suite = self
            .find_requirement_suite(org, project, plan_id, pbi_id)
            .await?
            .ok_or_else(|| AdoError::Http {
                status: 0,
                body: format!(
                    "the Boards route named plan {plan_id} but that plan has no requirement suite for #{pbi_id}"
                ),
            })?;
        let plan = self.get_test_plan(org, project, plan_id).await?;
        crate::applog::info(format!(
            "requirement suite for #{pbi_id} created through the Boards route: plan {plan_id}, suite {}",
            suite.id
        ));
        Ok(BoardsOutcome {
            suite: EnsuredSuite {
                plan_id,
                plan_name: plan.name,
                suite_id: suite.id,
                // `created_plan` is the app's own "no plan existed, so one
                // was made under this area" flag, which drives a sentence
                // about the plan scan. The Boards route is a different
                // story and the caller tells it in its own words.
                created_plan: false,
            },
            project_id,
            team_id,
        })
    }
}
