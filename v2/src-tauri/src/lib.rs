//! Crate root: module tree, the specta builder (typed IPC + events), and
//! the Tauri app wiring. All command handlers live under `commands/`,
//! grouped by domain; domain logic lives in the modules they call into.

pub mod ado;
pub mod ado_git;
pub mod ado_testplan;
pub mod audio;
pub mod auth;
pub mod capture;
pub mod commands;
pub mod events;
pub mod import_parser;
pub mod model;
pub mod note_server;
pub mod report;
pub mod state;
pub mod steps_xml;
pub mod updater;
pub mod work_board;

use std::sync::Mutex;
use tauri_specta::{collect_commands, collect_events, Builder};

// Pre-reorg public paths, kept resolving for tests and any external callers.
pub use commands::auth::AuthStatus;
pub use commands::bugs::{work_bug, RunAttachmentOut};
pub use commands::queue::{ImportResult, SubmitItemResult};
pub use commands::runs::{PointOutcome, ResultFailureDetail, RunAttachment};
pub use events::{CaseNoteSaved, SubmitProgress, SuiteScanProgress};
pub use state::SubmitCancel;

pub fn specta_builder() -> Builder<tauri::Wry> {
    use commands::{auth, board, bugs, cases, discovery, misc, prs, queue, runs, testplan};
    Builder::<tauri::Wry>::new()
        .events(collect_events![
            events::SubmitProgress,
            events::SuiteScanProgress,
            audio::AudioSpectrum,
            events::CaseNoteSaved,
            events::PlanCreated
        ])
        .commands(collect_commands![
            misc::ping,
            auth::auth_status,
            auth::sign_in,
            discovery::list_projects,
            discovery::list_orgs,
            discovery::search_pbis,
            cases::pbi_test_cases,
            queue::parse_import_file,
            queue::export_queue,
            queue::write_template,
            queue::submit_queue,
            testplan::ensure_pbi_suite,
            runs::list_test_points,
            runs::result_failure_detail,
            runs::submit_test_run,
            board::fetch_board,
            board::move_board_item,
            misc::audio_capture_start,
            misc::audio_capture_stop,
            misc::check_update,
            misc::apply_update,
            cases::list_test_case_fields,
            cases::pbi_test_cases_full,
            cases::update_test_case,
            queue::export_queue_json,
            testplan::list_plans_with_suites,
            runs::get_result_detail,
            bugs::capture_screens,
            bugs::file_bug,
            board::list_teams,
            board::list_team_members,
            board::work_item_detail,
            board::update_work_item,
            board::activity_values,
            board::work_item_comments,
            board::add_comment,
            board::avatar_b64,
            board::quick_create_item,
            discovery::classification_paths,
            queue::cancel_submit,
            queue::export_queue_html,
            discovery::list_project_tags,
            runs::result_screenshots,
            queue::view_queue_html,
            cases::test_cases_by_ids,
            cases::test_case_field_values,
            testplan::find_pbi_suite,
            runs::run_history,
            runs::view_execution_report,
            bugs::read_file_b64,
            bugs::open_snip,
            prs::list_repos,
            prs::pr_overview,
            prs::repo_pull_requests,
            prs::board_pr_links,
            prs::pr_work_items
        ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Mutex::new(auth::AuthState::default()))
        .manage(updater::UpdateState::default())
        .manage(SubmitCancel::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            // Registers the typed-event registry in Tauri state; without
            // this every specta Event::emit panics with "EventRegistry not
            // found in Tauri state".
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
