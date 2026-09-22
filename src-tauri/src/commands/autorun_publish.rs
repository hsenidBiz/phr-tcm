//! The one Auto Run command that reaches Azure DevOps, and only because a
//! person pressed Send on a run they had reviewed. GET, POST and PATCH.

#[tauri::command]
#[specta::specta]
pub async fn auto_run_publish(
    app: tauri::AppHandle,
    organization: String,
    project: String,
    pbi_id: i32,
    run_id: String,
    run_name: String,
    cases: Vec<crate::autorun::publish::PublishCase>,
) -> Result<crate::autorun::publish::PublishResult, crate::ado::AdoError> {
    use crate::autorun::publish::PublishResult;
    let root = match super::autorun::root(&app) {
        Ok(r) => r,
        Err(why) => return Ok(PublishResult::Refused { why }),
    };
    let token = crate::state::get_fresh_token(&app).await?;
    let client = crate::ado::AdoClient::new(token);
    // Read-only: a send never creates a plan or a suite - that already
    // happened, in Run Tests, before this screen could ever offer Send.
    let (area, _iteration) = client.get_work_item_paths(&organization, &project, pbi_id).await?;

    // The suite is resolved from the cache shared with Run Tests and the
    // AI bridge first - scanning every plan takes about a minute on a
    // large org - and only scanned fresh when there is nothing cached, or
    // when a cached suite turns out to have been deleted in Azure DevOps
    // since it was resolved (a 404 on its points), in which case it is
    // forgotten and looked up once more.
    let mut retried = false;
    loop {
        let cached = crate::ado_testplan::cached_suite(&client.base_url, &organization, &project, pbi_id);
        let (suite, from_cache) = match cached {
            Some(s) => (s, true),
            None => match client.find_pbi_requirement_suite(&organization, &project, pbi_id, &area).await? {
                Some(s) => {
                    crate::ado_testplan::remember_suite(&client.base_url, &organization, &project, pbi_id, &s);
                    (s, false)
                }
                None => {
                    return Ok(PublishResult::Refused {
                        why: "this PBI has no test suite in Azure DevOps yet - open Run Tests for it once, which creates the suite, then send again".into(),
                    });
                }
            },
        };
        crate::applog::warn(format!(
            "auto-run publish: sending run {run_id} for PBI #{pbi_id} to plan #{}",
            suite.plan_id
        ));
        match crate::autorun::publish::publish_run(&client, &root, &organization, &project, &suite, &run_name, &run_id, &cases).await {
            Err(crate::ado::AdoError::NotFound) if from_cache && !retried => {
                crate::ado_testplan::forget_suite(&client.base_url, &organization, &project, pbi_id);
                retried = true;
            }
            other => return other,
        }
    }
}
