//! HTTP transport for `AdoClient`: the only place requests are built and
//! status codes become `AdoError`s. Verbs stop at GET / POST / PATCH —
//! no DELETE (tests/suite/ado.rs scans this file). The one place that does
//! delete is `recycle.rs`, which carries its own send for exactly that
//! reason - so this funnel's verb allow-list stays as narrow as it was.
//!
//! Every request also goes through one function, `send`. That is what
//! makes "what was the app doing when it broke" answerable: pacing,
//! sending, timing and logging happen in a single place, so a call cannot
//! be added later that quietly skips any of them.

use super::{AdoClient, AdoError};

/// A URL as it should appear in the log: no scheme, and without the
/// `api-version` every single call carries. Keeps a request line readable
/// while still naming the exact endpoint that was hit.
/// `pub(crate)` for the Boards route, which logs its own request line
/// before sending it and must tidy that URL the same way `send` does.
pub(crate) fn tidy(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (path, query) = match rest.split_once('?') {
        None => return rest.to_string(),
        Some(parts) => parts,
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !p.starts_with("api-version="))
        .collect();
    if kept.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", kept.join("&"))
    }
}

/// The only three sentences a transport failure may show a user.
///
/// reqwest's `Display` is `error sending request for url (https://...)`:
/// the endpoint, its query string, and nothing anyone can act on. That is
/// also, word for word, what `send` already writes to the log - so putting
/// it in a toast repeated the log at the one person who cannot use it.
/// These say what to try instead and point at where the detail lives.
///
/// Pinned by `tests/suite/ado_network.rs` (the exact text, and the rule that no
/// URL may appear here) and mirrored by `src/dev/faults.ts`, which
/// replays them to simulate a failure. The test fails if the two drift.
pub const NET_TIMEOUT: &str =
    "Azure DevOps didn't respond in time. Check your connection and try again. Settings → Logs has the details.";
pub const NET_UNREACHABLE: &str =
    "Can't reach Azure DevOps. Check your internet connection or VPN, then try again. Settings → Logs has the details.";
pub const NET_GENERIC: &str =
    "The connection to Azure DevOps failed. Try again - restart the app if it keeps happening. Settings → Logs has the details.";

/// Classify a transport failure into something a user can act on.
///
/// Deliberately reqwest's typed predicates rather than matching on the
/// message text: that text is theirs to reword between versions, and a
/// classifier that quietly stopped recognising timeouts would degrade to
/// the generic advice with nothing failing to say so.
pub fn network_error(e: &reqwest::Error) -> AdoError {
    AdoError::Network(
        if e.is_timeout() {
            NET_TIMEOUT
        } else if e.is_connect() {
            NET_UNREACHABLE
        } else {
            NET_GENERIC
        }
        .to_string(),
    )
}

impl AdoClient {
    /// Azure DevOps caps the workitems batch-GET (?ids=) endpoint at 200 ids.
    pub(crate) const WORKITEM_BATCH_SIZE: usize = 200;

    /// Pace, send, time and record one request.
    ///
    /// The bearer token is attached here and never logged - the log is
    /// meant to be pasted into a bug report, so nothing that grants access
    /// may reach it. Successful calls are `debug` (they are a firehose
    /// during a bulk create); anything that did not succeed is `warn`, so
    /// the useful lines still stand out at the default filter.
    ///
    /// A GET is a read and everything else a write; a POST that only reads
    /// (WIQL, search, the work-item batch read) goes through `send_as`
    /// with `Effect::Read` instead.
    pub(crate) async fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, AdoError> {
        let effect = if method == reqwest::Method::GET { Effect::Read } else { Effect::Write };
        self.send_as(effect, method, url, build).await
    }

