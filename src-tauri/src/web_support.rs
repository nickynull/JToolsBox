//! 内嵌网页应用的通用托管：构建期把 `web/<name>/dist` 打进二进制，
//! 运行期按功能路由静态资源。各功能的网页互不共享，只在托管方式上复用这里。

use axum::{
    body::Body,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};

pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

pub struct WebApp {
    assets: &'static [(&'static str, &'static [u8])],
    index: &'static [u8],
}

impl WebApp {
    pub const fn new(
        assets: &'static [(&'static str, &'static [u8])],
        index: &'static [u8],
    ) -> Self {
        Self { assets, index }
    }

    pub fn asset(&self, url_path: &str) -> Option<&'static [u8]> {
        self.assets
            .iter()
            .find(|(path, _)| *path == url_path)
            .map(|(_, bytes)| *bytes)
    }

    pub fn index_response(&self) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(Body::from(self.index))
            .unwrap()
    }

    pub fn asset_response(&self, url_path: &str) -> Response {
        match self.asset(url_path) {
            Some(bytes) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime_type(url_path))
                .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                .header(header::CONTENT_LENGTH, bytes.len())
                .body(Body::from(bytes))
                .unwrap(),
            None => (StatusCode::NOT_FOUND, "Not found").into_response(),
        }
    }
}

fn mime_type(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
