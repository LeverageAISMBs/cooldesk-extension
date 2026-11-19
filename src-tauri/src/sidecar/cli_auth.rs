//! Token auth for the `/cli/*` namespace.
//!
//! The rest of the sidecar accepts any request that carries no Origin header
//! (`server.rs::is_allowed_origin`), so any local process can read and write
//! app data. `/cli/*` is where command-line control of the app is exposed, so
//! it requires a bearer token:
//!
//! * A fresh 256-bit token is generated from the OS RNG at every start and
//!   written atomically to `<local data dir>/com.cooldesk.desktop/cli-token`.
//!   That directory is per-user. On unix the file is also chmod 0600.
//! * Requests need `Authorization: Bearer <token>`. The comparison is constant-time.
//! * Requests that carry an `Origin` header are refused. Browsers always send
//!   one on cross-origin requests and the CLI never does, so a web page can't
//!   reach these routes even if it somehow has the token.
//! * Fail closed: if the token can't be created, `router()` returns `None` and
//!   no `/cli/*` route is mounted. An empty expected token never matches.

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use rand::RngCore;
use std::path::PathBuf;
use std::sync::Arc;

/// Same directory Tauri resolves for `app_local_data_dir()` with this app's identifier.
pub const APP_IDENTIFIER: &str = "com.cooldesk.desktop";
pub const TOKEN_FILE: &str = "cli-token";

pub fn token_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(APP_IDENTIFIER).join(TOKEN_FILE))
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write `token` to `path` atomically (temp file, then rename).
pub fn write_token(path: &std::path::Path, token: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.is_empty() || a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Clone)]
pub struct CliToken(Arc<String>);

async fn require_token(State(expected): State<CliToken>, req: Request, next: Next) -> Response {
    if req.headers().contains_key(header::ORIGIN) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({ "error": "browser origins are not allowed on /cli" })))
            .into_response();
    }
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if !constant_time_eq(expected.0.as_bytes(), presented.as_bytes()) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "missing or invalid CLI token" })))
            .into_response();
    }
    next.run(req).await
}

async fn cli_ping() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

/// Build the `/cli` router around `token`. Generic over the outer router's state
/// so it merges into the sidecar app.
pub fn router_with_token<S: Clone + Send + Sync + 'static>(token: String) -> Router<S> {
    Router::new()
        .route("/cli/ping", get(cli_ping))
        .route("/cli/commands", get(crate::sidecar::cli_invoke::commands_handler))
        .route("/cli/invoke", axum::routing::post(crate::sidecar::cli_invoke::invoke_handler))
        .route_layer(middleware::from_fn_with_state(CliToken(Arc::new(token)), require_token))
}

/// A token that's in memory and not yet on disk. `persist()` is called only
/// once the listener is bound, so the token file never points at a server
/// that failed to start.
pub struct PendingToken {
    path: PathBuf,
    token: String,
}

impl PendingToken {
    /// If the write fails, the routes stay mounted with a token nobody can
    /// read, so they're still closed.
    pub fn persist(self) {
        match write_token(&self.path, &self.token) {
            Ok(()) => log::info!("[Sidecar] /cli enabled; token at {}", self.path.display()),
            Err(e) => log::error!("[Sidecar] /cli unreachable: cannot write token {}: {e}", self.path.display()),
        }
    }
}

