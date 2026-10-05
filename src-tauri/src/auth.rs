//! Microsoft Entra ID sign-in via OAuth2 authorization-code + PKCE (RFC 7636)
//! against the well-known Azure CLI public client — no app registration, no
//! PATs, no client secret. Tokens live in [`AuthState`] in Rust memory and
//! must never be returned over IPC (enforced by tests/bindings.rs). The one
//! exception to "memory only" is Stay signed in: the refresh token is kept
//! in Windows Credential Manager between launches (`crate::saved_session`).

use base64::Engine;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

const CLIENT_ID: &str = "04b07795-8ddb-461a-bbee-02f9e1bf7b46"; // Azure CLI public client
const AUTHORITY: &str = "https://login.microsoftonline.com/organizations";
const SCOPE: &str = "499b84ac-1321-427f-aa17-267ca6975798/.default offline_access openid profile";
const TOKEN_URL: &str = "https://login.microsoftonline.com/organizations/oauth2/v2.0/token";

/// How long the browser has to come back with the redirect.
pub const SIGN_IN_WINDOW: Duration = Duration::from_secs(300);
/// How long one loopback connection may take to send its request.
pub const LOOPBACK_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// The most of one loopback request's head that is read before answering.
const LOOPBACK_HEAD_CAP: usize = 16 * 1024;
/// How long a closing loopback connection waits for the browser to close
/// its side (see `respond`).
const LOOPBACK_DRAIN: Duration = Duration::from_millis(500);
/// How long the loopback keeps answering after a sign-in, for a browser
/// that retries the redirect.
pub const SIGN_IN_LINGER: Duration = Duration::from_secs(30);
/// How long one connection may take to send its request during the
/// linger. Connections there are served one at a time, so a browser
/// preconnect that sends nothing must not hold the retry behind it for
/// the full `LOOPBACK_READ_TIMEOUT`.
const LINGER_READ_TIMEOUT: Duration = Duration::from_secs(1);
/// What a sign-in nobody finished says.
pub const SIGN_IN_TIMEOUT: &str = "Sign-in timed out. Try again.";

/// Refresh this long before the access token actually expires.
const EARLY_RENEW: Duration = Duration::from_secs(300);

pub fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// RFC 7636: verifier is 43-128 chars of unreserved charset; challenge = b64url(sha256(verifier)).
pub fn pkce_pair() -> (String, String) {
    let random: [u8; 32] = rand::random();
    let verifier = b64url(&random);
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    (verifier, challenge)
}

/// The full token set, Rust-side only. Deliberately does NOT derive
/// specta::Type / Serialize: nothing here may ever cross IPC.
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<Instant>,
    pub account: Option<String>,
}

#[derive(Default)]
pub struct AuthState {
    pub tokens: Option<TokenSet>,
}

/// True when the access token is missing an expiry or within EARLY_RENEW of it.
pub fn needs_refresh(expires_at: Option<Instant>, now: Instant) -> bool {
    match expires_at {
        None => true,
        Some(at) => now + EARLY_RENEW >= at,
    }
}

/// Store refreshed tokens only if the session they were refreshed FROM is
/// still the current one. A refresh for account A that finishes after the
/// user signed in as B (or signed out) must not replace B's tokens.
pub fn store_refreshed(state: &mut AuthState, sent_refresh_token: &str, fresh: TokenSet) -> bool {
    let still_current =
        state.tokens.as_ref().and_then(|t| t.refresh_token.as_deref()) == Some(sent_refresh_token);
    if still_current {
        state.tokens = Some(fresh);
    }
    still_current
}

/// The authorize URL, made to ask which account to use. After Sign out the
/// browser still holds the Microsoft session it signed in with, and without
/// this it would sign straight back in to the same account.
pub fn choosing_account(authorize_url: String) -> String {
    authorize_url + "&prompt=select_account"
}

pub fn build_authorize_url(challenge: &str, redirect_uri: &str, state: &str) -> String {
    format!(
        "{AUTHORITY}/oauth2/v2.0/authorize?client_id={CLIENT_ID}&response_type=code&redirect_uri={}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        urlencoding::encode(redirect_uri),
        urlencoding::encode(SCOPE),
    )
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

impl TokenResponse {
    fn into_token_set(self, fallback_account: Option<String>) -> TokenSet {
        let account = self
            .id_token
            .as_deref()
            .and_then(upn_from_id_token)
            .or(fallback_account);
        TokenSet {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at: self
                .expires_in
                .map(|secs| Instant::now() + Duration::from_secs(secs)),
            account,
        }
    }
}

/// Extract the UPN ("preferred_username") from an id_token without signature
/// verification — display only, never used for authorization decisions.
pub fn upn_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims["preferred_username"].as_str().map(String::from)
}