    /// `send` with the effect stated. Two things follow from it:
    /// - Only a write waits for the local pacer (Settings, request rate).
    ///   Reads still wait out any hold Azure DevOps asked for.
    /// - Only a read is retried, after a failure that is likely to pass: a
    ///   connection that failed, a 500/502/503/504, or a 429 that asks for
    ///   a short wait. A write is never sent twice: a retry after a lost
    ///   answer could create a second work item.
    pub(crate) async fn send_as(
        &self,
        effect: Effect,
        method: reqwest::Method,
        url: &str,
        build: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, AdoError> {
        // Funnelling every call through here means the verb is now a
        // runtime value, so the "no destructive verbs" rule gets a runtime
        // guard to match the one the tests enforce on this file's source.
        // An ALLOW-list, deliberately: a deny-list would have to name the
        // verb it forbids, and would miss the next one somebody adds.
        if !matches!(
            method,
            reqwest::Method::GET | reqwest::Method::POST | reqwest::Method::PATCH
        ) {
            crate::applog::error(format!("refused a {method} request to {}", tidy(url)));
            return Err(AdoError::Network(format!(
                "{method} is not a verb this client will send"
            )));
        }
        let mut request = build(self.http.request(method.clone(), url).bearer_auth(&self.token));
        let mut attempt: u32 = 0;
        loop {
            // A copy for a retry, reads only. A body that cannot be copied
            // (a stream) simply gets no retry.
            let spare = if effect == Effect::Read && attempt < MAX_READ_RETRIES {
                request.try_clone()
            } else {
                None
            };
            super::throttle::pace(effect == Effect::Write).await;
            let started = std::time::Instant::now();
            let outcome = request.send().await;
            let ms = started.elapsed().as_millis();
            match outcome {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let line = format!("{method} {} -> {status} in {ms} ms", tidy(url));
                    if (200..300).contains(&status) {
                        crate::applog::debug(line);
                    } else {
                        crate::applog::warn(line);
                    }
                    // Read the throttle hint on EVERY response, not just 429.
                    // Azure DevOps delays requests before it ever rejects one:
                    // a throttled call "still returns HTTP 200" carrying
                    // Retry-After / X-RateLimit-Delay. Honouring it only on
                    // 429 meant the app kept firing at full pace through the
                    // whole warning phase - see the 2026-08 audit (R-1).
                    if let Some(secs) = server_delay(&resp) {
                        super::throttle::note_server_delay(secs);
                    }
                    if let Some(next) = spare {
                        if status_is_transient(status, &resp) {
                            let wait = retry_wait(attempt);
                            crate::applog::warn(format!(
                                "retrying {method} {} in {} ms (attempt {} of {})",
                                tidy(url),
                                wait.as_millis(),
                                attempt + 2,
                                MAX_READ_RETRIES + 1
                            ));
                            tokio::time::sleep(wait).await;
                            request = next;
                            attempt += 1;
                            continue;
                        }
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    crate::applog::warn(format!(
                        "{method} {} failed after {ms} ms: {e}",
                        tidy(url)
                    ));
                    if let Some(next) = spare {
                        if error_is_transient(&e) {
                            let wait = retry_wait(attempt);
                            crate::applog::warn(format!(
                                "retrying {method} {} in {} ms (attempt {} of {})",
                                tidy(url),
                                wait.as_millis(),
                                attempt + 2,
                                MAX_READ_RETRIES + 1
                            ));
                            tokio::time::sleep(wait).await;
                            request = next;
                            attempt += 1;
                            continue;
                        }
                    }
                    return Err(network_error(&e));
                }
            }
        }
    }

    /// GET each URL, `READ_CONCURRENCY` at a time, and give the answers in
    /// the order the URLs were given. For reads split into 200-id chunks:
    /// a 1,000-case PBI was five round trips one after the other.
    pub(crate) async fn get_json_each(
        &self,
        urls: Vec<String>,
    ) -> Vec<Result<serde_json::Value, AdoError>> {
        use futures::stream::{self, StreamExt};
        stream::iter(urls)
            .map(|url| self.get_json(url))
            .buffered(super::READ_CONCURRENCY)
            .collect()
            .await
    }

    /// `get_json_each`, failing on the first error (no answer is used if
    /// one chunk could not be read).
    pub(crate) async fn get_json_all(
        &self,
        urls: Vec<String>,
    ) -> Result<Vec<serde_json::Value>, AdoError> {
        use futures::stream::{self, StreamExt, TryStreamExt};
        stream::iter(urls)
            .map(|url| self.get_json(url))
            .buffered(super::READ_CONCURRENCY)
            .try_collect()
            .await
    }

    pub(crate) async fn get_json(&self, url: String) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .send(reqwest::Method::GET, &url, |r| {
                r.header("Accept", "application/json")
            })
            .await?;
        Self::handle_json(resp).await
    }