/// Generate the token and the gated router. Returns `None` (nothing mounted)
/// when there's no local data directory to put the token in.
pub fn prepare<S: Clone + Send + Sync + 'static>() -> Option<(Router<S>, PendingToken)> {
    let Some(path) = token_path() else {
        log::error!("[Sidecar] /cli disabled: no local data directory");
        return None;
    };
    let token = generate_token();
    Some((router_with_token(token.clone()), PendingToken { path, token }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    fn app(token: &str) -> Router {
        Router::new()
            .route("/health", get(|| async { "ok" }))
            .merge(router_with_token::<()>(token.to_string()))
    }

    async fn status(app: Router, req: HttpRequest<Body>) -> StatusCode {
        app.oneshot(req).await.unwrap().status()
    }

    fn ping() -> axum::http::request::Builder {
        HttpRequest::builder().uri("/cli/ping")
    }

    #[tokio::test]
    async fn no_header_is_401() {
        assert_eq!(status(app("t0k"), ping().body(Body::empty()).unwrap()).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_token_is_401() {
        let req = ping().header("authorization", "Bearer nope").body(Body::empty()).unwrap();
        assert_eq!(status(app("t0k"), req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn non_bearer_scheme_is_401() {
        let req = ping().header("authorization", "t0k").body(Body::empty()).unwrap();
        assert_eq!(status(app("t0k"), req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn right_token_is_200() {
        let req = ping().header("authorization", "Bearer t0k").body(Body::empty()).unwrap();
        assert_eq!(status(app("t0k"), req).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn right_token_with_origin_is_403() {
        let req = ping()
            .header("authorization", "Bearer t0k")
            .header("origin", "chrome-extension://abc")
            .body(Body::empty())
            .unwrap();
        assert_eq!(status(app("t0k"), req).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn empty_expected_token_never_matches() {
        let req = ping().header("authorization", "Bearer ").body(Body::empty()).unwrap();
        assert_eq!(status(app(""), req).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unknown_cli_route_is_404_not_open() {
        let req = HttpRequest::builder().uri("/cli/nope").body(Body::empty()).unwrap();
        // route_layer only wraps matched routes; an unmatched path must not succeed.
        assert_ne!(status(app("t0k"), req).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn commands_and_invoke_are_behind_the_token() {
        let a = app("t0k");
        let req = HttpRequest::builder().uri("/cli/commands").body(Body::empty()).unwrap();
        assert_eq!(status(a.clone(), req).await, StatusCode::UNAUTHORIZED);
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/cli/invoke")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"cmd":"get_focused_app"}"#))
            .unwrap();
        assert_eq!(status(a.clone(), req).await, StatusCode::UNAUTHORIZED);
        let req = HttpRequest::builder().uri("/cli/commands").header("authorization", "Bearer t0k").body(Body::empty()).unwrap();
        assert_eq!(status(a, req).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn invoke_status_codes() {
        let post = |body: &'static str| {
            HttpRequest::builder()
                .method("POST")
                .uri("/cli/invoke")
                .header("authorization", "Bearer t0k")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap()
        };
        let a = app("t0k");
        assert_eq!(status(a.clone(), post(r#"{"cmd":"nope"}"#)).await, StatusCode::NOT_FOUND);
        assert_eq!(status(a.clone(), post(r#"{"cmd":"close_app","args":{"pid":1}}"#)).await, StatusCode::PRECONDITION_REQUIRED);
        assert_eq!(status(a.clone(), post(r#"{"cmd":"launch_app","args":{}}"#)).await, StatusCode::BAD_REQUEST);
        assert_eq!(status(a.clone(), post(r#"not json"#)).await, StatusCode::BAD_REQUEST);
        // No AppHandle in tests: commands that need one report 503 instead of panicking.
        assert_eq!(status(a.clone(), post(r#"{"cmd":"app_windows_list"}"#)).await, StatusCode::SERVICE_UNAVAILABLE);
        // hwnd 0 returns [] without touching the OS: a harmless 200 end to end.
        assert_eq!(status(a, post(r#"{"cmd":"list_window_tabs","args":{"hwnd":0}}"#)).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn health_stays_open() {
        let req = HttpRequest::builder().uri("/health").body(Body::empty()).unwrap();
        assert_eq!(status(app("t0k"), req).await, StatusCode::OK);
    }

    #[test]
    fn token_file_roundtrip_and_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join(TOKEN_FILE);
        let a = generate_token();
        assert_eq!(a.len(), 64);
        write_token(&p, &a).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), a);
        let b = generate_token();
        assert_ne!(a, b, "each start must rotate the token");
        write_token(&p, &b).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), b);
        assert!(!p.with_extension("tmp").exists());
    }

    #[test]
    fn pending_token_is_not_on_disk_until_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(TOKEN_FILE);
        let pending = PendingToken { path: path.clone(), token: generate_token() };
        assert!(!path.exists(), "nothing on disk before bind");
        let expected = pending.token.clone();
        pending.persist();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    }
}
