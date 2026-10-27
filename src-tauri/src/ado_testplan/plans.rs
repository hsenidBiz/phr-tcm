//! Plan and suite discovery, and find-or-create of the PBI's
//! requirement-based suite.

use futures::stream::{self, StreamExt};

use super::{
    area_matches, default_plan_name, EnsuredSuite, PlanWithSuites, SuiteRef, TestPlan,
    SUITE_SCAN_CONCURRENCY,
};
use crate::ado::single_flight::{Flights, ScanProgress};
use crate::ado::{AdoClient, AdoError};

/// Tells when a walk is waiting on one plan alone. Each plan of a walk
/// (or of one batch of it) reports the pages of its suite list it has read
/// and, when it ends, that it has; once every plan but one has ended, that
/// one's page count is what the walk can honestly say it is doing. A plan
/// read in a single page never reports pages, so a small project's walk
/// never shows any.
struct Stragglers {
    pages: Vec<u32>,
    ended: Vec<bool>,
}

impl Stragglers {
    fn new(plans: usize) -> Self {
        Self { pages: vec![0; plans], ended: vec![false; plans] }
    }

    /// The one plan not yet ended, when it is the only one.
    fn alone(&self) -> Option<usize> {
        let mut open = (0..self.ended.len()).filter(|&i| !self.ended[i]);
        match (open.next(), open.next()) {
            (Some(i), None) => Some(i),
            _ => None,
        }
    }

    /// Plan `i` has read `pages` pages. The count to report, if it is the
    /// plan the walk is waiting on alone.
    fn page(&mut self, i: usize, pages: u32) -> Option<u32> {
        if self.ended[i] {
            return None;
        }
        self.pages[i] = pages;
        (self.alone() == Some(i)).then_some(pages)
    }

    /// Plan `i` has ended. The count to report, if that leaves one plan
    /// that is already paging.
    fn end(&mut self, i: usize) -> Option<u32> {
        self.ended[i] = true;
        self.alone().map(|j| self.pages[j]).filter(|&p| p > 0)
    }
}

/// How many of a PBI's test cases `suite_from_cases` asks about before it
/// gives up and lets the plans be walked.
const SUITE_FROM_CASES_ASKED: usize = 3;

/// One of a PBI's requirement suites, as a test case's answer lists it.
/// `plan` is filled when the plan was read (to confirm it, or to rank it);
/// `in_project` is whether the suite itself named this project.
struct SuiteCandidate {
    plan_id: i32,
    suite_id: i32,
    plan_name: String,
    in_project: bool,
    plan: Option<TestPlan>,
}

/// What looking for a PBI's suite came to: the suite if there is one, and
/// the project's plans when they had to be read (so a caller about to
/// create a suite does not read them again). `plans` is empty when the
/// PBI's own test cases answered.
#[derive(Clone)]
struct SuiteWalk {
    found: Option<EnsuredSuite>,
    plans: std::sync::Arc<Vec<TestPlan>>,
}

/// The suite lookups running now, one per (server, signed-in account, org,
/// project, PBI, area, how far it looks). See `ado::single_flight`.
static SUITE_WALKS: Flights<SuiteWalk> = Flights::new();

/// The plans-and-suites walks running now, one per (server, signed-in
/// account, org, project).
static PLAN_TREES: Flights<Vec<PlanWithSuites>> = Flights::new();