/// Form params for the refresh_token grant (pure, for tests).
pub fn refresh_params(refresh_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("client_id", CLIENT_ID.to_string()),
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
        ("scope", SCOPE.to_string()),
    ]
}

/// What sign-in says when it cannot talk to Microsoft's sign-in service.
/// Offline it used to say "Can't reach Azure DevOps" - but sign-in asks
/// Microsoft, not Azure DevOps. The same shape and the same rule as the
/// sentences in `ado/transport.rs`: no URL (reqwest's Display names the
/// login endpoint, which nobody can act on), and a way to the details.
pub const SIGN_IN_NET_TIMEOUT: &str =
    "Microsoft sign-in didn't respond in time. Check your connection and try again. Settings → Logs has the details.";
pub const SIGN_IN_NET_UNREACHABLE: &str =
    "Can't reach Microsoft sign-in. Check your internet connection or VPN, then try again. Settings → Logs has the details.";
pub const SIGN_IN_NET_GENERIC: &str =
    "The connection to Microsoft sign-in failed. Try again - restart the app if it keeps happening. Settings → Logs has the details.";

/// How every refusal from the token endpoint begins. A refusal is Microsoft
/// saying no to THIS token or request - expired, revoked, a policy that now
/// wants the person at the keyboard - which is what makes a saved sign-in
/// worth forgetting (`is_refusal`). Trouble on Microsoft's side is not.
pub const SIGN_IN_REFUSED: &str = "Microsoft sign-in refused the request";
pub const SIGN_IN_UNAVAILABLE: &str = "Microsoft sign-in is having trouble right now";

/// True when `error` (from `refresh` or the code exchange) is a refusal,
/// not a network failure or a problem on Microsoft's side.
pub fn is_refusal(error: &str) -> bool {
    error.starts_with(SIGN_IN_REFUSED)
}

fn sign_in_network_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        SIGN_IN_NET_TIMEOUT
    } else if e.is_connect() {
        SIGN_IN_NET_UNREACHABLE
    } else {
        SIGN_IN_NET_GENERIC
    }
    .to_string()
}

async fn post_token_endpoint(url: &str, params: &[(&str, String)]) -> Result<TokenResponse, String> {
    // Entra's real token endpoint is never loopback; only tests point this
    // at a wiremock `MockServer`, so the same pooling hazard `ado/mod.rs`
    // documents for `AdoClient` applies here too.
    let client = if crate::ado::is_loopback_base(url) {
        crate::ado::unpooled_loopback_client()
    } else {
        crate::ado::http_client()
    };
    let resp = client
        .post(url)
        .form(params)
        .send()
        .await
        .map_err(|e| {
            crate::applog::warn(format!("token endpoint request failed: {e}"));
            sign_in_network_error(&e)
        })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        crate::applog::warn(format!(
            "token endpoint returned {status}: {}",
            body.chars().take(600).collect::<String>()
        ));
        // Entra's `error` code (invalid_grant, ...) - never `error_uri` or
        // the description, which carry URLs and trace ids.
        let code: String = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or_else(|| status.as_u16().to_string())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(40)
            .collect();
        // A 5xx or a 429 is Microsoft's side, not a verdict on the token.
        if status.is_server_error() || status.as_u16() == 429 {
            return Err(format!(
                "{SIGN_IN_UNAVAILABLE} ({code}). Try again in a few minutes - Settings → Logs has the details."
            ));
        }
        return Err(format!(
            "{SIGN_IN_REFUSED} ({code}). Sign in again - Settings → Logs has the details."
        ));
    }
    resp.json().await.map_err(|e| {
        crate::applog::warn(format!("token endpoint response could not be read: {e}"));
        sign_in_network_error(&e)
    })
}

/// `refresh` against a given token endpoint - the seam the no-URL tests use.
pub async fn refresh_at(
    token_url: &str,
    refresh_token: &str,
    account: Option<String>,
) -> Result<TokenSet, String> {
    let params = refresh_params(refresh_token);
    let tokens = post_token_endpoint(token_url, &params).await?;
    Ok(tokens.into_token_set(account))
}

/// Exchange a refresh token for a new token set (silent renew).
pub async fn refresh(refresh_token: &str, account: Option<String>) -> Result<TokenSet, String> {
    refresh_at(TOKEN_URL, refresh_token, account).await
}

