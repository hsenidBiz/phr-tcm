# Azure DevOps Test Case Manager

A Windows desktop app for creating, editing, importing and **executing** Azure
DevOps test cases against a Product Backlog Item — without leaving a single
window. Built with **Tauri 2 + Rust + React/TypeScript**; signs in with your own
Microsoft account; ships and updates itself via Velopack.

It lives in **[`v2/`](v2/)**.

### 👉 Start with [`v2/README.md`](v2/README.md)

That is the real documentation: features, security model, authentication,
architecture, and the develop / build / release workflow.

Updates are read from the public
[releases repo](https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases).
The app checks, shows a **Restart to update** banner, and applies the
update only when you click it.

---

## The old app (V1)

The original PyQt5 application is **frozen on the [`v1`](../../tree/v1) branch**
and is no longer developed. It is not on `master` and takes no further updates.
Everything about it — its code, tests, build and its own `CLAUDE.md` — is on
that branch, unchanged.

V1 installs update themselves through Velopack from their own
[releases repo](https://github.com/AvinAlwis/azure-devops-test-case-manager-releases),
not from this repository, so they are unaffected by the split.
