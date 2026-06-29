"""Microsoft Entra ID sign-in for Azure DevOps via MSAL.

Signs in through the well-known Azure CLI public client, so no app
registration is required in the tenant. Sign-in uses the system browser.
(On Windows it can also use the WAM broker — a native one-click account
picker — but that's currently disabled via ``_BROKER_ENABLED`` because it
needs device registration that some orgs block.) The MSAL token cache is
in-memory only (matching TokenManager's never-persist-to-disk policy), so
sign-in is needed once per launch, after which access tokens refresh silently.
"""

import threading

# Well-known public client id for Azure CLI — pre-consented for the
# Azure DevOps resource in Entra ID tenants, so no app registration is needed.
_AZURE_CLI_CLIENT_ID = "04b07795-8ddb-461a-bbee-02f9e1bf7b46"

# Azure DevOps service principal id; "/.default" requests its delegated scopes.
_ADO_SCOPE = "499b84ac-1321-427f-aa17-267ca6975798/.default"

_AUTHORITY = "https://login.microsoftonline.com/organizations"

# TEMPORARILY DISABLED: the WAM broker needs device registration, which some
# orgs block — so sign-in defaults to the system browser. Flip to True to
# re-enable the native one-click broker picker.
_BROKER_ENABLED = False


# Branded pages MSAL serves in the browser after the redirect (system-browser
# fallback only — the broker uses a native dialog). The token is already
# captured by MSAL's local redirect server before either page renders, so a
# template problem can never block sign-in. We don't try to auto-close the tab
# (browsers refuse to let a page close a tab they opened); the page just tells
# the user they can close it. MSAL runs these through
# string.Template.safe_substitute, so avoid a literal "$" (none here). HTML is
# detected by a leading "<".
_SIGNIN_SUCCESS_HTML = """<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>Signed in</title>
<style>
 html,body{height:100%;margin:0}
 body{display:flex;align-items:center;justify-content:center;
      font-family:'Segoe UI',system-ui,-apple-system,sans-serif;
      background:#1e1e1e;color:#e8e8e8}
 .card{max-width:440px;padding:40px;text-align:center}
 .badge{width:64px;height:64px;border-radius:16px;margin:0 auto 22px;
        background:linear-gradient(160deg,#1b8ae0,#0063b5);
        display:flex;align-items:center;justify-content:center}
 h1{font-size:21px;font-weight:600;margin:0 0 10px}
 p{color:#9aa0a6;font-size:14px;line-height:1.55;margin:0}
</style></head>
<body><div class="card">
 <div class="badge">
  <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="#fff"
       stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
   <path d="M20 6 9 17l-5-5"/></svg>
 </div>
 <h1>You're signed in</h1>
 <p>You can close this tab and return to Azure DevOps Test Case Manager.</p>
</div>
</body></html>
"""

_SIGNIN_ERROR_HTML = """<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>Sign-in problem</title>
<style>
 html,body{height:100%;margin:0}
 body{display:flex;align-items:center;justify-content:center;
      font-family:'Segoe UI',system-ui,-apple-system,sans-serif;
      background:#1e1e1e;color:#e8e8e8}
 .card{max-width:460px;padding:40px;text-align:center}
 h1{font-size:21px;font-weight:600;margin:0 0 10px}
 p{color:#9aa0a6;font-size:14px;line-height:1.55;margin:0}
</style></head>
<body><div class="card">
 <h1>Sign-in didn't complete</h1>
 <p>You can close this window and try signing in again from the app.</p>
</div></body></html>
"""


class MsalSignInError(Exception):
    """Raised when interactive sign-in fails or is cancelled."""


class MsalAuthenticator:
    """Wraps MSAL PublicClientApplication(s) for ADO access tokens.

    Thread-safe: sign-in runs on a GUI worker thread while silent refreshes
    happen on API worker threads.
    """

    def __init__(self):
        self._broker_app = None    # WAM-enabled (native one-click picker)
        self._browser_app = None   # system-browser fallback
        self._app = None           # whichever app holds the signed-in account
        self._account = None
        self._lock = threading.Lock()

    def _make_app(self, *, broker: bool):
        import msal
        # default in-memory TokenCache — nothing written to disk by us
        return msal.PublicClientApplication(
            _AZURE_CLI_CLIENT_ID,
            authority=_AUTHORITY,
            enable_broker_on_windows=broker,
        )

    def _broker(self):
        if self._broker_app is None:
            self._broker_app = self._make_app(broker=True)
        return self._broker_app

    def _browser(self):
        if self._browser_app is None:
            self._browser_app = self._make_app(broker=False)
        return self._browser_app

    def sign_in_interactive(self, timeout: int = 180,
                            parent_window_handle=None) -> str:
        """Sign in and return an access token.

        When ``parent_window_handle`` (the app window's HWND) is supplied, tries
        the Windows WAM broker first — the native account picker that reuses the
        device's Microsoft session, so sign-in is one click. Falls back to the
        system browser if the broker is unavailable or fails. Blocks until the
        flow completes (run on a worker thread). Raises MsalSignInError on
        failure or cancellation.
        """
        # 1) WAM broker: one-click, shared Windows session. (Currently gated off
        #    via _BROKER_ENABLED — see the constant near the top of this module.)
        if _BROKER_ENABLED and parent_window_handle is not None:
            try:
                app = self._broker()
                result = app.acquire_token_interactive(
                    scopes=[_ADO_SCOPE],
                    prompt="select_account",
                    timeout=timeout,
                    parent_window_handle=parent_window_handle,
                )
                if "access_token" in result:
                    return self._remember(app, result)
            except Exception:
                pass  # broker missing/failed -> fall through to the browser

        # 2) System browser fallback.
        app = self._browser()
        try:
            result = app.acquire_token_interactive(
                scopes=[_ADO_SCOPE],
                timeout=timeout,
                prompt="select_account",
                # Branded landing page that auto-closes (best-effort) after the
                # redirect, instead of MSAL's plain "please close this tab" text.
                success_template=_SIGNIN_SUCCESS_HTML,
                error_template=_SIGNIN_ERROR_HTML,
            )
        except Exception as exc:
            raise MsalSignInError(f"Could not start sign-in: {exc}") from exc

        if "access_token" not in result:
            raise MsalSignInError(
                result.get("error_description")
                or result.get("error")
                or "Sign-in was cancelled or timed out."
            )
        return self._remember(app, result)

    def _remember(self, app, result) -> str:
        """Record the signed-in account + active app; return the access token."""
        accounts = app.get_accounts()
        with self._lock:
            self._app = app
            self._account = accounts[0] if accounts else None
        return result["access_token"]

    def has_account(self) -> bool:
        with self._lock:
            return self._account is not None

    def acquire_token_silent(self) -> str | None:
        """Return a fresh access token from the cache / refresh token.

        Returns None when no account is signed in or the refresh fails
        (e.g. revoked session) — the next 401 then triggers an interactive
        re-sign-in.
        """
        with self._lock:
            account = self._account
            app = self._app
        if account is None or app is None:
            return None
        try:
            result = app.acquire_token_silent(
                scopes=[_ADO_SCOPE], account=account,
            )
        except Exception:
            return None
        if result and "access_token" in result:
            return result["access_token"]
        return None

    def sign_out(self):
        with self._lock:
            if self._account is not None and self._app is not None:
                try:
                    self._app.remove_account(self._account)
                except Exception:
                    pass
            self._account = None
