import threading
from datetime import datetime


class TokenManager:
    """Holds the Bearer token and connection config in memory only — never persisted to disk."""

    def __init__(self):
        self._token: str = ""
        self._org_url: str = ""
        self._project: str = ""
        self._payload_cache: dict | None = None
        self._msal = None  # MsalAuthenticator when signed in via Microsoft
        self._refresh_lock = threading.Lock()

    def set_credentials(self, token: str, org_url: str, project: str):
        self._token = token.strip()
        self._org_url = org_url.rstrip("/")
        self._project = project.strip()
        self._payload_cache = None

    def update_token(self, token: str):
        self._token = token.strip()
        self._payload_cache = None

    def attach_msal(self, authenticator):
        """Enable silent token refresh through a signed-in MsalAuthenticator."""
        self._msal = authenticator

    @property
    def msal_authenticator(self):
        """The attached MsalAuthenticator (for interactive re-sign-in), or None."""
        return self._msal

    def auto_refresh_active(self) -> bool:
        return self._msal is not None and self._msal.has_account()

    def _ensure_fresh(self):
        """Silently refresh the token via MSAL when it is about to expire.

        Called from the header getters, which run on API worker threads —
        never blocks the GUI thread. On refresh failure the stale token is
        kept; the resulting 401 surfaces through the existing
        TokenExpiredError path (interactive re-sign-in).
        """
        if self._msal is None or not self.is_likely_expired():
            return
        with self._refresh_lock:
            if not self.is_likely_expired():  # another thread already refreshed
                return
            fresh = self._msal.acquire_token_silent()
            if fresh:
                self.update_token(fresh)

    @property
    def org_url(self) -> str:
        return self._org_url

    @property
    def project(self) -> str:
        return self._project

    def has_credentials(self) -> bool:
        return bool(self._token and self._org_url and self._project)

    def get_patch_headers(self) -> dict:
        """Headers for POST/PATCH with JSON Patch body."""
        self._ensure_fresh()
        return {
            "Authorization": f"Bearer {self._token}",
            "Content-Type": "application/json-patch+json",
            "Accept": "application/json",
        }

    def get_json_headers(self) -> dict:
        """Headers for GET requests."""
        self._ensure_fresh()
        return {
            "Authorization": f"Bearer {self._token}",
            "Accept": "application/json",
        }

    def _get_payload(self) -> dict | None:
        """Decode JWT payload once and cache until the token changes."""
        if not self._token:
            return None
        if self._payload_cache is not None:
            return self._payload_cache
        try:
            import jwt
            payload = jwt.decode(
                self._token,
                options={"verify_signature": False},
                algorithms=["RS256", "HS256"],
            )
            self._payload_cache = payload
            return payload
        except Exception:
            return None

    def get_current_upn(self) -> str | None:
        """Extract the user's email / UPN from the JWT payload without verifying the signature."""
        payload = self._get_payload()
        if payload is None:
            return None
        return payload.get("upn") or payload.get("unique_name") or None

    def get_expiry(self) -> datetime | None:
        """Decode the JWT exp claim without signature verification."""
        payload = self._get_payload()
        if payload is None:
            return None
        exp = payload.get("exp")
        return datetime.fromtimestamp(exp) if exp else None

    def get_seconds_remaining(self) -> int:
        """Returns seconds until expiry, 0 if expired, -1 if unknown."""
        expiry = self.get_expiry()
        if expiry is None:
            return -1
        return max(0, int((expiry - datetime.now()).total_seconds()))

    def get_expiry_display(self) -> str:
        expiry = self.get_expiry()
        if expiry is None:
            return "Expiry unknown"
        total_secs = int((expiry - datetime.now()).total_seconds())
        if total_secs <= 0:
            return "Token has EXPIRED"
        hours = total_secs // 3600
        minutes = (total_secs % 3600) // 60
        seconds = total_secs % 60
        time_str = expiry.strftime("%I:%M %p").lstrip("0")
        if hours > 0:
            countdown = f"{hours}h {minutes:02d}m {seconds:02d}s"
        elif minutes > 0:
            countdown = f"{minutes}m {seconds:02d}s"
        else:
            countdown = f"{seconds}s"
        return f"Expires in {countdown}  ·  {time_str}"

    def is_expired(self) -> bool:
        """True only when a token is present and has passed its expiry time.

        With MSAL auto-refresh active the credentials never count as expired:
        the next API call refreshes the token silently before sending. This is
        a pure check (no network) so it is safe on the GUI thread.
        """
        if self.auto_refresh_active():
            return False
        expiry = self.get_expiry()
        if expiry is None:
            return False
        return datetime.now() >= expiry

    def is_likely_expired(self) -> bool:
        expiry = self.get_expiry()
        if expiry is None:
            return False
        return datetime.now().timestamp() > (expiry.timestamp() - 60)
