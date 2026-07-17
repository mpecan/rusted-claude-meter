use meter_core::SessionKey;
use serde::Deserialize;

use crate::error::ApiError;
use crate::headers::browser_headers;
use crate::response::UsageResponse;

const DEFAULT_BASE_URL: &str = "https://claude.ai/api";

/// An organization the session key has access to, from
/// `GET /api/organizations`. Fetching it doubles as session validation.
#[derive(Debug, Clone, Deserialize)]
pub struct Organization {
    pub uuid: String,
    pub name: String,
}

/// Client for the claude.ai usage endpoints.
///
/// The base URL is injectable so tests can point it at a local mock server;
/// production code uses [`UsageClient::new`].
#[derive(Debug, Clone)]
pub struct UsageClient {
    http: reqwest::Client,
    base_url: String,
}

impl UsageClient {
    pub fn new(session_key: &SessionKey) -> Result<Self, ApiError> {
        Self::with_base_url(session_key, DEFAULT_BASE_URL)
    }

    pub fn with_base_url(session_key: &SessionKey, base_url: &str) -> Result<Self, ApiError> {
        let headers = browser_headers(session_key).map_err(|_| ApiError::InvalidSessionKey)?;
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }

    /// List organizations; also the cheapest way to validate a session key.
    pub async fn organizations(&self) -> Result<Vec<Organization>, ApiError> {
        let body = self
            .get(&format!("{}/organizations", self.base_url))
            .await?;
        Ok(serde_json::from_str(&body)?)
    }

    /// Fetch the raw usage payload for one organization.
    pub async fn usage(&self, org_id: &str) -> Result<UsageResponse, ApiError> {
        let url = format!("{}/organizations/{org_id}/usage", self.base_url);
        let body = self.get(&url).await?;
        Ok(serde_json::from_str(&body)?)
    }

    async fn get(&self, url: &str) -> Result<String, ApiError> {
        let response = self.http.get(url).send().await?;
        match response.status().as_u16() {
            200 => Ok(response.text().await?),
            401 => Err(ApiError::Unauthorized),
            403 => Err(ApiError::Blocked),
            status => Err(ApiError::Status(status)),
        }
    }
}
