//! Process-layout discovery: the extra form pages (ADO tabs) a type
//! carries beyond the main Details page, e.g. a Bug's RCA and Preventive
//! Measures, resolved from the org-level processes API.

use super::{ExtraField, ExtraPage, BUG_AUTO_FILLED};
use crate::ado::{AdoClient, AdoError};

impl AdoClient {
    /// The fields a create of `wi_type` must be given: everything the type
    /// marks always-required that ADO does not fill itself (no default, not
    /// in `BUG_AUTO_FILLED`, not the repro field), plus every ref in
    /// `extra_refs` (fields a rule rejection named, shown even when the type
    /// does not mark them required). In the type's own order, extras last.
    /// A field whose own definition cannot be read is still offered, as text.
    pub async fn bug_field_definitions(
        &self,
        org: &str,
        project: &str,
        wi_type: &str,
        repro_field: &str,
        extra_refs: &[String],
    ) -> Result<Vec<ExtraField>, AdoError> {
        use crate::ado::endpoints::percent_encode_segment;
        use crate::ado::rule_errors::is_plain_ref;
        let (o, p) = (percent_encode_segment(org), percent_encode_segment(project));
        let url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes/{}/fields?$expand=all&api-version=7.1",
            self.base_url,
            o,
            p,
            urlencoding::encode(wi_type)
        );
        let listing = self.get_json(url).await?;
        let skip = |r: &str| r == repro_field || BUG_AUTO_FILLED.contains(&r) || !is_plain_ref(r);

