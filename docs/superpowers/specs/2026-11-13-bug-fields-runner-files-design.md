# File bug required fields + Test Runner file handling - design

Approved by the owner 2026-11-13 ("That looks correct, continue with the implementation").

## Why
1. **File bug fails** in the owner's org: `POST .../workitems/$Bug -> 400 TF401320 Rule Error for field Bug Type
   (Custom.BugType) ... 6 additional errors`. `commands/bugs.rs::file_bug` sends only System.Title, the repro field
   and links; the org's Bug type requires custom fields (and possibly conditional rules).
2. **Test Runner** (`src/screens/RunnerWindow.tsx`): files attached to a case cannot be downloaded; "Previously
   uploaded" shows only the last result's screenshots (images); Attach file takes one file; no drag and drop.

## Goals
### A. Bug required fields (BugDialog, `src/components/BugDialog.tsx`, used from the runner)
- When the dialog opens, ask Azure DevOps (GET only) which fields the detected bug work item type requires
  (`_apis/wit/workitemtypes/{type}/fields?$expand=all` -> `alwaysRequired`, plus `allowedValues`), and show each
  required field the app does not fill itself (not System.Title, the repro field, System.AreaPath/IterationPath/
  State/Reason/CreatedBy/etc. that ADO defaults - the builder decides the exact skip list from what ADO fills on
  create). A field with allowed values is a dropdown; a DateTime field a date input; otherwise text.
- File is disabled until every shown required field has a value.
- Last value per field is remembered per org+project (webview cache via `src/lib/cache.ts` conventions or the
  existing settings pattern - builder picks the project's one cache implementation; values are not secrets).
- `file_bug` takes the extra field values (reference name -> value) and sends them in the create PATCH.
- If create still fails with TF401320 / RuleValidationErrors, nothing is created; the command returns a typed
  error naming the offending field reference names (and ADO's per-field message), and the dialog shows those
  fields (fetching their definitions if not already shown) highlighted with the message, so the person fills them
  and files again. Error text shown to the user names no URL.
- Reuse what exists: `work_board/layout.rs` already reads field definitions + allowedValues (ExtraField).
### B. Test Runner files
- Every file attached to the current case this session (images, .webm recordings, any file) gets a Download
  control; the person picks the destination with a save dialog; bytes are written by Rust.
- "Previously uploaded" becomes the full list of the last result's attachments (name, size, kind), videos
  included, each with Download; images keep their thumbnails + lightbox. Download streams from Azure DevOps (GET)
  straight to the chosen file in Rust - the bytes never cross into the webview.
- Attach file accepts several files at once (`open({ multiple: true })`), each read and attached; one failure does
  not stop the others (report which failed).
- Dropping files anywhere on the runner window attaches them to the current case (Tauri drag-drop event gives
  paths; read via the existing `read_file_b64`). While dragging over the window, a visible drop hint shows. Dropping
  with no current case does nothing (toast).

## Non-goals / constraints
- No DELETE calls to Azure DevOps (safety invariant). GET/POST/PATCH only.
- Tokens stay in Rust. User-facing errors name no URL (`ado/transport.rs` sentences for transport errors).
- No new settings page for bug defaults (owner preferred remembered values over Settings defaults).
- `src/bindings.ts` is generated; Rust tests only in `src-tauri/tests/suite/`.
- Theming via tokens only; icons from `src/lib/actionIcons.ts` (add a Download icon there if missing, named for
  what it does); `src/ui-consistency.test.ts` must pass unweakened.
- Guide: update the Test Runner / bug wording in `docs-site/src/content/*` and rebuild `src-tauri/help`; no new
  screenshots unless a documented accessible name changes (then note it as owed).

## Left to the builder
Exact skip list of auto-filled fields; the date input format ADO accepts; file size display; where the remembered
values live (must follow the one-cache rule); drag-drop hint visuals within the theme.
