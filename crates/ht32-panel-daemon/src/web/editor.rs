//! The `/editor` page and its embedded static assets (no build step).

use askama::Template;
use axum::{
    http::header,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};

use crate::web::WebState;

#[derive(Template)]
#[template(path = "editor.html")]
struct EditorTemplate;

async fn page() -> impl IntoResponse {
    Html(EditorTemplate.render().unwrap())
}

const EDITOR_JS: &str = include_str!("../../assets/editor.js");
const WIDGETS_JS: &str = include_str!("../../assets/widgets.js");
const EDITOR_CSS: &str = include_str!("../../assets/editor.css");
const ALPINE_JS: &str = include_str!("../../assets/alpine.min.js");

async fn editor_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        EDITOR_JS,
    )
}
async fn widgets_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        WIDGETS_JS,
    )
}
async fn editor_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css")], EDITOR_CSS)
}
async fn alpine_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/javascript")],
        ALPINE_JS,
    )
}

/// Router for the editor page + assets.
pub fn editor_router() -> Router<WebState> {
    Router::new()
        .route("/editor", get(page))
        .route("/editor/editor.js", get(editor_js))
        .route("/editor/widgets.js", get(widgets_js))
        .route("/editor/editor.css", get(editor_css))
        .route("/editor/alpine.js", get(alpine_js))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    fn st() -> WebState {
        let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
        let app = std::sync::Arc::new(crate::state::AppState::for_tests(dir.path()));
        let (tx, _rx) = tokio::sync::broadcast::channel(8);
        WebState { app, signal_tx: tx }
    }

    #[tokio::test]
    async fn editor_page_and_assets_serve() {
        let r = editor_router().with_state(st());
        for path in [
            "/editor",
            "/editor/editor.js",
            "/editor/editor.css",
            "/editor/widgets.js",
            "/editor/alpine.js",
        ] {
            let resp = r
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::OK, "{path} should 200");
        }
    }
}
