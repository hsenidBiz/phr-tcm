//! Work-item-tracking endpoints (orgs, projects, PBIs, test cases, bugs,
//! tags, classification). Every method rides the `transport` helpers; the
//! writes are POST/PATCH create-or-update only - no DELETE in THIS file,
//! nor in any other except `recycle.rs`, which is the single audited
//! exception and is held to a tighter rule than this one. See its header.

use super::{
    tc_ids_i32, AdoClient, AdoError, BlankPolicy, BugTypeInfo, FieldRef, Org, PbiHit, Project,
    TestCaseFull, TestCaseSummary, WikiHit, WikiPage,
};

impl AdoClient {
    /// Two-hop org discovery, ported from v1 get_organizations():
    /// profile id first, then the accounts list. Sorted case-insensitively.
    pub async fn list_orgs(&self) -> Result<Vec<Org>, AdoError> {
        let me = self
            .get_json(format!(
                "{}/_apis/profile/profiles/me?api-version=6.0",
                self.vssps_base_url
            ))
            .await?;
        let member_id = me["id"].as_str().unwrap_or_default().to_string();
        let accounts = self
            .get_json(format!(
                "{}/_apis/accounts?memberId={}&api-version=6.0",
                self.vssps_base_url, member_id
            ))
            .await?;
        let mut orgs: Vec<Org> = accounts["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| {
                Some(Org {
                    name: a["accountName"].as_str()?.to_string(),
                    url: a["accountUri"].as_str().unwrap_or_default().to_string(),
                })
            })
            .collect();
        orgs.sort_by_key(|o| o.name.to_lowercase());
        Ok(orgs)
    }

    /// PBI search ported from v1 search_work_items(): title CONTAINS (quotes
    /// escaped), OR exact id when numeric, PBIs only, most recently changed
    /// first. WIQL order is preserved in the result.
    pub async fn search_pbis(
        &self,
        organization: &str,
        project: &str,
        text: &str,
        top: u32,
    ) -> Result<Vec<PbiHit>, AdoError> {
        let safe = text.trim().replace('\'', "''");
        let mut clause = format!("[System.Title] CONTAINS '{safe}'");
        if let Ok(id) = text.trim().parse::<i64>() {
            clause = format!("([System.Id] = {id} OR {clause})");
        }
        let wiql = format!(
            "SELECT [System.Id] FROM workitems \
             WHERE [System.TeamProject] = @project AND {clause} \
             AND [System.WorkItemType] = 'Product Backlog Item' \
             ORDER BY [System.ChangedDate] DESC"
        );
        let url = format!(
            "{}/{}/{}/_apis/wit/wiql?$top={}&api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project),
            top
        );
        let body = self
            .post_json_query(url, &serde_json::json!({ "query": wiql }))
            .await?;
        let ids: Vec<i64> = body["workItems"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|w| w["id"].as_i64())
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let ids_csv = ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let url = format!(
            "{}/{}/_apis/wit/workitems?ids={}&fields=System.Title,System.WorkItemType&api-version=7.1",
            self.base_url, percent_encode_segment(organization), ids_csv
        );
        let fetched = self.get_json(url).await?;
        let by_id: std::collections::HashMap<i64, &serde_json::Value> = fetched["value"]
            .as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|w| Some((w["id"].as_i64()?, &w["fields"])))
                    .collect()
            })
            .unwrap_or_default();
        Ok(ids
            .iter()
            .filter_map(|i| {
                let fields = by_id.get(i)?;
                Some(PbiHit {
                    id: *i as i32,
                    title: fields["System.Title"].as_str().unwrap_or_default().to_string(),
                    work_item_type: fields["System.WorkItemType"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                })
            })
            .collect())
    }

    /// Documentation search over the project's Azure DevOps Wiki. Uses the
    /// search-index host (almsearch.dev.azure.com in prod), derived from
    /// `base_url` by substring-swap so wiremock's mock-server override
    /// (which has no "dev.azure.com" substring) still lands on the same
    /// stub server untouched. This POST is a read-only query - same
    /// precedent as the WIQL search in `search_pbis` above - it does NOT
    /// violate the no-writes invariant.
    pub async fn search_wiki(
        &self,
        organization: &str,
        project: &str,
        query: &str,
        top: u32,
    ) -> Result<Vec<WikiHit>, AdoError> {
        let search_base = self.base_url.replacen("dev.azure.com", "almsearch.dev.azure.com", 1);
        let url = format!(
            "{search_base}/{organization}/{project}/_apis/search/wikisearchresults?api-version=7.1"
        );
        let body = self
            .post_json_query(url, &serde_json::json!({ "searchText": query, "$top": top }))
            .await?;
        Ok(body["results"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|hit| {
                let highlights = hit["hits"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .flat_map(|h| h["highlights"].as_array().cloned().unwrap_or_default())
                    .filter_map(|f| f.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
                    .join(" ... ");
                WikiHit {
                    file_name: hit["fileName"].as_str().unwrap_or_default().to_string(),
                    path: hit["path"].as_str().unwrap_or_default().to_string(),
                    wiki_name: hit["wiki"]["name"].as_str().unwrap_or_default().to_string(),
                    wiki_id: hit["wiki"]["id"].as_str().unwrap_or_default().to_string(),
                    highlights,
                }
            })
            .collect())
    }

    /// Full content of one wiki page (read-only GET), fetched after
    /// `search_wiki` narrows down a `wiki_id` + `path`.
    pub async fn get_wiki_page(
        &self,
        organization: &str,
        project: &str,
        wiki_id: &str,
        path: &str,
    ) -> Result<WikiPage, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wiki/wikis/{}/pages?path={}&includeContent=true&api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project),
            wiki_id,
            percent_encode_path(&wiki_page_path(path))
        );
        let body = self.get_json(url).await?;
        Ok(WikiPage {
            path: body["path"].as_str().unwrap_or(path).to_string(),
            content: body["content"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// Distinct values of `field_ref` actually used on the project's Test
    /// Cases - the fallback when the field definition has no picklist (many
    /// orgs keep Modules as plain values, not allowedValues). Scans the 200
    /// most recently changed cases with the field set; values are deduped
    /// case-insensitively (first casing wins) and sorted. Read only.
    pub async fn field_values_in_use(
        &self,
        organization: &str,
        project: &str,
        field_ref: &str,
    ) -> Result<Vec<String>, AdoError> {
        // Field refs come from the project's own field list, but they are
        // interpolated into WIQL - allow only reference-name characters.
        if field_ref.is_empty()
            || !field_ref.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        {
            return Ok(vec![]);
        }
        let wiql = format!(
            "SELECT [System.Id] FROM workitems \
             WHERE [System.TeamProject] = @project \
             AND [System.WorkItemType] = 'Test Case' \
             AND [{field_ref}] <> '' \
             ORDER BY [System.ChangedDate] DESC"
        );
        let url = format!(
            "{}/{}/{}/_apis/wit/wiql?$top=200&api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project)
        );
        let body = self
            .post_json_query(url, &serde_json::json!({ "query": wiql }))
            .await?;
        let ids: Vec<i64> = body["workItems"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|w| w["id"].as_i64())
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let mut values: Vec<String> = vec![];
        let mut seen = std::collections::HashSet::new();
        for chunk in ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&fields={}&api-version=7.1",
                self.base_url,
                organization,
                ids_csv,
                urlencoding::encode(field_ref)
            );
            let fetched = self.get_json(url).await?;
            for wi in fetched["value"].as_array().cloned().unwrap_or_default() {
                if let Some(v) = wi["fields"][field_ref].as_str() {
                    let t = v.trim();
                    if !t.is_empty() && seen.insert(t.to_lowercase()) {
                        values.push(t.to_string());
                    }
                }
            }
        }
        values.sort_by_key(|v| v.to_lowercase());
        Ok(values)
    }

    /// All Test Cases linked to a PBI via TestedBy relations, ported from v1
    /// get_test_cases_for_pbi(). Fetches in batches so there is no overall cap.
    pub async fn get_pbi_test_cases(
        &self,
        organization: &str,
        pbi_id: i32,
    ) -> Result<Vec<TestCaseSummary>, AdoError> {
        let url = format!(
            "{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url, percent_encode_segment(organization), pbi_id
        );
        let data = self.get_json(url).await?;
        let tc_ids: Vec<i64> = data["relations"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|r| r["rel"].as_str().map(is_tested_by_forward).unwrap_or(false))
            .filter_map(|r| r["url"].as_str()?.rsplit('/').next()?.parse().ok())
            .collect();
        if tc_ids.is_empty() {
            return Ok(vec![]);
        }

        let mut cases = Vec::with_capacity(tc_ids.len());
        for chunk in tc_ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&fields=System.Id,System.Title,System.Tags,Microsoft.VSTS.TCM.AutomationStatus&api-version=7.1",
                self.base_url, percent_encode_segment(organization), ids_csv
            );
            let fetched = self.get_json(url).await?;
            for w in fetched["value"].as_array().cloned().unwrap_or_default() {
                let fields = &w["fields"];
                cases.push(TestCaseSummary {
                    id: w["id"].as_i64().unwrap_or_default() as i32,
                    title: fields["System.Title"].as_str().unwrap_or_default().to_string(),
                    tags: fields["System.Tags"].as_str().unwrap_or_default().to_string(),
                    automation_status: fields["Microsoft.VSTS.TCM.AutomationStatus"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }
        Ok(cases)
    }

    /// POST a new Test Case work item, ported from v1 create_test_case.
    /// Only creates work items of type 'Test Case'. Returns the new id.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_test_case(
        &self,
        organization: &str,
        project: &str,
        tc: &crate::model::TestCase,
        module_ref: Option<&str>,
        area_path: &str,
        iteration_path: &str,
        preconditions_ref: Option<&str>,
    ) -> Result<i32, AdoError> {
        let mut patch = vec![
            serde_json::json!({"op": "add", "path": "/fields/System.Title", "value": tc.title}),
            serde_json::json!({"op": "add", "path": "/fields/Microsoft.VSTS.TCM.Steps",
                "value": crate::steps_xml::build_steps_xml(&tc.steps)}),
            serde_json::json!({"op": "add", "path": "/fields/Microsoft.VSTS.TCM.AutomationStatus",
                "value": tc.automation_status}),
        ];
        if !area_path.is_empty() {
            patch.push(serde_json::json!({"op": "add", "path": "/fields/System.AreaPath", "value": area_path}));
        }
        if !iteration_path.is_empty() {
            patch.push(serde_json::json!({"op": "add", "path": "/fields/System.IterationPath", "value": iteration_path}));
        }
        // Blankness is judged AFTER trimming: a field holding only spaces is
        // blank to the person who typed it, and treating it as content
        // wrote a module that matches no picklist entry and a precondition
        // of "<div>   </div>" - a field that looks empty and is not.
        if !tc.tags.trim().is_empty() {
            patch.push(serde_json::json!({"op": "add", "path": "/fields/System.Tags", "value": tc.tags.trim()}));
        }
        if let Some(m) = module_ref {
            if !tc.module_value.trim().is_empty() {
                patch.push(serde_json::json!({"op": "add", "path": format!("/fields/{m}"), "value": tc.module_value.trim()}));
            }
        }
        if let Some(p) = preconditions_ref {
            if !tc.preconditions.trim().is_empty() {
                patch.push(serde_json::json!({"op": "add", "path": format!("/fields/{p}"),
                    "value": format!("<div>{}</div>", escape_html(tc.preconditions.trim()))}));
            }
        }
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/$Test%20Case?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project)
        );
        let data = self
            .send_json_patch(reqwest::Method::POST, url, &serde_json::Value::Array(patch))
            .await?;
        // A create with no id is not a create. `unwrap_or_default` handed
        // back work item 0 and called it success, so the queue reported
        // "created #0" and the link that followed went looking for a work
        // item that does not exist.
        data["id"].as_i64().map(|id| id as i32).ok_or_else(|| AdoError::Http {
            status: 0,
            body: format!(
                "Azure DevOps accepted the test case but its response carried no work item id, \
                 so there is nothing to link or report: {data}"
            ),
        })
    }

    /// PATCH System.State and return the state ADO actually persisted.
    /// Server-side rules (required dates, disallowed transitions) can reject
    /// or rewrite the change - the response body is the truth, so callers
    /// must never assume the requested state was applied. On a 400 rule
    /// rejection, ADO's human-readable message replaces the raw JSON body.
    pub async fn set_work_item_state(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
        state: &str,
    ) -> Result<String, AdoError> {
        let patch = serde_json::json!([
            {"op": "add", "path": "/fields/System.State", "value": state}
        ]);
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), wi_id
        );
        let data = self
            .send_json_patch(reqwest::Method::PATCH, url, &patch)
            .await
            .map_err(|e| match e {
                AdoError::Http { status: 400, body } => {
                    let msg = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| v["message"].as_str().map(String::from))
                        .unwrap_or(body);
                    AdoError::Http { status: 400, body: msg }
                }
                other => other,
            })?;
        Ok(data["fields"]["System.State"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }

    /// PATCH a work item's fields ({reference_name: value}); the 'add' op
    /// creates-or-replaces. Ported from v1 update_work_item_fields.
    ///
    /// One exception to "creates-or-replaces": System.Tags. Azure DevOps
    /// treats `add` on that one field as a MERGE, so writing tags through
    /// here can only ever grow the list - use `tags_write_ops` when a
    /// removal has to stick.
    pub async fn update_work_item_fields(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
        fields: &[(String, String)],
    ) -> Result<(), AdoError> {
        let patch: Vec<serde_json::Value> = fields
            .iter()
            .map(|(r, v)| serde_json::json!({"op": "add", "path": format!("/fields/{r}"), "value": v}))
            .collect();
        self.patch_work_item_ops(organization, project, wi_id, patch).await
    }

    /// PATCH a work item with explicit JSON-Patch operations.
    async fn patch_work_item_ops(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
        ops: Vec<serde_json::Value>,
    ) -> Result<(), AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), wi_id
        );
        self.send_json_patch(reqwest::Method::PATCH, url, &serde_json::Value::Array(ops))
            .await?;
        Ok(())
    }

    /// Title and AutomationStatus are always overwritten (is_valid rejects an
    /// empty title before any submit). Tags, module and Preconditions follow
    /// `blanks` - see `BlankPolicy`, which is the whole of the difference
    /// between an import and someone editing a form.
    ///
    /// `original_steps_xml` is the Steps field as Azure DevOps currently
    /// holds it, when the caller has it. See `steps_patch` for why that
    /// matters - in short, without it, saving a case you only retitled
    /// deletes the formatting and screenshots from its steps.
    ///
    /// `original_tags` is System.Tags as Azure DevOps currently holds it,
    /// when the caller has it - it decides which PATCH op can actually
    /// REMOVE a tag. See `tags_write_ops`.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_test_case_from_model(
        &self,
        organization: &str,
        project: &str,
        tc_id: i32,
        tc: &crate::model::TestCase,
        module_ref: Option<&str>,
        preconditions_ref: Option<&str>,
        original_steps_xml: Option<&str>,
        original_tags: Option<&str>,
        blanks: BlankPolicy,
    ) -> Result<(), AdoError> {
        let mut fields = vec![
            ("System.Title".to_string(), tc.title.clone()),
            (
                "Microsoft.VSTS.TCM.AutomationStatus".to_string(),
                tc.automation_status.clone(),
            ),
        ];
        if let Some(xml) = steps_patch(&tc.steps, original_steps_xml) {
            fields.push(("Microsoft.VSTS.TCM.Steps".to_string(), xml));
        }
        // A blank is either "no opinion" or "erase it", and only the caller
        // knows which.
        // Trimmed, because a field holding only spaces is blank to the
        // person who left it that way. Judging it as content meant a Skip
        // import overwrote real tags with a space, and wrote a precondition
        // of "<div>   </div>" - which is the "looks blank but is not" case
        // the Clear branch below already went out of its way to avoid.
        let writes = |value: &str| blanks == BlankPolicy::Clear || !value.trim().is_empty();
        // Tags do NOT ride in `fields`: the plain `add` op those become
        // merges on this one field, which is exactly the "removed a tag,
        // nothing happened" bug. They get their own op(s) below.
        let tag_ops = if writes(&tc.tags) {
            tags_write_ops(&tc.tags, original_tags)
        } else {
            vec![]
        };
        if let Some(m) = module_ref {
            if writes(&tc.module_value) {
                fields.push((m.to_string(), tc.module_value.trim().to_string()));
            }
        }
        if let Some(p) = preconditions_ref {
            if writes(&tc.preconditions) {
                let text = tc.preconditions.trim();
                let html = if text.is_empty() {
                    String::new()
                } else {
                    format!("<div>{}</div>", escape_html(text))
                };
                fields.push((p.to_string(), html));
            }
        }
        let mut ops: Vec<serde_json::Value> = fields
            .iter()
            .map(|(r, v)| serde_json::json!({"op": "add", "path": format!("/fields/{r}"), "value": v}))
            .collect();
        ops.extend(tag_ops);
        self.patch_work_item_ops(organization, project, tc_id, ops).await
    }

    /// PATCH the Test Case to add a TestedBy-Reverse relation to the PBI -
    /// 'Tests' on the Test Case side, 'Tested By' on the PBI side. The PBI
    /// itself is never modified directly. Ported from v1 link_to_pbi.
    pub async fn link_to_pbi(
        &self,
        organization: &str,
        project: &str,
        test_case_id: i32,
        pbi_id: i32,
    ) -> Result<(), AdoError> {
        let pbi_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), pbi_id
        );
        let patch = serde_json::json!([{
            "op": "add",
            "path": "/relations/-",
            "value": {
                "rel": "Microsoft.VSTS.Common.TestedBy-Reverse",
                "url": pbi_url,
                "attributes": {"comment": "Linked by DevOps Test Case Manager"},
            },
        }]);
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), test_case_id
        );
        self.send_json_patch(reqwest::Method::PATCH, url, &patch).await?;
        Ok(())
    }

}

