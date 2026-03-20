use std::borrow::Cow;

use axum::Router;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rust_embed::RustEmbed;

use super::common_routes;

pub(crate) fn router() -> Router {
    let router = Router::new().route("/", get(index_handler));
    let router = common_routes(router);

    router
        .route("/{*file}", get(static_handler))
        .fallback(index_handler)
}

async fn index_handler() -> impl IntoResponse {
    StaticFile(Cow::Borrowed("index.html"))
}

async fn static_handler(uri: Uri) -> impl IntoResponse {
    StaticFile(Cow::Owned(uri.path().trim_start_matches('/').to_string()))
}

#[derive(RustEmbed)]
#[folder = "../../dist"]
struct Asset;

pub struct StaticFile(Cow<'static, str>);

impl IntoResponse for StaticFile {
    fn into_response(self) -> Response {
        let Some(content) = Asset::get(self.0.as_ref()) else {
            let Some(content) = Asset::get("index.html") else {
                return (StatusCode::NOT_FOUND, "404 Not Found").into_response();
            };

            let mime = mime_guess::from_path("index.html").first_or_octet_stream();
            return ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response();
        };

        let mime = mime_guess::from_path(self.0.as_ref()).first_or_octet_stream();
        ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
    }
}