    /// GET returning the raw body as bytes - attachments and screenshots.
    ///
    /// These were fetched with a hand-rolled `self.http.get(..)` that
    /// skipped this funnel entirely: no pacing, no log line (so the request
    /// the user was looking for was simply not there), and every failure
    /// flattened to "no screenshots" - including a 401, which should have
    /// prompted a re-sign-in, and a 429, which should have backed off.
    pub(crate) async fn get_bytes(&self, url: String) -> Result<Vec<u8>, AdoError> {
        let resp = self
            .send(reqwest::Method::GET, &url, |r| {
                r.header("Accept", "application/octet-stream")
            })
            .await?;
        match resp.status().as_u16() {
            200..=299 => Ok(resp
                .bytes()
                .await
                .map_err(|e| {
                    crate::applog::warn(format!("reading {} body failed: {e}", tidy(&url)));
                    network_error(&e)
                })?
                .to_vec()),
            401 => Err(AdoError::Unauthorized),
            403 => Err(AdoError::Forbidden),
            404 => Err(AdoError::NotFound),
            429 => Err(AdoError::RateLimited { retry_after_secs: retry_after(&resp) }),
            s => Err(AdoError::Http { status: s, body: resp.text().await.unwrap_or_default() }),
        }
    }

    /// GET streamed chunk by chunk into a `<dest>.tcm-<unique>.part` file, renamed onto `dest`
    /// on success. On any failure the `.part` is removed and `dest` is left
    /// untouched. Status mapping as `get_bytes`. A local I/O failure names
    /// no path. Returns the bytes written.
    pub(crate) async fn get_to_file(
        &self,
        url: String,
        dest: &std::path::Path,
        deadline: std::time::Duration,
    ) -> Result<u64, AdoError> {
        use std::io::Write;
        let mut resp = self
            .send(reqwest::Method::GET, &url, |r| {
                r.timeout(deadline).header("Accept", "application/octet-stream")
            })
            .await?;
        match resp.status().as_u16() {
            200..=299 => {}
            401 => return Err(AdoError::Unauthorized),
            403 => return Err(AdoError::Forbidden),
            404 => return Err(AdoError::NotFound),
            429 => return Err(AdoError::RateLimited { retry_after_secs: retry_after(&resp) }),
            s => {
                return Err(AdoError::Http {
                    status: s,
                    body: resp.text().await.unwrap_or_default(),
                })
            }
        }
        let part = temp_part_path(dest);
        // Declared before the file so the file closes first when both drop
        // (Windows cannot remove an open file). Covers a cancelled future
        // and a panic as well as every error return.
        let guard = PartGuard(part.clone());
        let local = |e: std::io::Error| {
            crate::applog::warn(format!("saving a download failed: {e}"));
            AdoError::Http { status: 0, body: format!("Could not save the file: {e}") }
        };
        let mut file = std::fs::File::create(&part).map_err(local)?;
        let mut written: u64 = 0;
        let result: Result<(), AdoError> = async {
            loop {
                match resp.chunk().await {
                    Ok(Some(chunk)) => {
                        file.write_all(&chunk).map_err(local)?;
                        written += chunk.len() as u64;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        crate::applog::warn(format!("reading {} body failed: {e}", tidy(&url)));
                        return Err(network_error(&e));
                    }
                }
            }
            file.flush().map_err(local)?;
            Ok(())
        }
        .await;
        drop(file);
        let result = result.and_then(|()| std::fs::rename(&part, dest).map_err(local));
        if result.is_ok() {
            guard.disarm();
        }
        result.map(|()| written)
    }