/// Is this relation "the test cases that test THIS work item"?
///
/// Exact match on the forward direction, because the two directions are
/// different links: `TestedBy-Forward` hangs test cases off a PBI, while
/// `TestedBy-Reverse` ("Tests") points the other way. A substring test for
/// "testedby" matched BOTH, so asking for the test cases of an id that was
/// itself a Test Case followed the reverse link and returned the PBI
/// rendered as a test case (2026-08 audit, R-3). Microsoft's own MCP
/// server compares the rel by equality for the same reason.
fn is_tested_by_forward(rel: &str) -> bool {
    rel.eq_ignore_ascii_case("Microsoft.VSTS.Common.TestedBy-Forward")
}

/// One test case's fate after a relink attempt - same shape as
/// `deletion::DeleteOutcome` but named for what actually happened: a
/// successful MOVE reported as `deleted: true` would be a lie in the one
/// report a worried user reads most carefully.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct RelinkOutcome {
    pub id: i32,
    pub moved: bool,
    /// Why not, when it was not. `None` on success. Structured, so the
    /// frontend's describeAdoError can lift Azure DevOps' own sentence.
    pub error: Option<super::AdoError>,
}

impl AdoClient {
    /// Move a Test Case's PBI link: drop the TestedBy-Reverse relation
    /// that points at `from_pbi` and add one pointing at `to_pbi`, in ONE
    /// PATCH - the case is never observable in a half-moved state. The
    /// requirement-based suites follow on their own, because they populate
    /// from exactly this link.
    ///
    /// The remove is BY INDEX, which Azure DevOps requires - so the
    /// relations are read first and the index found by matching the
    /// relation URL's trailing id. A case that carries no link to
    /// `from_pbi` is reported as such rather than silently linked to a
    /// second PBI: the caller thought it lived somewhere it does not, and
    /// that misunderstanding is worth surfacing before any write.
    pub async fn relink_test_case(
        &self,
        organization: &str,
        project: &str,
        test_case_id: i32,
        from_pbi: i32,
        to_pbi: i32,
    ) -> Result<(), AdoError> {
        let url = format!(
            "{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url, percent_encode_segment(organization), test_case_id
        );
        let data = self.get_json(url).await?;
        let relations = data["relations"].as_array().cloned().unwrap_or_default();
        let from_suffix_slash = format!("/{from_pbi}");
        let idx = relations.iter().position(|r| {
            r["rel"]
                .as_str()
                .map(|s| s.eq_ignore_ascii_case("Microsoft.VSTS.Common.TestedBy-Reverse"))
                .unwrap_or(false)
                && r["url"]
                    .as_str()
                    .map(|u| u.ends_with(&from_suffix_slash))
                    .unwrap_or(false)
        });
        let Some(idx) = idx else {
            return Err(AdoError::Http {
                status: 0,
                body: format!("test case #{test_case_id} carries no link to PBI #{from_pbi}"),
            });
        };

        let to_url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), to_pbi
        );
        // `test` on rev first: if the case changed between the read above
        // and this write, the whole PATCH is refused rather than removing
        // an index that now names a different relation.
        let patch = serde_json::json!([
            { "op": "test", "path": "/rev", "value": data["rev"] },
            { "op": "remove", "path": format!("/relations/{idx}") },
            { "op": "add", "path": "/relations/-", "value": {
                "rel": "Microsoft.VSTS.Common.TestedBy-Reverse",
                "url": to_url,
                "attributes": {"comment": "Relinked by DevOps Test Case Manager"},
            }},
        ]);
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), test_case_id
        );
        self.send_json_patch(reqwest::Method::PATCH, url, &patch).await?;
        Ok(())
    }

    /// All writable fields on the Test Case type, ported from v1
    /// get_test_case_fields: readOnly dropped, System.* dropped except
    /// Title/Tags/Description, sorted by display name. Read only.
    pub async fn get_test_case_fields(
        &self,
        organization: &str,
        project: &str,
    ) -> Result<Vec<FieldRef>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes/Test%20Case/fields?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project)
        );
        let data = self.get_json(url).await?;
        let mut fields: Vec<FieldRef> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|f| {
                let reference_name = f["referenceName"].as_str()?.to_string();
                if f["readOnly"].as_bool().unwrap_or(false) {
                    return None;
                }
                if reference_name.starts_with("System.")
                    && !matches!(
                        reference_name.as_str(),
                        "System.Title" | "System.Tags" | "System.Description"
                    )
                {
                    return None;
                }
                Some(FieldRef {
                    name: f["name"].as_str().unwrap_or(&reference_name).to_string(),
                    reference_name,
                })
            })
            .collect();
        fields.sort_by_key(|f| f.name.to_lowercase());
        Ok(fields)
    }

    /// The v1 Edit-tab field set (same batch endpoint as the summaries) plus
    /// the optional module/preconditions refs, with steps parsed and
    /// preconditions flattened for editing. Read only.
    pub async fn get_pbi_test_cases_full(
        &self,
        organization: &str,
        pbi_id: i32,
        module_ref: Option<&str>,
        preconditions_ref: Option<&str>,
    ) -> Result<Vec<TestCaseFull>, AdoError> {
        let url = format!(
            "{}/{}/_apis/wit/workitems/{}?$expand=relations&api-version=7.1",
            self.base_url, percent_encode_segment(organization), pbi_id
        );
        let data = self.get_json(url).await?;
        let tc_ids: Vec<i64> = data["relations"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|r| r["rel"].as_str().map(is_tested_by_forward).unwrap_or(false))
            .filter_map(|r| r["url"].as_str()?.rsplit('/').next()?.parse().ok())
            .collect();
        if tc_ids.is_empty() {
            return Ok(vec![]);
        }

        self.get_test_cases_by_ids(organization, &tc_ids_i32(&tc_ids), module_ref, preconditions_ref)
            .await
    }

    /// Full test cases for arbitrary work-item ids (chunked at 200) - the
    /// suite browser's Edit-cases handoff and browser views use this
    /// directly; get_pbi_test_cases_full delegates here. Read only.
    pub async fn get_test_cases_by_ids(
        &self,
        organization: &str,
        ids: &[i32],
        module_ref: Option<&str>,
        preconditions_ref: Option<&str>,
    ) -> Result<Vec<TestCaseFull>, AdoError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let mut field_list = vec![
            "System.Id",
            "System.Title",
            "System.Tags",
            "Microsoft.VSTS.TCM.AutomationStatus",
            "Microsoft.VSTS.TCM.Steps",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
        if let Some(m) = module_ref {
            field_list.push(m.to_string());
        }
        if let Some(p) = preconditions_ref {
            field_list.push(p.to_string());
        }

        let mut cases = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(Self::WORKITEM_BATCH_SIZE) {
            let ids_csv = chunk
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let url = format!(
                "{}/{}/_apis/wit/workitems?ids={}&fields={}&api-version=7.1",
                self.base_url,
                organization,
                ids_csv,
                field_list.join(",")
            );
            let fetched = self.get_json(url).await?;
            for w in fetched["value"].as_array().cloned().unwrap_or_default() {
                let f = &w["fields"];
                let str_of = |key: &str| f[key].as_str().unwrap_or_default().to_string();
                cases.push(TestCaseFull {
                    id: w["id"].as_i64().unwrap_or_default() as i32,
                    title: str_of("System.Title"),
                    tags: str_of("System.Tags"),
                    automation_status: {
                        let s = str_of("Microsoft.VSTS.TCM.AutomationStatus");
                        if s.is_empty() { "Not Automated".to_string() } else { s }
                    },
                    steps: crate::steps_xml::parse_steps_xml(&str_of("Microsoft.VSTS.TCM.Steps")),
                    step_ids: crate::steps_xml::parse_step_ids(&str_of("Microsoft.VSTS.TCM.Steps")),
                    steps_xml: str_of("Microsoft.VSTS.TCM.Steps"),
                    module_value: module_ref.map(str_of).unwrap_or_default(),
                    preconditions: preconditions_ref
                        .map(|p| crate::steps_xml::html_to_text(&str_of(p)))
                        .unwrap_or_default(),
                });
            }
        }
        Ok(cases)
    }

    /// Which type bugs are filed as on this project's process, ported from
    /// v1 detect_bug_type: prefer Bug (ReproSteps), fall back to Issue
    /// (Description). Enumeration failure assumes Bug. Read only.
    pub async fn detect_bug_type(
        &self,
        organization: &str,
        project: &str,
    ) -> Result<BugTypeInfo, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project)
        );
        let names: Vec<String> = match self.get_json(url).await {
            Ok(data) => data["value"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|wt| wt["name"].as_str().map(String::from))
                .collect(),
            Err(_) => vec![], // enumeration failure -> assume Bug (v1 behaviour)
        };
        let info = if names.iter().any(|n| n == "Bug") || names.is_empty() {
            BugTypeInfo {
                wi_type: "Bug".into(),
                repro_field: "Microsoft.VSTS.TCM.ReproSteps".into(),
                has_severity: true,
            }
        } else if names.iter().any(|n| n == "Issue") {
            BugTypeInfo {
                wi_type: "Issue".into(),
                repro_field: "System.Description".into(),
                has_severity: false,
            }
        } else {
            BugTypeInfo {
                wi_type: "Bug".into(),
                repro_field: "Microsoft.VSTS.TCM.ReproSteps".into(),
                has_severity: true,
            }
        };
        Ok(info)
    }

    /// POST a new work item of `wi_type` with fields and optional Related
    /// links. Returns (id, web_url). Only POST - never DELETEs.
    pub async fn create_work_item(
        &self,
        organization: &str,
        project: &str,
        wi_type: &str,
        fields: &[(String, String)],
        related_ids: &[i32],
        parent_id: Option<i32>,
    ) -> Result<(i32, String), AdoError> {
        let mut patch: Vec<serde_json::Value> = fields
            .iter()
            .map(|(r, v)| serde_json::json!({"op": "add", "path": format!("/fields/{r}"), "value": v}))
            .collect();
        for rel_id in related_ids {
            patch.push(serde_json::json!({
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": "System.LinkTypes.Related",
                    "url": format!("{}/{}/{}/_apis/wit/workitems/{}", self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), rel_id),
                },
            }));
        }
        // Hierarchy-Reverse = "my parent is" - lands the new item under its
        // PBI/Feature so boards and backlogs nest it correctly.
        if let Some(pid) = parent_id {
            patch.push(serde_json::json!({
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": "System.LinkTypes.Hierarchy-Reverse",
                    "url": format!("{}/{}/{}/_apis/wit/workitems/{}", self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), pid),
                },
            }));
        }
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/${}?api-version=7.1",
            self.base_url,
            organization,
            project,
            urlencoding::encode(wi_type)
        );
        let data = self
            .send_json_patch(reqwest::Method::POST, url, &serde_json::Value::Array(patch))
            .await?;
        let web = data["_links"]["html"]["href"].as_str().unwrap_or_default().to_string();
        Ok((data["id"].as_i64().unwrap_or_default() as i32, web))
    }

    /// Upload raw bytes as a work-item attachment; returns the attachment
    /// URL for an AttachedFile relation. POST only.
    pub async fn upload_wi_attachment(
        &self,
        organization: &str,
        project: &str,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<String, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/attachments?fileName={}&api-version=7.1",
            self.base_url,
            organization,
            project,
            urlencoding::encode(file_name)
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .header("Content-Type", "application/octet-stream")
            .body(bytes)
            .send()
            .await
            .map_err(|e| AdoError::Network(e.to_string()))?;
        let data = Self::handle_json(resp).await?;
        Ok(data["url"].as_str().unwrap_or_default().to_string())
    }

    /// PATCH an AttachedFile relation onto a work item (bug screenshots).
    pub async fn add_wi_attachment_relation(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
        attachment_url: &str,
    ) -> Result<(), AdoError> {
        let patch = serde_json::json!([{
            "op": "add",
            "path": "/relations/-",
            "value": {"rel": "AttachedFile", "url": attachment_url},
        }]);
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), wi_id
        );
        self.send_json_patch(reqwest::Method::PATCH, url, &patch).await?;
        Ok(())
    }

    /// All work-item tag names in the project, sorted (v1 get_tags, for the
    /// manual-entry autocomplete). Read only.
    pub async fn get_tags(&self, organization: &str, project: &str) -> Result<Vec<String>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/tags?api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project)
        );
        let data = self.get_json(url).await?;
        let mut tags: Vec<String> = data["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t["name"].as_str().map(String::from))
            .collect();
        tags.sort_by_key(|t| t.to_lowercase());
        Ok(tags)
    }

    /// The project's Area or Iteration tree flattened to path strings,
    /// ported from v1 get_classification_paths: built from node NAMES, not
    /// the node's `path` field (that carries an extra \Area or \Iteration
    /// segment the stored field values don't have). Failure -> empty so
    /// pickers fall back gracefully. Read only.
    pub async fn get_classification_paths(
        &self,
        organization: &str,
        project: &str,
        structure: &str,
    ) -> Result<Vec<String>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/classificationnodes/{}?$depth=14&api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), structure
        );
        let root = match self.get_json(url).await {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };
        fn walk(node: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
            let name = node["name"].as_str().unwrap_or_default();
            let path = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}\\{name}")
            };
            out.push(path.clone());
            for child in node["children"].as_array().cloned().unwrap_or_default() {
                walk(&child, &path, out);
            }
        }
        let mut paths = vec![];
        walk(&root, "", &mut paths);
        Ok(paths)
    }

    /// Iteration paths WITH their sprint dates (classificationnodes carries
    /// startDate/finishDate in `attributes`), so pickers can render like
    /// Azure DevOps's own iteration dropdown. Non-sprint nodes (the project
    /// root, grouping folders) have no dates. Failure -> empty. Read only.
    pub async fn get_iterations_dated(
        &self,
        organization: &str,
        project: &str,
    ) -> Result<Vec<crate::work_board::IterationRef>, AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/classificationnodes/iterations?$depth=14&api-version=7.1",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project)
        );
        let root = match self.get_json(url).await {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };
        fn walk(node: &serde_json::Value, prefix: &str, out: &mut Vec<crate::work_board::IterationRef>) {
            let name = node["name"].as_str().unwrap_or_default();
            let path = if prefix.is_empty() {
                name.to_string()
            } else {
                format!("{prefix}\\{name}")
            };
            out.push(crate::work_board::IterationRef {
                path: path.clone(),
                start_date: node["attributes"]["startDate"].as_str().map(str::to_string),
                finish_date: node["attributes"]["finishDate"].as_str().map(str::to_string),
            });
            for child in node["children"].as_array().cloned().unwrap_or_default() {
                walk(&child, &path, out);
            }
        }
        let mut out = vec![];
        walk(&root, "", &mut out);
        Ok(out)
    }

    /// A work item's area + iteration path (used to home the PBI's test
    /// plan). Read only.
    pub async fn get_work_item_paths(
        &self,
        organization: &str,
        project: &str,
        wi_id: i32,
    ) -> Result<(String, String), AdoError> {
        let url = format!(
            "{}/{}/{}/_apis/wit/workitems/{}?api-version=7.1&$select=System.AreaPath,System.IterationPath",
            self.base_url,
            percent_encode_segment(organization),
            percent_encode_segment(project), wi_id
        );
        let data = self.get_json(url).await?;
        Ok((
            data["fields"]["System.AreaPath"].as_str().unwrap_or_default().to_string(),
            data["fields"]["System.IterationPath"].as_str().unwrap_or_default().to_string(),
        ))
    }

    pub async fn get_projects(&self, organization: &str) -> Result<Vec<Project>, AdoError> {
        let url = format!(
            "{}/{}/_apis/projects?api-version=7.1&$top=500",
            self.base_url, percent_encode_segment(organization)
        );
        let body = self.get_json(url).await?;
        let projects = body["value"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
        Ok(projects)
    }
}

