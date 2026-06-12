"""Microsoft Entra ID sign-in for Azure DevOps via MSAL.

Signs in through the well-known Azure CLI public client, so no app
registration is required in the tenant. The token cache is kept in memory
only (matching TokenManager's never-persist-to-disk policy): sign-in is
needed once per app launch, after which access tokens refresh silently.
"""

import threading

# Well-known public client id for Azure CLI — pre-consented for the
# Azure DevOps resource in Entra ID tenants, so no app registration is needed.
_AZURE_CLI_CLIENT_ID = "04b07795-8ddb-461a-bbee-02f9e1bf7b46"

# Azure DevOps service principal id; "/.default" requests its delegated scopes.
_ADO_SCOPE = "499b84ac-1321-427f-aa17-267ca6975798/.default"

_AUTHORITY = "https://login.microsoftonline.com/organizations"


class MsalSignInError(Exception):
    """Raised when interactive sign-in fails or is cancelled."""


class MsalAuthenticator:
    """Wraps an MSAL PublicClientApplication for ADO access tokens.

    Thread-safe: sign-in runs on a GUI worker thread while silent refreshes
    happen on API worker threads.
    """

    def __init__(self):
        self._app = None
        self._account = None
        self._lock = threading.Lock()

    def _get_app(self):
        if self._app is None:
            import msal
            self._app = msal.PublicClientApplication(
                _AZURE_CLI_CLIENT_ID,
                authority=_AUTHORITY,
                # default in-memory TokenCache — nothing written to disk
            )
        return self._app

    def sign_in_interactive(self, timeout: int = 180) -> str:
        """Open the system browser for sign-in and return an access token.

        Blocks until the browser flow completes (run on a worker thread).
        Raises MsalSignInError on failure or cancellation.
        """
        app = self._get_app()
        try:
            result = app.acquire_token_interactive(
                scopes=[_ADO_SCOPE],
                timeout=timeout,
                prompt="select_account",
            )
        except Exception as exc:
            raise MsalSignInError(
                f"Could not start the browser sign-in: {exc}"
            ) from exc

        if "access_token" not in result:
            raise MsalSignInError(
                result.get("error_description")
                or result.get("error")
                or "Sign-in was cancelled or timed out."
            )

        accounts = app.get_accounts()
        with self._lock:
            self._account = accounts[0] if accounts else None
        return result["access_token"]

    def has_account(self) -> bool:
        with self._lock:
            return self._account is not None

    def acquire_token_silent(self) -> str | None:
        """Return a fresh access token from the cache / refresh token.

        Returns None when no account is signed in or the refresh fails
        (e.g. revoked session) — callers fall back to the manual-paste flow.
        """
        with self._lock:
            account = self._account
        if account is None:
            return None
        try:
            result = self._get_app().acquire_token_silent(
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
