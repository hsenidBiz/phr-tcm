//! Process-layout discovery: the extra form pages (ADO tabs) a type
//! carries beyond the main Details page, e.g. a Bug's RCA and Preventive
//! Measures, resolved from the org-level processes API.

use super::{ExtraField, ExtraPage};
use crate::ado::AdoClient;

impl AdoClient {
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