/// RFC 3986 percent-encoding for the wiki page `path` query value: keeps
/// `/` unescaped (wiki paths are slash-separated segments) plus ALPHA /
/// DIGIT / `-._~`; escapes everything else (notably spaces).
/// Percent-encode ONE path segment - an org or project name.
///
/// Unlike `percent_encode_path`, `/` is escaped: a project name is a
/// single segment, so a slash inside it must not read as a separator.
/// These were interpolated raw until the 2026-08 audit (R-6), while the
/// board screen encoded the same value - so a project named "50% Done"
/// (a percent sign is legal in ADO project names and is not restricted in
/// the UI) went out as an invalid escape sequence.
/// Normalise whatever the caller has into a wiki PAGE path.
///
/// Azure DevOps keeps two path namespaces for the same page and this
/// endpoint accepts only one. `WikiPage.path` is the page path
/// (`/Auth Flow`); `gitItemPath` is the backing file (`/Auth-Flow.md`) -
/// and wiki SEARCH results report the file form, which is what
/// `search_wiki` surfaces. Feeding a search hit straight back in therefore
/// asked for a page whose name ends in ".md" (2026-08 audit, R-2).
///
/// Two conversions, both harmless on an already-correct page path:
///  - drop a trailing `.md`;
///  - add the leading `/` this parameter's own samples always carry.
///
/// What this deliberately does NOT do is turn hyphens back into spaces.
/// ADO writes spaces as hyphens in the file name, so "Auth-Flow.md" could
/// be the page "Auth Flow" OR one genuinely named "Auth-Flow" - the
/// mapping is not reversible, and guessing would break every page whose
/// title really contains a hyphen. That residue fails the way it does
/// today (a 404 the caller can act on), never by silently fetching a
/// different page.
fn wiki_page_path(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_ext = trimmed.strip_suffix(".md").unwrap_or(trimmed);
    if without_ext.is_empty() {
        return "/".to_string();
    }
    if without_ext.starts_with('/') {
        without_ext.to_string()
    } else {
        format!("/{without_ext}")
    }
}