    /// GET returning the raw body as text - build logs are plain text, not
    /// JSON. Same status handling as the JSON path.
    pub(crate) async fn get_text(&self, url: String) -> Result<String, AdoError> {
        let resp = self
            .send(reqwest::Method::GET, &url, |r| {
                r.header("Accept", "text/plain")
            })
            .await?;
        match resp.status().as_u16() {
            200..=299 => resp.text().await.map_err(|e| {
                crate::applog::warn(format!("reading {} body failed: {e}", tidy(&url)));
                network_error(&e)
            }),
            401 => Err(AdoError::Unauthorized),
            403 => Err(AdoError::Forbidden),
            404 => Err(AdoError::NotFound),
            429 => Err(AdoError::RateLimited { retry_after_secs: retry_after(&resp) }),
            s => Err(AdoError::Http {
                status: s,
                body: resp.text().await.unwrap_or_default(),
            }),
        }
    }

    /// GET returning (body, x-ms-continuationtoken) for ADO's paginated
    /// testplan endpoints.
    pub(crate) async fn get_json_with_continuation(
        &self,
        url: String,
    ) -> Result<(serde_json::Value, Option<String>), AdoError> {
        let resp = self
            .send(reqwest::Method::GET, &url, |r| {
                r.header("Accept", "application/json")
            })
            .await?;
        let cont = resp
            .headers()
            .get("x-ms-continuationtoken")
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body = Self::handle_json(resp).await?;
        Ok((body, cont))
    }

    /// Plain-JSON POST (application/json) - used by testplan/test-run
    /// endpoints. Still no DELETE through this funnel.
    pub(crate) async fn post_json(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .send(reqwest::Method::POST, &url, |r| {
                r.header("Accept", "application/json").json(body)
            })
            .await?;
        Self::handle_json(resp).await
    }

    /// PATCH with a plain JSON body. Not a json-patch document: the Test
    /// Plan suiteentry reorder takes an array of entries, and the only
    /// other PATCH helper (`send_json_patch`) sets a content type that
    /// endpoint refuses.
    pub(crate) async fn patch_json(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .send(reqwest::Method::PATCH, &url, |r| {
                r.header("Accept", "application/json").json(body)
            })
            .await?;
        Self::handle_json(resp).await
    }

    /// POST that only reads (WIQL, search, the work-item batch read):
    /// creates and modifies nothing, so it is paced and retried like a
    /// GET. Still no DELETE through this funnel.
    pub(crate) async fn post_json_query(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .send_as(Effect::Read, reqwest::Method::POST, &url, |r| {
                r.header("Accept", "application/json").json(body)
            })
            .await?;
        Self::handle_json(resp).await
    }

    /// Raw-body POST (application/octet-stream) - the attachment upload
    /// endpoint takes file bytes, not JSON. Still no DELETE here.
    pub(crate) async fn post_octet(
        &self,
        url: String,
        body: String,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .send(reqwest::Method::POST, &url, |r| {
                r.header("Accept", "application/json")
                    .header("Content-Type", "application/octet-stream")
                    .body(body)
            })
            .await?;
        Self::handle_json(resp).await
    }

    /// Plain-JSON PATCH (application/json, not json-patch).
    pub(crate) async fn patch_plain_json(
        &self,
        url: String,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .send(reqwest::Method::PATCH, &url, |r| {
                r.header("Accept", "application/json").json(body)
            })
            .await?;
        Self::handle_json(resp).await
    }

    /// PATCH-shaped request helpers. The only verbs this client will ever
    /// grow are GET, POST and PATCH. The one delete this app makes does
    /// not come through here at all - see `recycle.rs`.
    pub(crate) async fn send_json_patch(
        &self,
        method: reqwest::Method,
        url: String,
        patch: &serde_json::Value,
    ) -> Result<serde_json::Value, AdoError> {
        let resp = self
            .send(method, &url, |r| {
                r.header("Accept", "application/json")
                    .header("Content-Type", "application/json-patch+json")
                    .json(patch)
            })
            .await?;
        Self::handle_json(resp).await
    }

