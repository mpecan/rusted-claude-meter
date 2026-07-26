//! The two claude.ai endpoints the app calls, plus a control plane for
//! changing what they return while the app is running.
//!
//! Mirrors `crates/meter-api/src/client.rs`:
//!   * `GET {base}/organizations`              — session validation + org discovery
//!   * `GET {base}/organizations/{id}/usage`   — the usage payload
//!
//! Both are mounted at the root *and* under `/api`, so `RCM_API_BASE_URL`
//! works whether or not you copy the `/api` suffix from `DEFAULT_BASE_URL`.

use std::sync::{Arc, Mutex, PoisonError};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::scenario::{ScenarioError, Scenarios};

/// A failure the server injects instead of serving data, so the app's error
/// paths (`FetchOutcome::Unauthorized` / `Transient`, the tray's stale-data
/// handling, the wizard's rejection copy) can be driven on demand.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FailureMode {
    #[default]
    None,
    /// 401 — an expired session key. Pauses the app's polling loop.
    Unauthorized,
    /// 403 — claude.ai's bot block, which the app classifies separately.
    Forbidden,
    ServerError,
    /// Accept the connection and never answer, to exercise the client timeout.
    Hang,
}

pub struct AppState {
    scenarios: Scenarios,
    active: Mutex<String>,
    failure: Mutex<FailureMode>,
}

impl AppState {
    pub fn new(scenarios: Scenarios, initial: String) -> Self {
        Self {
            scenarios,
            active: Mutex::new(initial),
            failure: Mutex::new(FailureMode::None),
        }
    }

    fn active_scenario(&self) -> String {
        self.active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn failure_mode(&self) -> FailureMode {
        *self.failure.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/organizations", get(organizations))
        .route("/organizations/{org_id}/usage", get(usage));
    Router::new()
        .merge(api.clone())
        .nest("/api", api)
        .route("/__admin/state", get(admin_state))
        .route("/__admin/scenarios", get(admin_scenarios))
        .route("/__admin/scenario", put(set_scenario))
        .route("/__admin/failure", put(set_failure))
        .with_state(state)
}

/// Reject a request with no `sessionKey` cookie, exactly as claude.ai would.
///
/// Keeping this honest matters: it is what makes the wizard's "paste a key"
/// flow a real test rather than a formality. Any well-formed key is accepted —
/// the harness deliberately has no notion of a *correct* key, so no real
/// credential is ever needed.
fn authenticated(headers: &HeaderMap) -> bool {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| value.contains("sessionKey=sk-ant-"))
}

/// Everything both data endpoints do before serving: apply the injected
/// failure mode, then require a session cookie. One place, so the two can
/// never drift out of order.
///
/// `Some(response)` means "answer with this instead". `FailureMode::Hang`
/// never returns at all.
async fn guard(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    match state.failure_mode() {
        FailureMode::None => {}
        FailureMode::Unauthorized => return Some(StatusCode::UNAUTHORIZED.into_response()),
        FailureMode::Forbidden => return Some(StatusCode::FORBIDDEN.into_response()),
        FailureMode::ServerError => {
            return Some(StatusCode::INTERNAL_SERVER_ERROR.into_response());
        }
        FailureMode::Hang => std::future::pending::<()>().await,
    }
    (!authenticated(headers)).then(|| StatusCode::UNAUTHORIZED.into_response())
}

async fn organizations(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if let Some(response) = guard(&state, &headers).await {
        return response;
    }
    axum::Json(serde_json::json!([{
        "uuid": "demo-org-0000-0000-0000-000000000000",
        "name": "Demo Org",
    }]))
    .into_response()
}

async fn usage(
    State(state): State<Arc<AppState>>,
    Path(_org_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = guard(&state, &headers).await {
        return response;
    }
    let name = state.active_scenario();
    match state.scenarios.render(&name, Timestamp::now()) {
        Ok(body) => axum::Json(body).into_response(),
        Err(error) => {
            eprintln!("scenario `{name}`: {error}");
            let status = match error {
                ScenarioError::NotFound(_) => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (status, error.to_string()).into_response()
        }
    }
}

#[derive(Serialize)]
struct AdminState {
    scenario: String,
    failure: FailureMode,
    available: Vec<String>,
}

async fn admin_state(State(state): State<Arc<AppState>>) -> Response {
    axum::Json(AdminState {
        scenario: state.active_scenario(),
        failure: state.failure_mode(),
        available: state.scenarios.available(),
    })
    .into_response()
}

async fn admin_scenarios(State(state): State<Arc<AppState>>) -> Response {
    axum::Json(state.scenarios.available()).into_response()
}

#[derive(Deserialize)]
struct ScenarioRequest {
    name: String,
}

async fn set_scenario(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<ScenarioRequest>,
) -> Response {
    if !state.scenarios.exists(&request.name) {
        return (
            StatusCode::NOT_FOUND,
            format!("no scenario named `{}`", request.name),
        )
            .into_response();
    }
    *state.active.lock().unwrap_or_else(PoisonError::into_inner) = request.name.clone();
    eprintln!("scenario -> {}", request.name);
    admin_state(State(state)).await
}

#[derive(Deserialize)]
struct FailureRequest {
    mode: FailureMode,
}

async fn set_failure(
    State(state): State<Arc<AppState>>,
    axum::Json(request): axum::Json<FailureRequest>,
) -> Response {
    *state.failure.lock().unwrap_or_else(PoisonError::into_inner) = request.mode;
    eprintln!("failure -> {:?}", request.mode);
    admin_state(State(state)).await
}