fn percent_encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The Steps XML to write, or `None` to leave the field out of the patch.
///
/// The problem this solves: `steps` on a loaded case is a LOSSY read.
/// `parse_steps_xml` strips every tag, so a step whose action is
/// `<b>Click Save</b><img src="...screenshot...">` comes back as the bare
/// text `Click Save`. Rebuilding the field from that and PATCHing it - which
/// is what every save used to do, unconditionally - deleted the formatting
/// and the screenshot from Azure DevOps. Editing only the TITLE was enough
/// to do it, and the save reported success.
///
/// The comparison is deliberately PARSED against PARSED. A case the user did
/// not touch parses to exactly what it parsed to when it was loaded, so it
/// compares equal and the field is omitted - the original XML stays in ADO,
/// markup and all. Only a real edit to a step differs, and then the loss of
/// markup in that one step is unavoidable: the editor is plain text, and the
/// user is deliberately replacing what was there.
///
/// Without a baseline (an imported update, where the file genuinely supplies
/// the steps) the field is written as before.
/// The PATCH op(s) that write System.Tags on an EXISTING work item.
///
/// Azure DevOps treats the `add` op on this ONE field as a merge: the
/// listed tags are appended and omitted ones stay put, so an update that
/// dropped a tag silently kept it - both from an import file and from the
/// editor's clear. Removal needs `replace`, but JSON Patch only replaces
/// a path that exists, so the right op depends on what the work item
/// currently holds:
/// - had tags: one `replace` sets the exact final list (`""` clears);
///   skipped entirely when nothing changed.
/// - had none: there is nothing to remove - a plain `add` creates the
///   field, and an empty desired list needs no op at all.
/// - unknown (the caller could not read the current value): `add` then
///   `replace` in the same document - the add guarantees the path exists,
///   the replace makes the value exact. Only for a non-empty desired
///   list: a blind clear could fail the WHOLE patch on a tagless item,
///   and losing the title/steps update over tags that may not even exist
///   is the worse trade.
pub fn tags_write_ops(desired: &str, original: Option<&str>) -> Vec<serde_json::Value> {
    let desired = desired.trim();
    let op = |kind: &str| serde_json::json!({"op": kind, "path": "/fields/System.Tags", "value": desired});
    match original.map(str::trim) {
        Some(orig) if orig == desired => vec![],
        Some(orig) if !orig.is_empty() => vec![op("replace")],
        Some(_) if desired.is_empty() => vec![],
        Some(_) => vec![op("add")],
        None if desired.is_empty() => vec![],
        None => vec![op("add"), op("replace")],
    }
}

fn steps_patch(steps: &[crate::steps_xml::Step], original_xml: Option<&str>) -> Option<String> {
    // build_steps_xml(&[]) emits a single blank placeholder step, so writing
    // it would replace a real step list with one empty row. Nothing upstream
    // should send an empty list - is_valid rejects it - but the cost of
    // being wrong here is unrecoverable, so refuse rather than wipe.
    if steps.is_empty() {
        crate::applog::warn("refused to write an empty step list over an existing test case");
        return None;
    }
    if let Some(xml) = original_xml {
        if crate::steps_xml::parse_steps_xml(xml) == steps {
            return None;
        }
    }
    Some(crate::steps_xml::build_steps_xml(steps))
}

/// Text going into an HTML field value.
///
/// Preconditions are typed as plain text and were interpolated straight
/// into `<div>...</div>`, so a `<` or an `&` - "value < 10", "Tom & Jerry"
/// - reached Azure DevOps as broken markup and came back mangled or
/// truncated. Escaping is what makes the round-trip faithful.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