    pub(crate) async fn handle_json(resp: reqwest::Response) -> Result<serde_json::Value, AdoError> {
        match resp.status().as_u16() {
            200..=299 => {
                // Taken before `json()` consumes the response - a decode
                // failure has to name the endpoint whose body we could not
                // read, and by then the response is gone.
                let url = tidy(resp.url().as_str());
                resp.json().await.map_err(|e| {
                    crate::applog::warn(format!("decoding {url} response failed: {e}"));
                    network_error(&e)
                })
            }
            401 => Err(refused(resp, AdoError::Unauthorized).await),
            403 => Err(refused(resp, AdoError::Forbidden).await),
            404 => Err(refused_not_found(resp).await),
            429 => Err(AdoError::RateLimited { retry_after_secs: retry_after(&resp) }),
            s => {
                let url = tidy(resp.url().as_str());
                let body = resp.text().await.unwrap_or_default();
                // A 400 on a PATCH used to leave one line in the log - the
                // status - and nothing to say which rule refused it
                // (2026-10-09, a share's relation PATCH). The reason goes
                // next to it, like the 401/403/404 bodies `refused` keeps.
                if (400..500).contains(&s) {
                    let said = said(&body);
                    if !said.is_empty() {
                        crate::applog::warn(format!("{url} -> {s} said: {said}"));
                    }
                }
                Err(AdoError::Http { status: s, body })
            }
        }
    }
}

/// What Azure DevOps said when it refused a request, fit for the log:
/// trimmed, anything shaped like a credential taken out, and capped at 500
/// characters. Scrubbed BEFORE the cap, so a cut can never leave half a
/// token showing. Azure DevOps does not echo the bearer token back, but a
/// log line is pasted into bug reports, so this does not rely on that.
pub fn said(body: &str) -> String {
    use std::sync::LazyLock;
    static BEARER: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}").unwrap());
    static JWT: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"eyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]+").unwrap()
    });
    let scrubbed = BEARER.replace_all(body.trim(), "$1 [redacted]");
    let scrubbed = JWT.replace_all(&scrubbed, "[redacted]");
    scrubbed.chars().take(500).collect()
}

/// A 401/403/404 becomes a bodiless variant, and until 2026-09-22 the
/// sentence Azure DevOps sent with it went nowhere: 48 suite creates were
/// refused with a 403 and nothing anywhere said whether that was an area
/// permission, an access level or a malformed request. The body goes to
/// the log (which a bug report ships) before the variant is returned;
/// what the person sees is unchanged.
pub(crate) async fn refused(resp: reqwest::Response, err: AdoError) -> AdoError {
    let status = resp.status().as_u16();
    let url = tidy(resp.url().as_str());
    let body = said(&resp.text().await.unwrap_or_default());
    if !body.is_empty() {
        crate::applog::warn(format!("{url} -> {status} said: {body}"));
    }
    err
}

/// What a person is told when Azure DevOps answers 404 with
/// `GitRepositoryNotFoundException` (TF401019). That one answer covers a
/// disabled repository and one the person has no Read permission on, so the
/// sentence names both and no URL (tests/suite/ado_network.rs checks).
/// `src/lib/ipc.ts` quotes it for the webview; a test keeps the two equal.
pub const REPO_UNAVAILABLE: &str =
    "Azure DevOps says this repository is disabled or you cannot read it.";

/// Whether a 404 body is Azure DevOps naming a git repository it will not
/// serve - the `TF401019` code, or the exception it travels with.
fn names_unavailable_repo(body: &str) -> bool {
    body.contains("TF401019") || body.contains("GitRepositoryNotFoundException")
}

