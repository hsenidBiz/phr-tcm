//! HTTP transport for `AdoClient`: the only place requests are built and
//! status codes become `AdoError`s. Verbs stop at GET / POST / PATCH —
//! no DELETE (tests/ado.rs scans this file). The one place that does
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
fn tidy(url: &str) -> String {
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
/// Pinned by `tests/ado_network.rs` (the exact text, and the rule that no
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
pub(crate) fn network_error(e: &reqwest::Error) -> AdoError {
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
    async fn send(
        &self,
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
        super::throttle::pace().await;
        let started = std::time::Instant::now();
        let request = build(self.http.request(method.clone(), url).bearer_auth(&self.token));
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
                Ok(resp)
            }
            Err(e) => {
                crate::applog::warn(format!(
                    "{method} {} failed after {ms} ms: {e}",
                    tidy(url)
                ));
                Err(network_error(&e))
            }
        }
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

    /// POST used for WIQL queries only — query-only, creates and modifies
    /// nothing. Still no DELETE through this funnel.
    pub(crate) async fn post_json_query(
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
}

/// The delay ADO asked for on THIS response, if any.
///
/// `Retry-After` is the documented instruction; `X-RateLimit-Delay` says
/// how long the request we just made was already held, which is ADO's
/// early warning that the account is over its budget. Either one means
/// "slow down". Absent on a healthy response, so this is `None` on the
/// overwhelming majority of calls.
fn server_delay(resp: &reqwest::Response) -> Option<u64> {
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
