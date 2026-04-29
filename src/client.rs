//! Typed HTTP client for the helpdesk's `/api/v1/*` surface.
//!
//! Carries an optional bearer token; if absent, requests will return 401 from
//! protected endpoints. Designed to be cheap to construct so commands can
//! make one fresh per invocation.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use reqwest::{header, Client as Http, Method, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Client {
    http: Http,
    base: String,
    token: Option<String>,
}

impl Client {
    pub fn new(base: impl Into<String>, token: Option<String>) -> Result<Self> {
        let http = Http::builder()
            .user_agent(format!("csshd/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("building HTTP client")?;
        Ok(Self {
            http,
            base: base.into(),
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    async fn req<TReq: Serialize, TRes: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        body: Option<&TReq>,
    ) -> Result<TRes> {
        let mut rb = self.http.request(method.clone(), self.url(path));
        if let Some(t) = &self.token {
            rb = rb.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        if let Some(b) = body {
            rb = rb.json(b);
        }
        let res = rb.send().await.with_context(|| format!("{method} {path}"))?;
        let status = res.status();
        if status == StatusCode::UNAUTHORIZED {
            bail!("Unauthorized — run `csshd login` to refresh credentials.");
        }
        if status.is_client_error() || status.is_server_error() {
            // Try to lift a helpful message out of the JSON body.
            let body = res.text().await.unwrap_or_default();
            let parsed: Option<serde_json::Value> = serde_json::from_str(&body).ok();
            let msg = parsed
                .as_ref()
                .and_then(|v| {
                    v.get("message")
                        .and_then(|x| x.as_str())
                        .or_else(|| v.get("error").and_then(|x| x.as_str()))
                })
                .map(|s| s.to_string())
                .unwrap_or_else(|| body.chars().take(200).collect());
            bail!("{} {} ({status}): {msg}", method, path);
        }
        let parsed = res
            .json::<TRes>()
            .await
            .with_context(|| format!("decoding {method} {path}"))?;
        Ok(parsed)
    }

    // ── auth ────────────────────────────────────────────────────────────

    pub async fn auth_init(&self, name: Option<&str>) -> Result<DeviceCodeResponse> {
        #[derive(Serialize)]
        struct Req<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            name: Option<&'a str>,
        }
        self.req::<_, _>(Method::POST, "/api/v1/cli/auth/init", Some(&Req { name }))
            .await
    }

    /// One poll iteration. Returns:
    ///   Ok(Some(token)) — approved.
    ///   Ok(None)        — pending; keep polling.
    ///   Err(...)        — terminal (expired, denied, bogus, network).
    pub async fn auth_poll(&self, device_code: &str) -> Result<Option<TokenResponse>> {
        #[derive(Serialize)]
        struct Req<'a> {
            #[serde(rename = "deviceCode")]
            device_code: &'a str,
        }
        let url = self.url("/api/v1/cli/auth/poll");
        let res = self
            .http
            .post(url)
            .json(&Req { device_code })
            .send()
            .await
            .context("POST /api/v1/cli/auth/poll")?;
        match res.status() {
            StatusCode::OK => Ok(Some(res.json::<TokenResponse>().await?)),
            StatusCode::REQUEST_TIMEOUT | StatusCode::PRECONDITION_REQUIRED => Ok(None),
            // 428 PRECONDITION_REQUIRED is the spec for "authorization_pending".
            // reqwest gives StatusCode::PRECONDITION_REQUIRED for it.
            StatusCode::GONE => bail!("Login session expired — run `csshd login` again."),
            StatusCode::FORBIDDEN => bail!("Login was denied in the browser."),
            s => bail!("auth poll: unexpected status {s}"),
        }
    }

    pub async fn whoami(&self) -> Result<WhoAmI> {
        self.req::<(), _>(Method::GET, "/api/v1/cli/whoami", None).await
    }

    // ── tickets ─────────────────────────────────────────────────────────

    pub async fn list_tickets(&self, q: TicketQuery) -> Result<TicketsPage> {
        // Build query as Vec<(&str, String)> and let reqwest handle encoding.
        // Avoids url::form_urlencoded::Serializer, whose inner buffer holds a
        // Cow<'_, [u8]> that's !Send — futures touching it can't cross thread
        // boundaries (breaks tokio::spawn).
        let mut params: Vec<(&str, String)> = Vec::with_capacity(5);
        if let Some(s) = q.status {
            params.push(("status", s));
        }
        if let Some(a) = q.assignee {
            params.push(("assignee", a));
        }
        if let Some(s) = q.search {
            params.push(("q", s));
        }
        if let Some(p) = q.page {
            params.push(("page", p.to_string()));
        }
        if let Some(ps) = q.page_size {
            params.push(("pageSize", ps.to_string()));
        }

        let mut rb = self.http.get(self.url("/api/v1/tickets")).query(&params);
        if let Some(t) = &self.token {
            rb = rb.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let res = rb.send().await.context("GET /api/v1/tickets")?;
        let status = res.status();
        if status == StatusCode::UNAUTHORIZED {
            bail!("Unauthorized — run `csshd login` to refresh credentials.");
        }
        if status.is_client_error() || status.is_server_error() {
            let body = res.text().await.unwrap_or_default();
            bail!("GET /api/v1/tickets ({status}): {body}");
        }
        Ok(res.json::<TicketsPage>().await.context("decoding tickets")?)
    }

    pub async fn get_ticket(&self, id_or_number: &str) -> Result<Ticket> {
        // The API takes IDs (cuid). If the user passes a CSS-XXXXX, we'd need
        // a lookup endpoint — for now require an internal id. `view` handles
        // the human-friendly resolution before calling this.
        self.req::<(), _>(
            Method::GET,
            &format!("/api/v1/tickets/{id_or_number}"),
            None,
        )
        .await
    }

    pub async fn patch_ticket(
        &self,
        id: &str,
        patch: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.req::<_, _>(Method::PATCH, &format!("/api/v1/tickets/{id}"), Some(&patch))
            .await
    }

    pub async fn comment(
        &self,
        id: &str,
        body: &str,
        is_internal: bool,
    ) -> Result<serde_json::Value> {
        #[derive(Serialize)]
        struct Req<'a> {
            body: &'a str,
            #[serde(rename = "isInternal")]
            is_internal: bool,
        }
        self.req::<_, _>(
            Method::POST,
            &format!("/api/v1/tickets/{id}/comments"),
            Some(&Req { body, is_internal }),
        )
        .await
    }

    /// Resolve a ticket number like `CSS-04234` (or `4234`) to its internal id.
    pub async fn resolve_ticket(&self, input: &str) -> Result<String> {
        // The API doesn't expose a number→id lookup directly; use the list
        // endpoint with the search filter as a stand-in. This isn't perfect
        // but covers ~99% of cases without a new endpoint.
        let normalized = input.trim().to_uppercase();
        let number = normalized
            .strip_prefix("CSS-")
            .unwrap_or(&normalized)
            .trim_start_matches('0');
        if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
            // Maybe it was already an id (cuid). Pass through.
            return Ok(input.to_string());
        }
        let needle = format!("CSS-{:0>5}", number);
        let page = self
            .list_tickets(TicketQuery {
                search: Some(needle.clone()),
                ..Default::default()
            })
            .await?;
        page.tickets
            .into_iter()
            .find(|t| t.ticket_number == needle)
            .map(|t| t.id)
            .ok_or_else(|| anyhow!("Ticket {needle} not found"))
    }
}

// ── DTOs ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeviceCodeResponse {
    #[serde(rename = "deviceCode")]
    pub device_code: String,
    #[serde(rename = "userCode")]
    pub user_code: String,
    #[serde(rename = "verificationUri")]
    pub verification_uri: String,
    #[serde(rename = "verificationUriComplete")]
    pub verification_uri_complete: String,
    #[serde(rename = "expiresIn")]
    pub expires_in: u32,
    pub interval: u32,
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhoAmI {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub role: String,
    pub is_active: bool,
    pub team: Option<NamedRef>,
    pub ooo_start: Option<DateTime<Utc>>,
    pub ooo_end: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct NamedRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Default)]
pub struct TicketQuery {
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub search: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct TicketsPage {
    pub tickets: Vec<TicketSummary>,
    pub total: u32,
    pub page: u32,
    #[serde(default, rename = "pageSize")]
    pub page_size: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketSummary {
    pub id: String,
    pub ticket_number: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub source: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub requester: PartyRef,
    pub assigned_agent: Option<PartyRef>,
    pub site: Option<NamedRef>,
}

#[derive(Debug, Deserialize)]
pub struct PartyRef {
    pub id: Option<String>,
    pub name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticket {
    pub id: String,
    pub ticket_number: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub requester: PartyRef,
    pub assigned_agent: Option<PartyRef>,
    pub site: Option<NamedRef>,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub body: String,
    pub is_internal: bool,
    pub created_at: DateTime<Utc>,
    pub author: PartyRef,
}
