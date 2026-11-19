# Tool list without a restart - design

Date: 2026-11-19. Owner asked for it ("Sure, do 1"). Status: approved by the owner 19 November 2026.

## Why

After a TCM update or a module switch, Claude Code keeps showing the old tools until the person restarts it (or reconnects TCM in `/mcp`). The cause: the `--mcp` connector process that Claude Code starts is a copy of the TCM exe that keeps running across updates, and it holds the tool list **and** the per-tool call handling itself (`src-tauri/src/mcp.rs`: `tools_list`, `tools_call_outcome` and its ~40 helpers, `coerce_args`, the role-notes session). An old connector serves the old list and cannot route a tool added later ("Unknown tool").

Goal: after one more restart (the first update that ships this), tool changes from an update or a module switch reach the assistant with no restart and no reconnect.

## Goals / non-goals

- Goal: the running app answers `tools/list` and `tools/call`; the connector only relays.
- Goal: the connector tells Claude Code `tools.listChanged: true` and sends `notifications/tools/list_changed` when the app's list changes.
- Non-goal: a plugin, an HTTP MCP endpoint, renamed tools, or changes to the slash commands or agent files (they are already user-level files Claude Code reads fresh).
- Anti-goal: tools disappearing while TCM is closed. Today a closed app still lists the tools of the saved modules (they answer "open TCM"); that stays.

## Approaches considered

1. **Thin relay (chosen).** The app gains one bridge route, `POST /mcp`, that takes a JSON-RPC message plus a session id and answers it with today's `mcp.rs` code, run inside the app. The connector forwards each line and writes the answer back. Same exe, same code, so nothing is duplicated - only *where* it runs moves.
2. Move only the list into the app (what the earlier suggestion said). Rejected: an old connector still routes calls with old code, so a new or changed tool still needs a restart.
3. HTTP MCP endpoint served by the app, no connector. Rejected: needs a local token in every client's config, every client set up again, tools vanish while TCM is closed.

## How it works

- **App side:** `POST /mcp` with `{ "session": "<id>", "message": <json-rpc> }`. It runs `mcp::handle_message_shared` on a blocking thread with the in-app bridge as the `BridgeCall`, keeping one role-notes book per session id (dropped after the existing idle reset). It answers `{ "reply": <json-rpc or null> }`. It also offers `GET /mcp/list-version`: a short hash of what `tools/list` would answer now.
- **Connector side:** on start it makes a random session id. `initialize` answers locally with `capabilities.tools.listChanged = true` (so it works while the app is closed). Every other request goes to `POST /mcp`; if the app cannot be reached, or is an older app without the route (404), it falls back to today's local handling, so nothing is lost.
- **Change notice:** a background thread asks `GET /mcp/list-version` every 5 s (only after `initialize`); when the hash differs from the one last listed, it writes `{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}` to stdout under the same lock the replies use. An app that was closed and opens again counts as a change.
- **Version warning:** the existing "connector older than the app" instruction is dropped once the connector relays, because version gaps no longer matter; kept for the fallback path.

## Constraints

- The token never crosses into the webview; the route is behind the bridge's existing token like every other route. No new port, no new secret.
- No HTTP DELETE; user-facing text names no URL; never log arguments that may hold secrets (the route logs the method and tool name only).
- Same tool names (`mcp__tcm__...`), so permission rules and docs are untouched.
- Rust tests in `src-tauri/tests/suite/`: relay answers equal the local answers for a sample of tools; fallback when the route 404s or the app is down; the notice is written once per change and not while nothing changed; the session id keeps role notes apart.

## Left to the builder

The poll interval within 2-10 s, the hash input (the serialized list is fine), and how the in-app `BridgeCall` reaches the routes (direct call or loopback HTTP to its own port).

## What the person sees

Nothing new. After the first restart following this release, switching a module on or off, or updating TCM, changes the assistant's tools within a few seconds. Needs Claude Code to support `list_changed` (current versions do); a client that ignores it behaves as today.