/// `refused` for a 404, which can be one of two things: a thing that is not
/// there (`NotFound`, bodiless as ever) or a git repository Azure DevOps
/// will not serve (`RepositoryUnavailable`). The body is logged the same
/// way either time; it is only read here to tell the two apart.
pub(crate) async fn refused_not_found(resp: reqwest::Response) -> AdoError {
    let status = resp.status().as_u16();
    let url = tidy(resp.url().as_str());
    let raw = resp.text().await.unwrap_or_default();
    let body = said(&raw);
    if !body.is_empty() {
        crate::applog::warn(format!("{url} -> {status} said: {body}"));
    }
    if names_unavailable_repo(&raw) {
        AdoError::RepositoryUnavailable
    } else {
        AdoError::NotFound
    }
}

/// The delay ADO asked for on THIS response, if any.
///
/// `Retry-After` is the documented instruction; `X-RateLimit-Delay` says
/// how long the request we just made was already held, which is ADO's
/// early warning that the account is over its budget. Either one means
/// "slow down". Absent on a healthy response, so this is `None` on the
/// overwhelming majority of calls.
pub(crate) fn server_delay(resp: &reqwest::Response) -> Option<u64> {
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            // Values arrive as whole seconds; ADO also emits fractional
            // delays on X-RateLimit-Delay, so take the ceiling rather than
            // dropping a sub-second hold to zero.
            .and_then(|s| s.trim().parse::<f64>().ok())
            .filter(|f| f.is_finite() && *f > 0.0)
            .map(|f| f.ceil() as u64)
    };
    header("Retry-After").or_else(|| header("X-RateLimit-Delay"))
}

/// What a request does, for pacing and retry (see `AdoClient::send_as`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Read,
    Write,
}

/// Retries after the first try, for reads only. Three tries in all.
pub const MAX_READ_RETRIES: u32 = 2;

/// A 429 asking for a longer wait than this is not retried: the hold it
/// sets still applies to the next request, but a screen that waits that
/// long twice looks hung. The error reaches the screen instead.
const RETRY_429_MAX_SECS: u32 = 10;

fn status_is_transient(status: u16, resp: &reqwest::Response) -> bool {
    match status {
        // A 5xx that asks for a long wait is not retried either: the hold
        // it sets still applies, and two of them hung a read a minute.
        500 | 502 | 503 | 504 => server_delay(resp).is_none_or(|secs| secs <= u64::from(RETRY_429_MAX_SECS)),
        429 => retry_after(resp) <= RETRY_429_MAX_SECS,
        _ => false,
    }
}

/// A connection that failed or broke. Not a timeout: the request may have
/// taken its full 60 s, and three of those is a hang, not a recovery.
fn error_is_transient(e: &reqwest::Error) -> bool {
    !e.is_timeout() && (e.is_connect() || e.is_request())
}

/// 400 ms then 1.2 s, each with up to 200 ms of jitter so screens that
/// failed together do not retry together. Any hold the server asked for
/// is added by the pacer on top.
fn retry_wait(attempt: u32) -> std::time::Duration {
    let base = 400u64 * 3u64.pow(attempt);
    std::time::Duration::from_millis(base + rand::random_range(0..200u64))
}


/// ADO's back-off hint, defaulting to 5s when it doesn't send one.
// pub(crate) so the recycle-bin delete - which has its own sender rather
// than going through this funnel - can map 429 the same way everything
// else does, instead of reporting a throttle as an unexplained status.
pub(crate) fn retry_after(resp: &reqwest::Response) -> u32 {
    resp.headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
}

/// A temp file name beside `dest` that no other program is likely to use:
/// process id, a nanosecond clock and a counter.
pub(crate) fn temp_part_path(dest: &std::path::Path) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut name = dest.as_os_str().to_owned();
    name.push(format!(
        ".tcm-{}-{nanos}-{}.part",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::path::PathBuf::from(name)
}

/// Removes its file on drop unless disarmed.
pub(crate) struct PartGuard(pub(crate) std::path::PathBuf);

impl PartGuard {
    pub(crate) fn disarm(mut self) {
        self.0 = std::path::PathBuf::new();
    }
}

impl Drop for PartGuard {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}
