//! The HTTP surface: the page, pairing, and the read endpoints.
//!
//! Every route but `/`, `/static/{file}` and `POST /api/pair` goes through the
//! [`Auth`] extractor, so a handler that forgets the check does not compile —
//! the device is an argument, not something to remember to look up.

use super::hub::{Device, PairAttempt, PairOutcome};
use super::media;
use super::server::{static_file, Srv};
use crate::headshots::ImageCache;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{header, request::Parts, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::net::SocketAddr;
use std::sync::Arc;

pub fn router(srv: Arc<Srv>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/static/{file}", get(static_route))
        .route("/api/pair", post(pair))
        .route("/api/state", get(draft_state))
        .route("/api/season", get(season))
        .route("/api/config", get(config))
        .route("/api/devices", get(devices))
        .route("/api/headshot/{player_id}", get(headshot))
        .route("/api/avatar/{reference}", get(avatar))
        .route(
            "/api/chat",
            get(super::routes_chat::get_chat).post(super::routes_chat::post_chat),
        )
        .route("/api/events", get(super::ws::events))
        .layer(axum::middleware::from_fn_with_state(srv.clone(), gate))
        .with_state(srv)
}

/// What the phone page is allowed to load and talk to.
///
/// Everything the page needs comes from this server: its own scripts and
/// stylesheet, the pictures under `/api/headshot` and `/api/avatar` (same
/// origin, and `data:` for the placeholder), and the WebSocket. Nothing else
/// is reachable, so a string that somehow became markup has nowhere to send
/// what it found and no third-party script to run.
pub const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
     img-src 'self' data:; connect-src 'self' ws: wss:; base-uri 'none'; form-action 'none'";

/// CORS, the page's security headers, and the cross-origin check.
///
/// The follower desktop's webview is its own origin (`tauri://localhost`), and
/// so is the Vite dev server; without the CORS headers a browser refuses to
/// hand either the response, and the preflight it sends first would be a 405.
/// Reads stay open to any origin — the bearer token is the whole access
/// control and no cookie is ever set — but a request that *changes* something
/// and names an origin has to name one of ours, so a page the phone has open
/// in another tab cannot post to the host in the background.
async fn gate(
    State(srv): State<Arc<Srv>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let allow = |mut response: Response| {
        let headers = response.headers_mut();
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            "*".parse().expect("static"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_HEADERS,
            "authorization, content-type".parse().expect("static"),
        );
        headers.insert(
            header::ACCESS_CONTROL_ALLOW_METHODS,
            "GET, POST, OPTIONS".parse().expect("static"),
        );
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            CSP.parse().expect("static"),
        );
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            "nosniff".parse().expect("static"),
        );
        headers.insert(
            header::REFERRER_POLICY,
            "no-referrer".parse().expect("static"),
        );
        response
    };
    if request.method() == axum::http::Method::OPTIONS {
        return allow(StatusCode::NO_CONTENT.into_response());
    }
    if changes_something(request.method()) && !origin_is_ours(&request, &srv) {
        return allow(fail(StatusCode::FORBIDDEN, "that page cannot post here"));
    }
    allow(next.run(request).await)
}

fn changes_something(method: &axum::http::Method) -> bool {
    !matches!(
        *method,
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    )
}

/// Whether the `Origin` on a state-changing request is one of ours. A request
/// with no `Origin` header is not a browser making a cross-site request, and
/// is left to the bearer token as before.
fn origin_is_ours(request: &axum::extract::Request, srv: &Srv) -> bool {
    let Some(origin) = request.headers().get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    super::net::origin_allowed(origin, srv.hub.port())
}

/// Every failure this server produces: a status and `{ "error": … }`, which
/// is the one shape the phone page has to know how to read.
pub fn fail(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

/// A request that carries a token belonging to a paired device.
pub struct Auth(pub Device);

impl axum::extract::FromRequestParts<Arc<Srv>> for Auth {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        srv: &Arc<Srv>,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer(parts).unwrap_or_default();
        srv.hub
            .device_for(&token)
            .map(Auth)
            .ok_or_else(|| fail(StatusCode::UNAUTHORIZED, "not paired"))
    }
}

/// The token out of an `Authorization: Bearer …` header.
pub fn bearer(parts: &Parts) -> Option<String> {
    let raw = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = raw.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    Some(token.trim().to_string())
}

async fn index() -> Response {
    html(super::server::INDEX_HTML)
}

