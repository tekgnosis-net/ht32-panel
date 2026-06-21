//! JSON template-editor API (`/api/...`). Distinct from the HTML-partial routes.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::dbus::DaemonSignals;
use crate::faces::template::spec::TemplateSpec;
use crate::web::WebState;

/// Maps an `anyhow::Error` from AppState into a 400 with its message.
fn bad_request(e: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, e.to_string()).into_response()
}

/// GET /api/templates -> ["name", ...]
async fn list(State(st): State<WebState>) -> Json<Vec<String>> {
    Json(st.app.template_names())
}

/// POST /api/templates  (body: TemplateSpec) -> 200 {"name":...} | 400
async fn create(State(st): State<WebState>, Json(spec): Json<TemplateSpec>) -> Response {
    match st.app.save_template(&spec) {
        Ok(()) => {
            let _ = st.signal_tx.send(DaemonSignals::DisplaySettingsChanged);
            Json(serde_json::json!({ "name": spec.name })).into_response()
        }
        Err(e) => bad_request(e),
    }
}

/// GET /api/templates/:name -> TemplateSpec | 404
async fn get_one(State(st): State<WebState>, Path(name): Path<String>) -> Response {
    match st.app.load_template_spec(&name) {
        Some(spec) => Json(spec).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            format!("template '{name}' not found"),
        )
            .into_response(),
    }
}

/// PUT /api/templates/:name  (body: TemplateSpec) -> 200 | 400
async fn update(
    State(st): State<WebState>,
    Path(name): Path<String>,
    Json(mut spec): Json<TemplateSpec>,
) -> Response {
    spec.name = name; // the URL is authoritative for the file name
    match st.app.save_template(&spec) {
        Ok(()) => {
            let _ = st.signal_tx.send(DaemonSignals::DisplaySettingsChanged);
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Err(e) => bad_request(e),
    }
}

/// DELETE /api/templates/:name -> 204 | 400 (refused if active)
async fn delete(State(st): State<WebState>, Path(name): Path<String>) -> Response {
    match st.app.delete_template(&name) {
        Ok(()) => {
            let _ = st.signal_tx.send(DaemonSignals::DisplaySettingsChanged);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => bad_request(e),
    }
}

#[derive(Deserialize)]
struct CloneBody {
    new_name: String,
}

/// POST /api/templates/:name/clone  (body: {"new_name":...}) -> 200 {"name":...} | 400
async fn clone(
    State(st): State<WebState>,
    Path(name): Path<String>,
    Json(b): Json<CloneBody>,
) -> Response {
    match st.app.clone_template(&name, &b.new_name) {
        Ok(()) => {
            let _ = st.signal_tx.send(DaemonSignals::DisplaySettingsChanged);
            Json(serde_json::json!({"name": b.new_name})).into_response()
        }
        Err(e) => bad_request(e),
    }
}

/// Router for the JSON API. Schema + preview routes are added by later tasks.
pub fn api_router() -> Router<WebState> {
    Router::new()
        .route("/api/templates", get(list).post(create))
        .route(
            "/api/templates/:name",
            get(get_one).put(update).delete(delete),
        )
        .route("/api/templates/:name/clone", post(clone))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt; // oneshot

    fn test_state() -> (tempfile::TempDir, WebState) {
        let dir = tempfile::tempdir().unwrap();
        let app = std::sync::Arc::new(crate::state::AppState::for_tests(dir.path()));
        let (tx, _rx) = tokio::sync::broadcast::channel(16);
        (dir, WebState { app, signal_tx: tx })
    }

    #[tokio::test]
    async fn post_then_get_template() {
        let (_d, st) = test_state();
        let router = super::api_router().with_state(st);
        let body = r#"{"name":"web_made","widgets":[]}"#;
        let resp = router
            .clone()
            .oneshot(
                Request::post("/api/templates")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = router
            .oneshot(
                Request::get("/api/templates/web_made")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn post_bad_name_is_400() {
        let (_d, st) = test_state();
        let router = super::api_router().with_state(st);
        let resp = router
            .oneshot(
                Request::post("/api/templates")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"../evil","widgets":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
