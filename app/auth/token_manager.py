from datetime import datetime


class TokenManager:
    """Holds the Bearer token and connection config in memory only — never persisted to disk."""

    def __init__(self):
        self._token: str = ""
        self._org_url: str = ""
        self._project: str = ""

    def set_credentials(self, token: str, org_url: str, project: str):
        self._token = token.strip()
        self._org_url = org_url.rstrip("/")
        self._project = project.strip()

    def update_token(self, token: str):
        self._token = token.strip()

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
        return {
            "Authorization": f"Bearer {self._token}",
            "Content-Type": "application/json-patch+json",
            "Accept": "application/json",
        }

    def get_json_headers(self) -> dict:
        """Headers for GET requests."""
        return {
            "Authorization": f"Bearer {self._token}",
            "Accept": "application/json",
        }

    def get_expiry(self) -> datetime | None:
        """Decode the JWT exp claim without signature verification."""
        if not self._token:
            return None
        try:
            import jwt
            payload = jwt.decode(
                self._token,
                options={"verify_signature": False},
                algorithms=["RS256", "HS256"],
            )
            exp = payload.get("exp")
            if exp:
                return datetime.fromtimestamp(exp)
        except Exception:
            pass
        return None

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

    def is_likely_expired(self) -> bool:
        expiry = self.get_expiry()
        if expiry is None:
            return False
        return datetime.now().timestamp() > (expiry.timestamp() - 60)
