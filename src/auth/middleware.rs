use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use super::OAuthState;
use super::helpers::www_authenticate;

pub async fn bearer_auth_middleware(
    state: OAuthState,
    req: Request,
    next: Next,
) -> Response {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(h) if h.len() > 7 && h[..7].eq_ignore_ascii_case("bearer ") => &h[7..],
        _ => {
            return Response::builder()
                .status(401)
                .header("WWW-Authenticate", www_authenticate(&state.server_url))
                .body(Body::from("Unauthorized"))
                .unwrap();
        }
    };

    if state.validate_access_token(token) {
        next.run(req).await
    } else {
        Response::builder()
            .status(401)
            .header("WWW-Authenticate", www_authenticate(&state.server_url))
            .body(Body::from("Unauthorized"))
            .unwrap()
    }
}