fn html(body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

async fn static_route(Path(file): Path<String>) -> Response {
    match static_file(&file) {
        Some((mime, body)) => ([(header::CONTENT_TYPE, mime)], body).into_response(),
        None => fail(StatusCode::NOT_FOUND, "no such file"),
    }
}

#[derive(Deserialize)]
struct PairRequest {
    code: String,
    #[serde(default)]
    device_name: String,
    #[serde(default)]
    kind: String,
    /// The id this client was given the last time it paired, when it has one.
    /// Only this replaces an existing entry; a device that cannot say which
    /// one it was is a new device, whatever it calls itself.
    #[serde(default)]
    device_id: Option<String>,
}

async fn pair(
    State(srv): State<Arc<Srv>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<PairRequest>,
) -> Response {
    let attempt = PairAttempt {
        code: &body.code,
        name: &body.device_name,
        kind: &body.kind,
        peer: peer.ip(),
        previous_device_id: body.device_id.as_deref(),
    };
    match srv.hub.pair(attempt) {
        Ok(PairOutcome::Ok {
            token,
            device_id,
            host_name,
        }) => Json(serde_json::json!({
            "token": token,
            "host_name": host_name,
            "device_id": device_id,
        }))
        .into_response(),
        Ok(PairOutcome::WrongCode) => fail(StatusCode::FORBIDDEN, "wrong code"),
        Ok(PairOutcome::LockedOut) => fail(
            StatusCode::TOO_MANY_REQUESTS,
            "too many wrong codes — wait a minute and try again",
        ),
        // The only way this fails is the machine's random source, and a token
        // that is not random is worse than no pairing at all.
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

async fn draft_state(State(srv): State<Arc<Srv>>, _auth: Auth) -> Response {
    let loaded = srv.state.loaded.lock().await;
    let Some(loaded) = loaded.as_ref() else {
        return fail(StatusCode::NOT_FOUND, "no league loaded");
    };
    let config = srv.state.config.lock().await;
    Json(crate::state::view_from(loaded, &config)).into_response()
}

async fn season(State(srv): State<Arc<Srv>>, _auth: Auth) -> Response {
    // The same view the desktop's own chat answers from: reused when the
    // season screen has built one recently, and otherwise built off the
    // runtime thread. A phone refreshing must not stall the pollers.
    match crate::state::season_view_for_chat(
        &srv.state.loaded,
        &srv.state.season,
        &srv.state.config,
        &srv.state.last_season_view,
    )
    .await
    {
        Ok(view) => Json(&*view).into_response(),
        Err(e) => fail(StatusCode::NOT_FOUND, &e),
    }
}

/// The settings a follower needs, assembled field by field.
///
/// Deliberately *not* a serialisation of [`crate::engine::AppConfig`]: that
/// struct carries the Anthropic key and the running spend, and a future field
/// added to it would be published to every phone on the network by a route
/// nobody thought to re-read. What goes out is only ever what is listed here.
async fn config(State(srv): State<Arc<Srv>>, _auth: Auth) -> Response {
    let config = srv.state.config.lock().await;
    let active = config.active_league_id.clone();
    let platform = active
        .as_ref()
        .and_then(|id| config.leagues.iter().find(|l| &l.league_id == id))
        .map(|l| l.platform.clone())
        .unwrap_or_else(|| crate::view_types::SLEEPER.to_string());
    Json(serde_json::json!({
        "active_league_id": active,
        "leagues": config.leagues,
        "my_user_id": config.my_user_id,
        "host_name": srv.hub.host_name(),
        "platform": platform,
    }))
    .into_response()
}

async fn devices(State(srv): State<Arc<Srv>>, _auth: Auth) -> Response {
    Json(srv.hub.devices()).into_response()
}

async fn headshot(
    State(srv): State<Arc<Srv>>,
    _auth: Auth,
    Path(player_id): Path<String>,
) -> Response {
    image(srv.state.engine.headshot(&player_id).await)
}

#[derive(Deserialize)]
struct AvatarQuery {
    #[serde(default)]
    full: Option<String>,
}

async fn avatar(
    State(srv): State<Arc<Srv>>,
    _auth: Auth,
    Path(reference): Path<String>,
    Query(query): Query<AvatarQuery>,
) -> Response {
    let full = matches!(query.full.as_deref(), Some("1") | Some("true"));
    image(srv.state.engine.avatar(&reference, full).await)
}

/// Turn what the image cache answered into bytes on the wire.
///
/// A picture nobody has is a 404 rather than an error: the phone draws its
/// placeholder and stops asking, which is what a missing headshot should cost.
fn image(cached: Result<Option<String>, String>) -> Response {
    let Ok(Some(url)) = cached else {
        return fail(StatusCode::NOT_FOUND, "no picture");
    };
    let Some((mime, bytes)) = media::decode_data_url(&url) else {
        return fail(StatusCode::NOT_FOUND, "no picture");
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            // The cache behind this is keyed by a content hash on Sleeper's
            // side; a picture that has been fetched once does not change.
            (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::bearer;
    use axum::http::{HeaderValue, Request};

    fn parts(header: Option<&str>) -> axum::http::request::Parts {
        let mut request = Request::builder().uri("/api/state");
        if let Some(value) = header {
            request = request.header(
                "authorization",
                HeaderValue::from_str(value).expect("a valid header"),
            );
        }
        request.body(()).expect("the request builds").into_parts().0
    }

    #[test]
    fn a_bearer_token_is_read_out_of_the_header_and_nothing_else_is() {
        assert_eq!(
            bearer(&parts(Some("Bearer abc123"))).as_deref(),
            Some("abc123")
        );
        // Case of the scheme is not the client's problem.
        assert_eq!(
            bearer(&parts(Some("bearer abc123"))).as_deref(),
            Some("abc123")
        );
        assert_eq!(bearer(&parts(None)), None);
        assert_eq!(bearer(&parts(Some("abc123"))), None);
        assert_eq!(bearer(&parts(Some("Basic abc123"))), None);
    }
}
