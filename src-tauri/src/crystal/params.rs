//! The open report's parameters: what a person or an assistant can give a
//! preview, and the saved-data preview that takes those values.
//!
//! A value is used for the one drawing it is given to. It is never written
//! into the report, so the file and the open report are as they were.

use super::live::check_param_names;
use super::{file_name, state, Open, PreviewInfo, NOTHING_OPEN};

/// One pick-list or saved value of a parameter.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct ParameterChoice {
    pub value: String,
    pub description: Option<String>,
}

/// One parameter of the open report, shaped for a prompt.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ReportParameter {
    pub name: String,
    /// Number, String, Currency, Boolean, Date, Time or DateTime.
    pub value_kind: String,
    pub prompt_text: Option<String>,
    /// The prompt may be left out.
    pub optional_prompt: bool,
    pub allow_multiple_values: bool,
    /// An empty value is allowed.
    pub allow_null_value: bool,
    /// The report's authored pick list.
    pub choices: Vec<ParameterChoice>,
    /// The value the report was saved with, if any.
    pub saved: Vec<ParameterChoice>,
}

/// Said at the head of a saved-data preview of a report that holds no data.
pub const NO_SAVED_DATA: &str =
    "This report has no saved data, so only its static parts are drawn. A live preview draws fresh rows.";

fn choices(values: Vec<rpt_core::ParameterValueInfo>) -> Vec<ParameterChoice> {
    values.into_iter().map(|v| ParameterChoice { value: v.value, description: v.description }).collect()
}

/// The open report's parameters, in the order the report declares them.
pub fn parameters() -> Result<Vec<ReportParameter>, String> {
    let slot = state();
    let open = slot.as_ref().ok_or(NOTHING_OPEN)?;
    Ok(rpt_core::report_parameters(open.rpt.report())
        .into_iter()
        .map(|p| ReportParameter {
            name: p.name,
            value_kind: p.value_kind,
            prompt_text: p.prompt_text,
            optional_prompt: p.optional_prompt,
            allow_multiple_values: p.allow_multiple_values,
            allow_null_value: p.allow_null_value,
            choices: choices(p.default_values),
            saved: choices(p.current_values),
        })
        .collect())
}

/// Draw `open` from its saved data with `params` as the parameter values and
/// keep the result. The values are checked first: a refusal draws nothing
/// and leaves the stored preview as it was.
fn render_with_values(open: &mut Open, params: Vec<(String, String)>) -> Result<PreviewInfo, String> {
    let name = file_name(&open.path);
    let refused = |why: String| format!("{name} cannot be previewed with these values: {why}.");
    check_param_names(open.rpt.report(), &params).map_err(refused)?;
    let typed = rpt_core::coerce_params(open.rpt.report(), &params).map_err(refused)?;

    let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let doc = rpt_render::render_with(
            open.rpt.report(),
            rpt_render::RenderOptions { params: typed, ..Default::default() },
        );
        let rendered: Vec<String> = doc.diagnostics.iter().map(|d| d.describe()).collect();
        let pdf = rpt_render::try_render_document(&doc, &rpt_render::PdfOptions::default())
            .map_err(|e| format!("PDF export failed: {e}"))?;
        Ok::<_, String>((doc.pages.len(), rendered, pdf))
    }))
    .unwrap_or_else(|_| Err("the renderer stopped unexpectedly".to_string()));

    match drawn {
        Ok((pages, rendered, pdf)) => {
            let mut diagnostics = Vec::new();
            if open.rpt.report().has_saved_data {
                diagnostics = rendered;
            } else {
                diagnostics.push(NO_SAVED_DATA.to_string());
                diagnostics.extend(rendered.into_iter().filter(|d| !d.starts_with("formula error: unknown name")));
            }
            let info = PreviewInfo {
                path: open.path.clone(),
                page_count: u32::try_from(pages).unwrap_or(u32::MAX),
                diagnostics,
                live: None,
            };
            open.preview = Some((info.clone(), pdf));
            Ok(info)
        }
        Err(e) => {
            let why = format!("The preview of {name} could not be drawn: {e}.");
            let info =
                PreviewInfo { path: open.path.clone(), page_count: 0, diagnostics: vec![why.clone()], live: None };
            open.preview = Some((info, Vec::new()));
            Err(why)
        }
    }
}

/// Render the open report from its saved data with `params` (name, value
/// text) as the parameter values, and keep the PDF for the screen. Nothing
/// is saved. With no values this is the plain saved-data preview.
pub fn preview_saved_with(params: Vec<(String, String)>) -> Result<PreviewInfo, String> {
    let mut slot = state();
    let open = slot.as_mut().ok_or(NOTHING_OPEN)?;
    if params.is_empty() {
        super::render(open)
    } else {
        render_with_values(open, params)
    }
}