impl AdoClient {
    /// All test plans in the project (paginated). Read only.
    pub async fn get_test_plans(&self, org: &str, project: &str) -> Result<Vec<TestPlan>, AdoError> {
        let mut plans = vec![];
        let mut continuation: Option<String> = None;
        let mut pages = 0usize;
        loop {
            let mut url = format!("{}/testplan/plans?api-version=7.1", self.tp_base(org, project));
            if let Some(c) = &continuation {
                super::push_continuation(&mut url, c);
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for p in data["value"].as_array().cloned().unwrap_or_default() {
                plans.push(TestPlan {
                    id: p["id"].as_i64().unwrap_or_default() as i32,
                    name: p["name"].as_str().unwrap_or_default().to_string(),
                    area_path: p["areaPath"].as_str().unwrap_or_default().to_string(),
                    root_suite_id: p["rootSuite"]["id"].as_i64().map(|i| i as i32),
                    iteration: p["iteration"].as_str().unwrap_or_default().to_string(),
                    state: p["state"].as_str().unwrap_or_default().to_string(),
                });
            }
            pages += 1;
            continuation = match cont {
                Some(next) if super::may_continue(pages, continuation.as_ref(), &next, "test plans") => {
                    Some(next)
                }
                _ => break,
            };
        }
        Ok(plans)
    }

    /// A single plan including its root suite id. Read only.
    pub async fn get_test_plan(&self, org: &str, project: &str, plan_id: i32) -> Result<TestPlan, AdoError> {
        let url = format!(
            "{}/testplan/plans/{}?api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let data = self.get_json(url).await?;
        Ok(TestPlan {
            id: data["id"].as_i64().unwrap_or_default() as i32,
            name: data["name"].as_str().unwrap_or_default().to_string(),
            area_path: data["areaPath"].as_str().unwrap_or_default().to_string(),
            root_suite_id: data["rootSuite"]["id"].as_i64().map(|i| i as i32),
                    iteration: data["iteration"].as_str().unwrap_or_default().to_string(),
                    state: data["state"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// The requirement-based suite bound to `pbi_id` within `plan_id`, or
    /// None. Stops paging as soon as it matches. Read only.
    pub async fn find_requirement_suite(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        pbi_id: i32,
    ) -> Result<Option<SuiteRef>, AdoError> {
        self.find_requirement_suite_cb(org, project, plan_id, pbi_id, |_| {}).await
    }

    /// Same, calling `on_page` with the number of pages read so far each
    /// time the plan's suite list turns out to have another page.
    pub async fn find_requirement_suite_cb(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        pbi_id: i32,
        mut on_page: impl FnMut(u32),
    ) -> Result<Option<SuiteRef>, AdoError> {
        let mut continuation: Option<String> = None;
        let mut pages = 0usize;
        loop {
            let mut url = format!(
                "{}/testplan/Plans/{}/suites?api-version=7.1",
                self.tp_base(org, project),
                plan_id
            );
            if let Some(c) = &continuation {
                super::push_continuation(&mut url, c);
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for s in data["value"].as_array().cloned().unwrap_or_default() {
                if s["requirementId"].as_i64() == Some(pbi_id as i64)
                    && s["suiteType"].as_str() == Some("requirementTestSuite")
                {
                    return Ok(Some(SuiteRef {
                        id: s["id"].as_i64().unwrap_or_default() as i32,
                        name: s["name"].as_str().unwrap_or_default().to_string(),
                        suite_type: "requirementTestSuite".to_string(),
                        requirement_id: Some(pbi_id),
                        parent_id: s["parentSuite"]["id"].as_i64().map(|i| i as i32),
                    }));
                }
            }
            pages += 1;
            continuation = match cont {
                Some(next)
                    if super::may_continue(pages, continuation.as_ref(), &next, "plan suites") =>
                {
                    on_page(pages as u32);
                    Some(next)
                }
                _ => return Ok(None),
            };
        }
    }

    /// Every suite in a plan, flat (parent links let callers rebuild the
    /// tree). `on_page` hears the number of pages read so far each time
    /// the list turns out to have another page. Read only.
    pub async fn get_all_suites(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        mut on_page: impl FnMut(u32),
    ) -> Result<Vec<SuiteRef>, AdoError> {
        let mut suites = vec![];
        let mut continuation: Option<String> = None;
        let mut pages = 0usize;
        loop {
            let mut url = format!(
                "{}/testplan/Plans/{}/suites?api-version=7.1",
                self.tp_base(org, project),
                plan_id
            );
            if let Some(c) = &continuation {
                super::push_continuation(&mut url, c);
            }
            let (data, cont) = self.get_json_with_continuation(url).await?;
            for s in data["value"].as_array().cloned().unwrap_or_default() {
                suites.push(SuiteRef {
                    id: s["id"].as_i64().unwrap_or_default() as i32,
                    name: s["name"].as_str().unwrap_or_default().to_string(),
                    suite_type: s["suiteType"].as_str().unwrap_or_default().to_string(),
                    requirement_id: s["requirementId"].as_i64().map(|i| i as i32),
                    parent_id: s["parentSuite"]["id"].as_i64().map(|i| i as i32),
                });
            }
            pages += 1;
            continuation = match cont {
                Some(next)
                    if super::may_continue(pages, continuation.as_ref(), &next, "plan suites") =>
                {
                    on_page(pages as u32);
                    Some(next)
                }
                _ => break,
            };
        }
        Ok(suites)
    }

    /// Every plan with its suites, for the Test Suites browser. Plans whose
    /// suites can't be read (permissions) are skipped, and - the v1 rule the
    /// suite browser shipped with - plans containing no suites beyond their
    /// root are hidden entirely. Read only.
    pub async fn list_plans_with_suites(
        &self,
        org: &str,
        project: &str,
    ) -> Result<Vec<PlanWithSuites>, AdoError> {
        self.list_plans_with_suites_cb(org, project, |_| {}).await
    }

    /// Same, reporting (done, total) after each plan scanned so the UI can
    /// show "Scanning plans X of Y", and, once only one large plan is left,
    /// how many pages of its suite list have been read.
    pub async fn list_plans_with_suites_cb(
        &self,
        org: &str,
        project: &str,
        progress: impl FnMut(ScanProgress),
    ) -> Result<Vec<PlanWithSuites>, AdoError> {
        // Every screen that wants the tree shares one walk; a failed walk
        // is shared too, and never kept.
        let key = format!("{}|{}|{org}|{project}", self.base_url, self.account);
        let (me, org, project)= (self.clone(), org.to_string(), project.to_string());
        PLAN_TREES
            .run(
                &key,
                None,
                move |tx| async move {
                    me.walk_plans_with_suites(&org, &project, |p| {
                        let _ = tx.send(p);
                    })
                    .await
                },
                progress,
            )
            .await
    }

    async fn walk_plans_with_suites(
        &self,
        org: &str,
        project: &str,
        mut progress: impl FnMut(ScanProgress),
    ) -> Result<Vec<PlanWithSuites>, AdoError> {
        let plans = self.get_test_plans(org, project).await?;
        let total = plans.len() as u32;
        let mut done = 0u32;

        // One suites request per plan, run SUITE_SCAN_CONCURRENCY at a time
        // (serial scanning made big projects take minutes). Results are put
        // back in plan order so the browser output is stable.
        let mut fetched: Vec<Option<Result<Vec<SuiteRef>, AdoError>>> =
            plans.iter().map(|_| None).collect();
        {
            // Futures are created eagerly (concrete lifetimes keep the tauri
            // command macro happy); the async block only owns (index, future).
            let (page_tx, mut page_rx) = tokio::sync::mpsc::unbounded_channel::<(usize, u32)>();
            let futs: Vec<_> = plans
                .iter()
                .enumerate()
                .map(|(i, plan)| {
                    let tx = page_tx.clone();
                    let fut = self.get_all_suites(org, project, plan.id, move |n| {
                        let _ = tx.send((i, n));
                    });
                    async move { (i, fut.await) }
                })
                .collect();
            drop(page_tx);
            let mut in_flight = stream::iter(futs).buffer_unordered(SUITE_SCAN_CONCURRENCY);
            let mut watch = Stragglers::new(plans.len());
            loop {
                tokio::select! {
                    next = in_flight.next() => {
                        let Some((i, res)) = next else { break };
                        done += 1;
                        progress(ScanProgress::plans(done, total));
                        if let Some(pages) = watch.end(i) {
                            progress(ScanProgress { done, total, pages: Some(pages) });
                        }
                        fetched[i] = Some(res);
                    }
                    Some((i, n)) = page_rx.recv() => {
                        if let Some(pages) = watch.page(i, n) {
                            progress(ScanProgress { done, total, pages: Some(pages) });
                        }
                    }
                }
            }
        }

        let mut out = vec![];
        for (plan, res) in plans.into_iter().zip(fetched) {
            let suites = match res.expect("every plan index was filled") {
                Ok(s) => s,
                Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                Err(e) => return Err(e),
            };
            // The root suite is structural, not user content: a plan whose
            // only suite is its root has no suites worth browsing.
            let non_root: Vec<SuiteRef> = suites
                .into_iter()
                .filter(|s| Some(s.id) != plan.root_suite_id && s.suite_type != "")
                .collect();
            let non_root: Vec<SuiteRef> = if plan.root_suite_id.is_some() {
                non_root
            } else {
                // Root id unknown from the list endpoint: drop the first
                // suite only when it is the conventional "<plan name>" root.
                non_root
                    .into_iter()
                    .filter(|s| !(s.suite_type == "staticTestSuite" && s.name == plan.name))
                    .collect()
            };
            if non_root.is_empty() {
                continue;
            }
            // Children of the stripped root become top-level in the tree.
            let root_id = plan.root_suite_id;
            let non_root: Vec<SuiteRef> = non_root
                .into_iter()
                .map(|mut s| {
                    if s.parent_id == root_id {
                        s.parent_id = None;
                    }
                    s
                })
                .collect();
            out.push(PlanWithSuites { plan, suites: non_root });
        }
        Ok(out)
    }

    /// POST a new test plan (creating one also creates its root suite).
    pub async fn create_test_plan(
        &self,
        org: &str,
        project: &str,
        name: &str,
        area_path: &str,
        iteration: &str,
    ) -> Result<TestPlan, AdoError> {
        let mut body = serde_json::json!({ "name": name });
        if !area_path.is_empty() {
            body["areaPath"] = serde_json::json!(area_path);
        }
        if !iteration.is_empty() {
            body["iteration"] = serde_json::json!(iteration);
        }
        let url = format!("{}/testplan/plans?api-version=7.1", self.tp_base(org, project));
        let data = self.post_json(url, &body).await?;
        Ok(TestPlan {
            id: data["id"].as_i64().unwrap_or_default() as i32,
            name: data["name"].as_str().unwrap_or(name).to_string(),
            area_path: data["areaPath"].as_str().unwrap_or(area_path).to_string(),
            root_suite_id: data["rootSuite"]["id"].as_i64().map(|i| i as i32),
                    iteration: data["iteration"].as_str().unwrap_or_default().to_string(),
                    state: data["state"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// POST a requirement-based suite bound to the PBI under the plan's root
    /// suite. ADO auto-populates it from the PBI's Tested By links.
    pub async fn create_requirement_suite(
        &self,
        org: &str,
        project: &str,
        plan_id: i32,
        root_suite_id: i32,
        pbi_id: i32,
    ) -> Result<i32, AdoError> {
        let body = serde_json::json!({
            "suiteType": "requirementTestSuite",
            "requirementId": pbi_id,
            "parentSuite": {"id": root_suite_id},
        });
        let url = format!(
            "{}/testplan/Plans/{}/suites?api-version=7.1",
            self.tp_base(org, project),
            plan_id
        );
        let data = self.post_json(url, &body).await?;
        Ok(data["id"].as_i64().unwrap_or_default() as i32)
    }

    /// Scan `plans` for the PBI's requirement suite - area-matched plans
    /// first (the common case finds it quickly), probed in concurrent
    /// batches; join_all keeps plan order within a batch so the FIRST plan
    /// holding the suite wins and the scan stops early. Read only.
    async fn scan_plans_for_suite(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        plans: &[TestPlan],
        exhaustive: bool,
        progress: &mut impl FnMut(ScanProgress),
    ) -> Result<Option<EnsuredSuite>, AdoError> {
        let mut ordered: Vec<&TestPlan> = plans.iter().collect();
        ordered.sort_by_key(|p| if area_matches(&p.area_path, area_path) { 0 } else { 1 });
        // A caller about to CREATE a suite would only ever create it under
        // an area-matched plan, so once those are checked the rest of the
        // scan can only find a suite it would not have used anyway. On a
        // project with hundreds of plans that is the difference between a
        // few requests and a minute. The read-only lookup stays exhaustive:
        // Run Tests must find a suite wherever someone put it.
        if !exhaustive {
            let matched = ordered.iter().filter(|p| area_matches(&p.area_path, area_path)).count();
            if matched > 0 {
                ordered.truncate(matched);
            }
        }
        let total = ordered.len() as u32;
        let mut done = 0u32;
        for batch in ordered.chunks(SUITE_SCAN_CONCURRENCY) {
            // (plan in the batch, pages read so far) while it is paging;
            // (plan, None) when it has ended.
            let (tick_tx, mut ticks) = tokio::sync::mpsc::unbounded_channel::<(usize, Option<u32>)>();
            let mut all = Box::pin(futures::future::join_all(batch.iter().enumerate().map(
                |(i, plan)| {
                    let (page_tx, end_tx) = (tick_tx.clone(), tick_tx.clone());
                    let fut = self.find_requirement_suite_cb(org, project, plan.id, pbi_id, move |n| {
                        let _ = page_tx.send((i, Some(n)));
                    });
                    async move {
                        let res = fut.await;
                        let _ = end_tx.send((i, None));
                        res
                    }
                },
            )));
            drop(tick_tx);
            let mut watch = Stragglers::new(batch.len());
            let results = loop {
                tokio::select! {
                    results = &mut all => break results,
                    Some((i, tick)) = ticks.recv() => {
                        let pages = match tick {
                            Some(n) => watch.page(i, n),
                            None => watch.end(i),
                        };
                        if let Some(pages) = pages {
                            progress(ScanProgress { done, total, pages: Some(pages) });
                        }
                    }
                }
            };
            done += batch.len() as u32;
            progress(ScanProgress::plans(done, total));
            for (plan, res) in batch.iter().zip(results) {
                // A plan whose suites can't be listed (permissions) is
                // skipped rather than aborting the search; auth/rate-limit
                // errors still propagate so callers can re-auth / back off.
                match res {
                    Ok(Some(suite)) => {
                        return Ok(Some(EnsuredSuite {
                            plan_id: plan.id,
                            plan_name: plan.name.clone(),
                            suite_id: suite.id,
                            created_plan: false,
                        }))
                    }
                    Ok(None) => {}
                    Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(None)
    }

    /// The PBI's requirement suite, asked of the PBI's own test cases: Azure
    /// DevOps answers "which suites hold test case N" in one request
    /// (`GET {org}/_apis/testplan/suites?testCaseId=N`, each suite with its
    /// plan, project, `suiteType` and `requirementId`), where walking plans
    /// reads every plan's suite list. Asks up to the first three Tested By
    /// cases, one after another, and stops at the first that names one of
    /// this PBI's requirement suites. A PBI can have one in several plans
    /// (one per release, an old plan copied from, another team's), all
    /// listed by that one answer; they are ranked as the plan walk ranks
    /// plans - the plan for the PBI's area first, then an active plan, then
    /// the newest - so the lookup and the walk agree. None means "not found
    /// this way" - no cases, none in this PBI's requirement suite, or the
    /// service would not say - and the caller falls back to the plan walk.
    /// Read only.
    pub async fn suite_from_cases(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
    ) -> Result<Option<EnsuredSuite>, AdoError> {
        // A PBI that cannot be read or has no links is not an error here:
        // the walk that follows reports whatever is really wrong. The same
        // goes for an answer this shortcut cannot use (a 400, a 5xx): it is
        // logged and the walk decides. A bad sign-in or a throttle is not
        // let past, since the walk would only meet it too.
        let case_ids = match self.get_pbi_tested_by_ids(org, pbi_id).await {
            Ok(ids) => ids,
            Err(AdoError::Forbidden) | Err(AdoError::NotFound) => return Ok(None),
            Err(AdoError::Http { status, .. }) => {
                crate::applog::warn(format!(
                    "reading PBI {pbi_id} for its test cases answered http {status}; walking the plans instead"
                ));
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        for case_id in case_ids.into_iter().take(SUITE_FROM_CASES_ASKED) {
            let url = format!(
                "{}/{}/_apis/testplan/suites?testCaseId={}&api-version=7.1",
                self.base_url,
                crate::ado::endpoints::percent_encode_segment(org),
                case_id
            );
            let data = match self.get_json(url).await {
                Ok(d) => d,
                Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                Err(AdoError::Http { status, .. }) => {
                    // The shortcut is not served here (or hiccuped): the
                    // other cases would get the same answer.
                    crate::applog::warn(format!(
                        "asking which suites hold test case {case_id} answered http {status}; walking the plans instead"
                    ));
                    return Ok(None);
                }
                Err(e) => return Err(e),
            };
            let suites = data["value"].as_array().or_else(|| data.as_array()).cloned().unwrap_or_default();
            // Every suite of this PBI that this answer lists. The call is
            // org wide, so one may be another project's: the suite names
            // its project when it can; when it does not, the plan read in
            // `project` decides.
            let mut candidates: Vec<SuiteCandidate> = vec![];
            for s in suites {
                if s["suiteType"].as_str() != Some("requirementTestSuite")
                    || s["requirementId"].as_i64() != Some(pbi_id as i64)
                {
                    continue;
                }
                let (Some(suite_id), Some(plan_id)) = (s["id"].as_i64(), s["plan"]["id"].as_i64()) else {
                    continue;
                };
                let named = s["project"]["name"].as_str().filter(|n| !n.is_empty());
                match named {
                    Some(n) if n.eq_ignore_ascii_case(project) => {}
                    Some(_) => continue,
                    None => {}
                }
                candidates.push(SuiteCandidate {
                    plan_id: plan_id as i32,
                    suite_id: suite_id as i32,
                    plan_name: s["plan"]["name"].as_str().unwrap_or_default().to_string(),
                    in_project: named.is_some(),
                    plan: None,
                });
            }
            if candidates.is_empty() {
                continue;
            }
            // One candidate that names this project needs nothing more.
            // Otherwise each plan is read once: it confirms a suite that
            // named no project, and its area, state and id rank the rest.
            if !(candidates.len() == 1 && candidates[0].in_project) {
                let mut kept = Vec::with_capacity(candidates.len());
                for mut c in candidates {
                    match self.get_test_plan(org, project, c.plan_id).await {
                        Ok(p) => c.plan = Some(p),
                        Err(AdoError::Forbidden) | Err(AdoError::NotFound) if c.in_project => {}
                        Err(AdoError::Forbidden) | Err(AdoError::NotFound) => continue,
                        Err(AdoError::Http { status, .. }) => {
                            crate::applog::warn(format!(
                                "reading test plan {} answered http {status}; leaving its suite out of the shortcut",
                                c.plan_id
                            ));
                            if !c.in_project {
                                continue;
                            }
                        }
                        Err(e) => return Err(e),
                    }
                    kept.push(c);
                }
                candidates = kept;
            }
            candidates.sort_by_key(|c| {
                let (matched, active) = match &c.plan {
                    Some(p) => (area_matches(&p.area_path, area_path), is_active(&p.state)),
                    None => (false, false),
                };
                std::cmp::Reverse((matched, active, c.plan_id))
            });
            if let Some(best) = candidates.into_iter().next() {
                let plan_name = match &best.plan {
                    Some(p) if !p.name.is_empty() => p.name.clone(),
                    _ => best.plan_name,
                };
                return Ok(Some(EnsuredSuite {
                    plan_id: best.plan_id,
                    plan_name,
                    suite_id: best.suite_id,
                    created_plan: false,
                }));
            }
        }
        Ok(None)
    }

    /// Read-only lookup of the PBI's requirement suite across every plan -
    /// the background-prefetch variant of ensure: it NEVER creates a plan
    /// or suite, so it is safe to run without the user asking to run tests.
    /// The PBI's own test cases are asked first (`suite_from_cases`); the
    /// plans are walked only when that finds nothing.
    pub async fn find_pbi_requirement_suite(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
    ) -> Result<Option<EnsuredSuite>, AdoError> {
        self.find_pbi_requirement_suite_cb(org, project, pbi_id, area_path, |_| {}).await
    }

    /// Same, reporting the walk's progress (see `list_plans_with_suites_cb`).
    pub async fn find_pbi_requirement_suite_cb(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        progress: impl FnMut(ScanProgress),
    ) -> Result<Option<EnsuredSuite>, AdoError> {
        Ok(self
            .suite_walk(org, project, pbi_id, area_path, true, progress)
            .await?
            .found)
    }

    /// Look for the PBI's suite: its own test cases first, then the plans
    /// (`exhaustive`: every plan; otherwise the area-matched ones when
    /// there are any). Lookups for the same PBI started together share one
    /// walk, and a lookup that only needs the area-matched plans joins an
    /// exhaustive walk already running, since it covers them too. A failed
    /// walk is shared with whoever was waiting and is never kept.
    async fn suite_walk(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        exhaustive: bool,
        progress: impl FnMut(ScanProgress),
    ) -> Result<SuiteWalk, AdoError> {
        // The area is part of the key: it decides which suite wins when a
        // PBI has several, so a walk for another area is not this one's.
        let key = |all: bool| {
            format!("{}|{}|{org}|{project}|{pbi_id}|{}|{all}", self.base_url, self.account, area_path.trim())
        };
        let own = key(exhaustive);
        let wider = key(true);
        let also = if exhaustive { None } else { Some(wider.as_str()) };
        let (me, org, project, area) =
            (self.clone(), org.to_string(), project.to_string(), area_path.to_string());
        SUITE_WALKS
            .run(
                &own,
                also,
                move |tx| async move {
                    if let Some(found) = me.suite_from_cases(&org, &project, pbi_id, &area).await? {
                        return Ok(SuiteWalk { found: Some(found), plans: Default::default() });
                    }
                    let plans = me.get_test_plans(&org, &project).await?;
                    let found = me
                        .scan_plans_for_suite(&org, &project, pbi_id, &area, &plans, exhaustive, &mut |p| {
                            let _ = tx.send(p);
                        })
                        .await?;
                    Ok(SuiteWalk { found, plans: std::sync::Arc::new(plans) })
                },
                progress,
            )
            .await
    }

    /// Find-or-create the area-matched plan and the PBI's requirement suite,
    /// ported from v1 ensure_requirement_suite. Reuses any existing suite so
    /// nothing is duplicated. May POST; never DELETEs.
    pub async fn ensure_requirement_suite(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        iteration: &str,
    ) -> Result<EnsuredSuite, AdoError> {
        self.ensure_requirement_suite_cb(org, project, pbi_id, area_path, iteration, |_| {})
            .await
    }

    /// The upload's ensure: the same, except that an area whose suite create
    /// was refused within the last week is not asked again - the upload
    /// takes the Boards route after its cases exist instead (see
    /// `boards_after_refusal`). Nothing else may skip the ask: only the
    /// upload has a second route.
    pub async fn ensure_requirement_suite_for_upload(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        iteration: &str,
    ) -> Result<EnsuredSuite, AdoError> {
        self.ensure_inner(org, project, pbi_id, area_path, iteration, true, |_| {})
            .await
    }

    /// Same as `ensure_requirement_suite`, reporting (done, total) per plan
    /// scanned. Always asks the documented create, remembered refusal or not.
    pub async fn ensure_requirement_suite_cb(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        iteration: &str,
        progress: impl FnMut(ScanProgress),
    ) -> Result<EnsuredSuite, AdoError> {
        self.ensure_inner(org, project, pbi_id, area_path, iteration, false, progress)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn ensure_inner(
        &self,
        org: &str,
        project: &str,
        pbi_id: i32,
        area_path: &str,
        iteration: &str,
        skip_remembered: bool,
        progress: impl FnMut(ScanProgress),
    ) -> Result<EnsuredSuite, AdoError> {
        // The PBI's own test cases know their suite: they are asked before
        // any plan is read. Not exhaustive: a suite found under a plan for
        // another area is not one this would create under, so the
        // area-matched plans are the whole search when there are any.
        let walk = self.suite_walk(org, project, pbi_id, area_path, false, progress).await?;
        if let Some(found) = walk.found {
            return Ok(found);
        }
        let plans = walk.plans;

        // No suite anywhere. Every area-matched plan is a candidate, ranked
        // so the first is the one a person would pick: the most specific
        // area, then the plan whose iteration is the PBI's, then active
        // plans, then the newest. The old rule took the first plan the API
        // listed among equally specific ones - the OLDEST - which in one
        // project was a years-old plan the user could not create suites in:
        // the 403 was a warning in the log, 197 cases were created and
        // linked, and no suite appeared anywhere.
        let mut candidates: Vec<&TestPlan> = plans
            .iter()
            .filter(|p| area_matches(&p.area_path, area_path))
            .collect();
        candidates.sort_by(|a, b| {
            b.area_path
                .len()
                .cmp(&a.area_path.len())
                .then_with(|| {
                    iteration_matches(&b.iteration, iteration)
                        .cmp(&iteration_matches(&a.iteration, iteration))
                })
                .then_with(|| is_active(&b.state).cmp(&is_active(&a.state)))
                .then_with(|| b.id.cmp(&a.id))
        });
        if candidates.is_empty() {
            let plan = self
                .create_test_plan(org, project, &default_plan_name(area_path), area_path, iteration)
                .await?;
            let ensured = self.suite_under(org, project, &plan, pbi_id, true).await?;
            super::forget_area_refused(&self.base_url, org, project, &plan.area_path);
            return Ok(ensured);
        }
        // Try each candidate in turn - but only ONE per area path. "Manage
        // test suites" is an area-path permission, so a 403 in one plan is a
        // 403 in every plan that shares its area; asking each of them was 48
        // requests and 70 s of identical refusals (2026-09-22). A plan for a
        // DIFFERENT area is still worth asking. Anything but a 403 propagates
        // unchanged, so the caller keeps telling 401 and 429 apart from the rest.
        //
        // A refusal is also REMEMBERED per area for a week (2026-10-09): an
        // account without the access level was refused on every upload, and
        // the Boards route made the suite every time after. On the UPLOAD
        // (`skip_remembered`) a remembered area is not asked at all - it
        // counts as refused, and the upload takes the Boards route. Run
        // Tests has no Boards route, so it always asks: someone granted the
        // permission since must not be told "no permission" for a week.
        // Reading suites above is unaffected, so a suite that exists is
        // still found and used.
        let mut forbidden: Vec<(String, String, String, usize)> = vec![]; // (plan label, area key, area as named, skipped)
        for p in candidates {
            let area = super::area_key(&p.area_path);
            if let Some(f) = forbidden.iter_mut().find(|(_, a, _, _)| *a == area) {
                f.3 += 1;
                continue;
            }
            if skip_remembered && super::area_refused(&self.base_url, org, project, &p.area_path) {
                crate::applog::debug(format!(
                    "suite create for area '{}' was refused within the last week - not asked again",
                    p.area_path.trim()
                ));
                forbidden.push((format!("'{}' (id {})", p.name, p.id), area, p.area_path.trim().to_string(), 0));
                continue;
            }
            let plan = if p.root_suite_id.is_some() {
                p.clone()
            } else {
                self.get_test_plan(org, project, p.id).await?
            };
            match self.suite_under(org, project, &plan, pbi_id, false).await {
                Ok(ensured) => {
                    super::forget_area_refused(&self.base_url, org, project, &plan.area_path);
                    return Ok(ensured);
                }
                Err(AdoError::Forbidden) => {
                    crate::applog::debug(format!(
                        "no permission to create a suite in test plan '{}' (id {}) for area '{}' - skipping the other plans for that area",
                        plan.name, plan.id, plan.area_path
                    ));
                    super::remember_area_refused(&self.base_url, org, project, &plan.area_path);
                    forbidden.push((format!("'{}' (id {})", plan.name, plan.id), area, plan.area_path.trim().to_string(), 0));
                }
                Err(e) => return Err(e),
            }
        }
        // The one case a 403 is the whole answer: name the plan and the AREA
        // the permission lives on, so the person knows what to ask for.
        // Http rather than Forbidden so the sentence reaches the screen -
        // `describeAdoError` prints a Forbidden as a bare "no permission".
        let tried: Vec<String> = forbidden
            .iter()
            .map(|(label, _, area, skipped)| match skipped {
                0 => format!("{label} for area '{area}'"),
                1 => format!("{label} for area '{area}' (1 other plan for that area was not tried: the permission is per area)"),
                n => format!("{label} for area '{area}' ({n} other plans for that area were not tried: the permission is per area)"),
            })
            .collect();
        Err(AdoError::Http {
            status: 403,
            body: format!(
                "You don't have permission to create a test suite in {}. The test cases are linked to #{pbi_id}, but they will not appear in Run Tests until a requirement suite exists. Creating suites needs the Basic + Test Plans access level (Organization settings, Users) and 'Manage test suites' on that area; or add a test to #{pbi_id} from Boards once, which creates the suite.",
                tried.join("; ")
            ),
        })
    }

    /// The requirement suite for `pbi_id` under `plan`'s root suite.
    /// `created_plan` is whether this call created the plan too - a
    /// failure past that point leaves an empty test plan behind that this
    /// tool cannot delete, so it is named in the log and the error rather
    /// than left to appear out of nowhere.
    async fn suite_under(
        &self,
        org: &str,
        project: &str,
        plan: &TestPlan,
        pbi_id: i32,
        created_plan: bool,
    ) -> Result<EnsuredSuite, AdoError> {
        let orphan = |what: &str| {
            if created_plan {
                crate::applog::error(format!(
                    "left test plan '{}' (id {}) in {project} with no requirement suite - {what}. \
                     This tool never deletes; remove it in Azure DevOps if it is not wanted.",
                    plan.name, plan.id
                ));
            }
        };
        let root = match plan.root_suite_id {
            Some(r) => r,
            None => {
                orphan("it has no root suite");
                return Err(AdoError::Http {
                    status: 0,
                    body: format!(
                        "test plan '{}' (id {}) has no root suite, so the requirement suite for \
                         #{pbi_id} could not be created.",
                        plan.name, plan.id
                    ),
                });
            }
        };
        let suite_id = match self.create_requirement_suite(org, project, plan.id, root, pbi_id).await {
            Ok(id) => id,
            Err(e) => {
                orphan(&format!("creating the suite failed: {e}"));
                return Err(e);
            }
        };
        Ok(EnsuredSuite {
            plan_id: plan.id,
            plan_name: plan.name.clone(),
            suite_id,
            created_plan,
        })
    }
}

/// Does the plan's iteration cover the PBI's? Either may be the other's
/// ancestor: a plan on "Proj\\2026" covers a PBI in "Proj\\2026\\S3",
/// and a plan pinned to the sprint covers the PBI on it. Case-insensitive,
/// like every path Azure DevOps hands back.
fn iteration_matches(plan_iteration: &str, pbi_iteration: &str) -> bool {
    let a = plan_iteration.trim().to_lowercase();
    let b = pbi_iteration.trim().to_lowercase();
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || b.starts_with(&format!("{a}\\")) || a.starts_with(&format!("{b}\\"))
}

/// Unknown counts as active - the list API has always sent the state, but
/// a plan without one should not lose a tie to one that has it.
fn is_active(state: &str) -> bool {
    state.is_empty() || state.eq_ignore_ascii_case("active")
}