        // (reference name, name, allowed values from the type's own entry)
        let mut wanted: Vec<(String, String, Vec<String>)> = Vec::new();
        let push = |r: &str, name: &str, allowed: Vec<String>, wanted: &mut Vec<_>| {
            if !skip(r) && !wanted.iter().any(|(w, _, _): &(String, String, Vec<String>)| w == r) {
                wanted.push((r.to_string(), name.to_string(), allowed));
            }
        };
        let allowed_of = |f: &serde_json::Value| -> Vec<String> {
            f["allowedValues"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        };
        let has_default = |f: &serde_json::Value| match &f["defaultValue"] {
            serde_json::Value::Null => false,
            serde_json::Value::String(s) => !s.is_empty(),
            _ => true,
        };
        for f in listing["value"].as_array().into_iter().flatten() {
            let Some(r) = f["referenceName"].as_str() else { continue };
            let extra = extra_refs.iter().any(|e| e == r);
            if extra || (f["alwaysRequired"].as_bool() == Some(true) && !has_default(f)) {
                push(r, f["name"].as_str().unwrap_or(r), allowed_of(f), &mut wanted);
            }
        }
        for r in extra_refs {
            push(r, r, Vec::new(), &mut wanted);
        }

        let urls = wanted
            .iter()
            .map(|(r, _, _)| format!("{}/{}/{}/_apis/wit/fields/{}?api-version=7.1", self.base_url, o, p, urlencoding::encode(r)))
            .collect();
        let defs = self.get_json_each(urls).await;

        let mut out = Vec::new();
        for ((reference_name, listed_name, mut allowed), def) in wanted.into_iter().zip(defs) {
            let def = def.unwrap_or(serde_json::Value::Null);
            let label = if listed_name != reference_name {
                listed_name
            } else {
                def["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(&reference_name).to_string()
            };
            let ftype = def["type"].as_str().unwrap_or_default().to_ascii_lowercase();
            if allowed.is_empty() && ftype.starts_with("picklist") {
                allowed = self
                    .get_field_allowed_values(org, project, wi_type, &reference_name)
                    .await
                    .unwrap_or_default();
            }
            let kind = if !allowed.is_empty() {
                "pick"
            } else if ftype == "datetime" {
                "date"
            } else if ftype == "html" {
                "html"
            } else {
                "text"
            };
            out.push(ExtraField {
                label,
                reference_name,
                section: 0,
                kind: kind.to_string(),
                allowed,
                value: String::new(),
            });
        }
        Ok(out)
    }

    /// The type's custom form pages beyond the first (main) one, with every
    /// visible field control they contain - the same tabs ADO's own form
    /// shows (Bug: RCA, Preventive Measures). Fields are included even when
    /// empty so the drawer can fill them in.
    ///
    /// The layout lives in the org-level PROCESSES api (there is no
    /// project-scoped layout endpoint), so this chains: project capabilities
    /// (process id) -> work item type (reference name) -> process layout.
    /// Every hop reports its failure so the drawer can show why.
    pub(super) async fn extra_pages_for(
        &self,
        org: &str,
        project: &str,
        wi_type: &str,
        item_fields: &serde_json::Value,
    ) -> Result<Vec<ExtraPage>, String> {
        // Capabilities work with the project NAME (the properties api wants
        // a GUID) and carry the current process template id.
        let proj_url = format!(
            "{}/{}/_apis/projects/{}?includeCapabilities=true&api-version=7.1",
            self.base_url,
            org,
            urlencoding::encode(project)
        );
        let proj = self
            .get_json(proj_url)
            .await
            .map_err(|e| format!("project lookup failed: {e}"))?;
        let process_id = proj["capabilities"]["processTemplate"]["templateTypeId"]
            .as_str()
            .ok_or("project capabilities carried no process template id")?
            .to_string();

        let wit_url = format!(
            "{}/{}/{}/_apis/wit/workitemtypes/{}?api-version=7.1",
            self.base_url,
            org,
            project,
            urlencoding::encode(wi_type)
        );
        let wit = self
            .get_json(wit_url)
            .await
            .map_err(|e| format!("work item type lookup failed: {e}"))?;
        let Some(wit_ref) = wit["referenceName"].as_str().map(String::from) else {
            return Err("work item type carried no referenceName".into());
        };

        let layout_url = format!(
            "{}/{}/_apis/work/processes/{}/workItemTypes/{}/layout?api-version=7.1-preview.1",
            self.base_url,
            org,
            process_id,
            urlencoding::encode(&wit_ref)
        );
        let layout = self
            .get_json(layout_url)
            .await
            .map_err(|e| format!("process layout failed: {e}"))?;
        let Some(pages) = layout["pages"].as_array() else {
            return Err("process layout carried no pages".into());
        };

        let mut out = Vec::new();
        // The layout lists the main Details page first plus system pages
        // (history/links/attachments). The extra tabs are every "custom"
        // page after that first one.
        let mut seen_main = false;
        for page in pages {
            if page["pageType"].as_str() != Some("custom")
                || page["visible"].as_bool() == Some(false)
            {
                continue;
            }
            if !seen_main {
                seen_main = true; // the drawer already covers the main form
                continue;
            }
            let name = page["label"].as_str().unwrap_or_default().to_string();
            let mut fields = Vec::new();
            let mut section_no: u32 = 0;
            for section in page["sections"].as_array().into_iter().flatten() {
                let mut section_used = false;
                for group in section["groups"].as_array().into_iter().flatten() {
                    let group_label = group["label"].as_str().unwrap_or_default();
                    for control in group["controls"].as_array().into_iter().flatten() {
                        if let Some(mut f) = self
                            .extra_field_from_control(
                                org,
                                project,
                                wi_type,
                                control,
                                group_label,
                                item_fields,
                            )
                            .await
                        {
                            f.section = section_no;
                            section_used = true;
                            fields.push(f);
                        }
                    }
                }
                // Only sections that contributed fields advance the column
                // count (ADO pads layouts with empty sections).
                if section_used {
                    section_no += 1;
                }
            }
            if !name.is_empty() && !fields.is_empty() {
                out.push(ExtraPage { name, fields });
            }
        }
        Ok(out)
    }

    /// Map one layout control to an editable field (None for non-field
    /// controls: links, attachments, extensions, history, hidden).
    async fn extra_field_from_control(
        &self,
        org: &str,
        project: &str,
        wi_type: &str,
        control: &serde_json::Value,
        group_label: &str,
        item_fields: &serde_json::Value,
    ) -> Option<ExtraField> {
        if control["visible"].as_bool() == Some(false)
            || control["isContribution"].as_bool() == Some(true)
        {
            return None;
        }
        let control_type = control["controlType"].as_str().unwrap_or_default();
        let reference_name = control["id"].as_str().unwrap_or_default().to_string();
        if reference_name.is_empty() || !reference_name.contains('.') || reference_name == "System.History" {
            return None;
        }
        let kind = match control_type {
            "HtmlFieldControl" => "html".to_string(),
            "FieldControl" | "DateTimeControl" => "text".to_string(),
            _ => return None, // LinksControl, AttachmentsControl, extensions...
        };
        // Rich-text controls usually sit in their own group whose label is
        // what ADO's form displays; the control label is the fallback.
        let label = [control["label"].as_str().unwrap_or_default(), group_label]
            .into_iter()
            .find(|l| !l.is_empty())
            .unwrap_or(reference_name.as_str())
            .to_string();
        // Plain field controls may be picklists - ask the field definition.
        let (kind, allowed) = if kind == "text" && control_type == "FieldControl" {
            let url = format!(
                "{}/{}/{}/_apis/wit/workitemtypes/{}/fields/{}?$expand=allowedValues&api-version=7.1",
                self.base_url,
                org,
                project,
                urlencoding::encode(wi_type),
                reference_name
            );
            let allowed: Vec<String> = match self.get_json(url).await {
                Ok(def) => def["allowedValues"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                Err(_) => Vec::new(),
            };
            if allowed.is_empty() {
                ("text".to_string(), Vec::new())
            } else {
                ("pick".to_string(), allowed)
            }
        } else {
            (kind, Vec::new())
        };
        let value = match &item_fields[&reference_name] {
            serde_json::Value::Null => String::new(),
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        Some(ExtraField { label, reference_name, section: 0, kind, allowed, value })
    }
}