async fn exchange_code(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, String> {
    let params = [
        ("client_id", CLIENT_ID.to_string()),
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("code_verifier", verifier.to_string()),
        ("scope", SCOPE.to_string()),
    ];
    post_token_endpoint(TOKEN_URL, &params).await
}

/// The one page a user sees outside the app - make it feel like ours.
const SIGNED_IN_PAGE: &str = r##"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">
<title>Signed in - Test Case Manager</title>
<style>
  * { margin:0; box-sizing:border-box; }
  body { min-height:100vh; display:flex; align-items:center; justify-content:center;
         font-family:'Segoe UI',system-ui,sans-serif; color:#f8fafc;
         background:radial-gradient(1200px 600px at 20% -10%, #1e3a5f 0%, #0f172a 55%, #0a0f1e 100%); }
  .card { text-align:center; padding:56px 64px; border:1px solid rgba(148,163,184,.25);
          border-radius:20px; background:rgba(30,41,59,.55); backdrop-filter:blur(8px);
          box-shadow:0 24px 80px rgba(0,0,0,.45); animation:pop .5s cubic-bezier(.2,.9,.3,1.2) both; }
  .mark { width:84px; height:84px; margin:0 auto 20px; border-radius:22px;
          background:linear-gradient(135deg,#2aa5e0,#1565c0); display:flex;
          align-items:center; justify-content:center; box-shadow:0 10px 30px rgba(21,101,192,.5); }
  .mark svg { width:44px; height:44px; stroke:#fff; fill:none; stroke-width:2;
              stroke-linecap:round; stroke-linejoin:round; }
  h1 { font-size:26px; font-weight:600; margin-bottom:8px; }
  p  { color:#94a3b8; font-size:15px; line-height:1.6; }
  .check { display:inline-flex; align-items:center; gap:8px; margin-top:22px;
           padding:8px 18px; border-radius:999px; font-size:13.5px; font-weight:600;
           color:#4ade80; background:rgba(34,197,94,.12); border:1px solid rgba(74,222,128,.35);
           animation:fade .6s .25s both; }
  @keyframes pop  { from { opacity:0; transform:translateY(14px) scale(.97); } }
  @keyframes fade { from { opacity:0; } }
</style></head><body>
<div class="card">
  <div class="mark"><svg viewBox="0 0 24 24"><path d="M10 2v7.31L4.29 19.7A2 2 0 0 0 6.05 22h11.9a2 2 0 0 0 1.76-2.3L14 9.31V2"/><path d="M8.5 2h7"/><path d="M7 16h10"/></svg></div>
  <h1>You're signed in</h1>
  <p>Head back to <b>Test Case Manager</b> - it already has your session.<br>This tab can be closed.</p>
  <span class="check">&#10003;&nbsp;Authentication complete</span>
</div>
<script>setTimeout(function(){ try { window.close(); } catch(e){} }, 2500);</script>
</body></html>"##;

/// Shown when the browser came back without a usable answer.
const FAILED_PAGE: &str = r##"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Sign-in did not complete - Test Case Manager</title>
<style>
  * { margin:0; box-sizing:border-box; }
  body { min-height:100vh; display:flex; align-items:center; justify-content:center;
         font-family:'Segoe UI',system-ui,sans-serif; color:#f8fafc; background:#0f172a; }
  .card { text-align:center; padding:48px 56px; border:1px solid rgba(148,163,184,.25);
          border-radius:20px; background:rgba(30,41,59,.55); }
  h1 { font-size:24px; font-weight:600; margin-bottom:8px; }
  p  { color:#94a3b8; font-size:15px; line-height:1.6; }
</style></head><body>
<div class="card">
  <h1>Sign-in did not complete</h1>
  <p>Go back to <b>Test Case Manager</b> and sign in again.<br>This tab can be closed.</p>
</div>
</body></html>"##;

/// What the loopback made of one request line.
#[derive(Debug, PartialEq, Eq)]
pub enum Redirect {
    /// The authorization code, state checked.
    Code(String),
    /// The browser came back, but not with a sign-in: denied, wrong state,
    /// no code. The sentence is for the user and names no URL.
    Refused(String),
    /// Not the redirect at all (favicon, a preconnect that sent nothing).
    NotTheRedirect,
}

pub fn read_redirect(request_line: &str, expected_state: &str) -> Redirect {
    let Some(target) = request_line
        .strip_prefix("GET ")
        .and_then(|r| r.split_whitespace().next())
    else {
        return Redirect::NotTheRedirect;
    };
    let Some(query) = target.strip_prefix("/?") else {
        return Redirect::NotTheRedirect;
    };
    let (mut code, mut state, mut error) = (None, None, None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("code", v)) => code = Some(v.to_string()),
            Some(("state", v)) => state = Some(v.to_string()),
            Some(("error", v)) => error = Some(v.to_string()),
            _ => {}
        }
    }
    if state.as_deref() != Some(expected_state) {
        return Redirect::Refused("The sign-in reply did not belong to this attempt. Try again.".into());
    }
    if let Some(e) = error {
        let e: String = e.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(40).collect();
        return Redirect::Refused(format!("Sign-in was not completed ({e}). Try again."));
    }
    match code {
        Some(c) if !c.is_empty() => Redirect::Code(c),
        _ => Redirect::Refused("The sign-in reply carried no authorization code. Try again.".into()),
    }
}

/// Write one answer and close the connection the way a browser expects.
///
/// Windows answers a socket closed with received bytes still unread by
/// resetting the connection, so the browser showed "connection was reset"
/// even after the page had been sent. So: write, half-close, then read
/// whatever the browser still sends until it closes its side (or
/// `LOOPBACK_DRAIN` passes), and only then drop the socket.
fn respond(stream: &mut std::net::TcpStream, status: &str, body: &str) {
    use std::io::{Read, Write};
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let until = Instant::now() + LOOPBACK_DRAIN;
    let mut sink = [0u8; 4096];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return;
        }
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

/// The request line of one loopback request, after reading its whole head
/// (up to the blank line, at most `LOOPBACK_HEAD_CAP` bytes) against ONE
/// budget. A per-read timeout alone restarts with every byte, so a
/// connection that sent a byte at a time just inside it could hold the wait
/// far past the sign-in window. Reading the whole head matters: headers
/// left unread when the socket closes turn into a reset (see `respond`).
/// A head that ends early (the budget, the cap, the peer closing) still
/// answers if its request line came whole. `None` when no whole line came.
fn read_request_line(stream: &mut std::net::TcpStream, budget: Duration) -> Option<String> {
    use std::io::Read;
    let until = Instant::now() + budget;
    let mut head: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let line = |head: &[u8]| -> Option<String> {
        let end = head.iter().position(|&b| b == b'\n')?;
        String::from_utf8(head[..=end].to_vec()).ok()
    };
    loop {
        if head.windows(4).any(|w| w == b"\r\n\r\n") || head.len() >= LOOPBACK_HEAD_CAP {
            return line(&head);
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return line(&head);
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return line(&head),
            Ok(n) => head.extend_from_slice(&chunk[..n.min(LOOPBACK_HEAD_CAP - head.len())]),
        }
    }
}

/// The loopback's listening sockets: IPv4 always, and IPv6 (`[::1]`, same
/// port) when it was free, so a browser that tries `localhost` over IPv6
/// first is answered rather than refused.
pub struct Loopback {
    listeners: Vec<std::net::TcpListener>,
}

impl From<std::net::TcpListener> for Loopback {
    fn from(listener: std::net::TcpListener) -> Self {
        Loopback { listeners: vec![listener] }
    }
}

impl Loopback {
    /// Bind `127.0.0.1` on a free port, then `[::1]` on the same port if it
    /// can be had. IPv4 alone is enough to go on with.
    pub fn bind() -> std::io::Result<(Loopback, u16)> {
        use std::net::TcpListener;
        let v4 = TcpListener::bind("127.0.0.1:0")?;
        let port = v4.local_addr()?.port();
        let mut listeners = vec![v4];
        if let Ok(v6) = TcpListener::bind(("::1", port)) {
            listeners.push(v6);
        }
        Ok((Loopback { listeners }, port))
    }

    /// Whether `[::1]` is being answered too.
    pub fn has_ipv6(&self) -> bool {
        self.listeners.iter().any(|l| l.local_addr().is_ok_and(|a| a.is_ipv6()))
    }

    /// The next waiting connection on any listener, made blocking.
    fn accept(&self) -> std::io::Result<Option<std::net::TcpStream>> {
        use std::io::ErrorKind;
        for listener in &self.listeners {
            match listener.accept() {
                Ok((s, _)) => {
                    // An accepted socket inherits the listener's
                    // non-blocking mode on Windows; reads must block (up
                    // to their timeout).
                    if s.set_nonblocking(false).is_ok() {
                        return Ok(Some(s));
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(None)
    }
}

/// Answer one loopback connection: read its request, send the page that
/// fits, close gracefully. What the request was, for the caller to act on.
fn serve(
    mut stream: std::net::TcpStream,
    expected_state: &str,
    budget: Duration,
    write_timeout: Duration,
) -> Option<Redirect> {
    let _ = stream.set_write_timeout(Some(write_timeout));
    let line = read_request_line(&mut stream, budget)?; // no whole line in time: drop it
    let redirect = read_redirect(&line, expected_state);
    match &redirect {
        Redirect::NotTheRedirect => respond(&mut stream, "404 Not Found", ""),
        Redirect::Code(_) => respond(&mut stream, "200 OK", SIGNED_IN_PAGE),
        Redirect::Refused(_) => respond(&mut stream, "200 OK", FAILED_PAGE),
    }
    Some(redirect)
}

/// After a sign-in, keep answering for `linger_for` on a background thread:
/// a browser that retries the redirect (or reloads the tab) gets the same
/// "You're signed in" page instead of "localhost refused to connect". It
/// only serves pages; the code has already been taken and is never used
/// again.
fn linger(loopback: Loopback, expected_state: String, linger_for: Duration, read_timeout: Duration) {
    if linger_for.is_zero() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("sign-in-linger".into())
        .spawn(move || {
            let until = Instant::now() + linger_for;
            while Instant::now() < until {
                match loopback.accept() {
                    Ok(Some(stream)) => {
                        let budget = read_timeout
                            .min(LINGER_READ_TIMEOUT)
                            .min(until.saturating_duration_since(Instant::now()));
                        let _ = serve(stream, &expected_state, budget, read_timeout);
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                    Err(_) => return,
                }
            }
        });
}

/// Wait on the loopback for the browser's redirect, for at most `window`.
///
/// Non-blocking accept so the deadline is real (a closed browser tab used
/// to leave sign-in pending forever, leaking a thread and a port). Each
/// connection gets `read_timeout` in all to send its request (never past
/// `window`); one that sends nothing (a browser preconnect) or something
/// else (a favicon) is dropped or answered with a 404 and the wait goes
/// on, so it cannot hide the real redirect behind it. Once the code is in,
/// the loopback keeps answering for `linger_for` (see `linger`); a refusal
/// does not linger.
pub fn await_redirect(
    loopback: impl Into<Loopback>,
    expected_state: &str,
    window: Duration,
    read_timeout: Duration,
    linger_for: Duration,
) -> Result<String, String> {
    let loopback = loopback.into();
    for listener in &loopback.listeners {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    }
    let deadline = Instant::now() + window;
    loop {
        if Instant::now() >= deadline {
            return Err(SIGN_IN_TIMEOUT.into());
        }
        let stream = match loopback.accept() {
            Ok(Some(s)) => s,
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        // The whole request within `read_timeout`, and never past the
        // sign-in window itself.
        let budget = read_timeout.min(deadline.saturating_duration_since(Instant::now()));
        match serve(stream, expected_state, budget, read_timeout) {
            None | Some(Redirect::NotTheRedirect) => continue,
            Some(Redirect::Code(code)) => {
                linger(loopback, expected_state.to_string(), linger_for, read_timeout);
                return Ok(code);
            }
            Some(Redirect::Refused(why)) => return Err(why),
        }
    }
}

/// Runs the interactive flow: opens the system browser at the authorize URL,
/// waits for the loopback redirect, exchanges the code. `choose_account`
/// makes Microsoft ask which account to use (see `choosing_account`).
pub async fn sign_in_interactive(open_url: impl Fn(&str), choose_account: bool) -> Result<TokenSet, String> {
    let (loopback, port) = Loopback::bind().map_err(|e| e.to_string())?;
    // Entra ID only allows arbitrary ports on "http://localhost" (RFC 8252
    // loopback), NOT on the literal 127.0.0.1 - using the IP form fails with
    // AADSTS50011 against the Azure CLI app registration. The socket still
    // binds to 127.0.0.1 (and [::1] when it can), which is where localhost
    // resolves for the browser redirect.
    let redirect_uri = format!("http://localhost:{port}");
    let (verifier, challenge) = pkce_pair();
    let state = b64url(&rand::random::<[u8; 16]>());
    let url = build_authorize_url(&challenge, &redirect_uri, &state);
    open_url(&if choose_account { choosing_account(url) } else { url });

    let code = tokio::task::spawn_blocking(move || {
        await_redirect(loopback, &state, SIGN_IN_WINDOW, LOOPBACK_READ_TIMEOUT, SIGN_IN_LINGER)
    })
    .await
    .map_err(|e| e.to_string())??;

    let tokens = exchange_code(&code, &verifier, &redirect_uri).await?;
    Ok(tokens.into_token_set(None))
}
