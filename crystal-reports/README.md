# Out — Crystal Reports editor building blocks

Read `.rpt` files into a readable JSON IR, edit the v1 scope (text, position/size,
fonts & colors, section & page setup), and save valid `.rpt` files — a
re-implementation, in pure Rust, of what Crystal Reports itself does with these files (wire
behaviors verified against the engine's own tables and limits; no Crystal code is used)
runtime, built for a Tauri + React + Tailwind V4 app.

```
Out/
├── Cargo.toml          # workspace: vendor/* + rpt-core + rpt-cli
├── vendor/             # rpt-rs (MPL-2.0, pinned commit) — see vendor/VENDORED.md
├── rpt-core/           # the library: IR projection, record locator, typed edit ops
├── rpt-cli/            # rpt-tools: ir / edit / diff / roundtrip / sweep
├── tauri-commands/     # drop-in #[tauri::command] module (commands.rs + README)
├── ui-react/           # React+TS+Tailwind4 editor components (types + api + 4 components)
├── docs/               # format-notes · ir-schema · integration-guide · validation-log
└── work/               # validation artifacts (IR dumps, sweep log, edited samples)
```

Start with **`docs/integration-guide.md`**. Quick taste, from `Out/`:

```
cargo build --release
target\release\rpt-tools.exe ir   "..\Sample Report\PMSV10_GoalAlignCascade.rpt" sample.ir.json
target\release\rpt-tools.exe edit "..\Sample Report\PMSV10_GoalAlignCascade.rpt" ops.json out.rpt
```

with `ops.json` e.g.

```json
[{"op": "set_text", "object": {"name": "FtConf"}, "text": "Edited!"}]
```

Validated: 6/6 samples IR with zero warnings; 263/263 corpus round-trip byte-identical; all 29
Vibey ops verified by model diff and pinned as `cargo test -p rpt-core` (`docs/validation-log.md`).
The bundle verifies itself anywhere: `powershell Out/tools/verify-portable.ps1` copies it to a
temp directory and runs the suite + consumer simulation from the copy.

## Adding this to your app

1. Keep this `Out/` folder anywhere your build can reach it (it is self-contained; the
   vendored fonts and embedded specimens compile in via `include_bytes!`).
2. From your `src-tauri/Cargo.toml`:
   ```toml
   [dependencies]
   rpt-core = { path = "../Out/rpt-core" }
   tauri-commands = { path = "../Out/tauri-commands", features = ["tauri"] }
   ```
   The workspace tables this bundle's root `Cargo.toml` carries (package/deps/lints) must
   come along — either reference the crates from within this workspace or copy those tables.
3. Register the commands (see `tauri-commands/README.md` for the `generate_handler!` list).
4. Copy `ui-react/src/` into your front end for the components + typed API.
5. Full wiring walkthrough: `docs/integration-guide.md`. CLI scripting: build `rpt-cli`
   (`cargo build -p rpt-cli`) for `rpt-tools`.

To verify the bundle anywhere: `cargo test --workspace --exclude rpt-reader` and
`tools/verify-portable.ps1`.
