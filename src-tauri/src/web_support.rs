//! 网页应用托管：运行期直接从磁盘目录读取，不再编译期嵌入。
//!
//! 资源位置是固定的，不提供任何覆盖入口（模块化隔离，按功能名分子目录）：
//!
//! 1. 仅 debug 构建：仓库内 `web/<功能名>/dist`（开发期产物总是最新，优先命中，
//!    改完网页端只需重建 dist + 刷新，不会被编译期复制到 target 的旧快照抢先）
//! 2. `<程序资产根>/webapps/<功能名>/`（随包分发，只读；正常发布只读这一级）
//!
//! 资产与用户数据分开：`function-datas/<功能名>/` 只放配置、用户与日志，
//! 网页资源不会写进去。

use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::path::{Path, PathBuf};
use tokio_util::io::ReaderStream;

/// 某功能模块的网页资源候选目录，按优先级排列
pub fn candidates(feature: &str, app_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut list = Vec::new();

    // 开发期优先：debug 构建优先读仓库里的构建产物。
    // Tauri 编译时会把 bundle.resources 复制到 target/debug/webapps，
    // 那份快照只在 Rust 重编时才更新；若它抢先命中，改完网页端
    // （不重编 Rust）页面就永远是旧内容，所以 debug 下把它排到后面。
    #[cfg(debug_assertions)]
    {
        list.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../web")
                .join(feature)
                .join("dist"),
        );
    }

    if let Some(dir) = app_dir {
        list.push(dir.join("webapps").join(feature));
    }

    list
}

/// 一个功能模块的网页资源挂载点
#[derive(Clone)]
pub struct WebMount {
    /// 功能名，仅用于引导页文案
    feature: String,
    /// 命中的资源根（已 canonicalize）；None 表示所有候选都没有 index.html
    root: Option<PathBuf>,
    /// 检查过的候选目录，用于引导页和桌面端提示
    candidates: Vec<PathBuf>,
}

impl WebMount {
    pub fn resolve(feature: &str, app_dir: Option<&Path>) -> Self {
        let candidates = candidates(feature, app_dir);
        // canonicalize 一次，后续请求比较前缀时不会被符号链接干扰
        let root = candidates
            .iter()
            .find(|dir| dir.join("index.html").is_file())
            .and_then(|dir| dir.canonicalize().ok());

        Self {
            feature: feature.to_string(),
            root,
            candidates,
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn candidates(&self) -> &[PathBuf] {
        &self.candidates
    }

    pub fn is_resolved(&self) -> bool {
        self.root.is_some()
    }

    /// 请求资源：命中返回文件内容，未命中返回 404；资源根本身缺失时返回引导页
    pub async fn response(&self, url_path: &str, headers: &HeaderMap) -> Response {
        match self.root.as_deref() {
            Some(root) => file_response(root, url_path, headers).await,
            None => self.guide_page(),
        }
    }

    /// 只判断某路径是否存在对应文件（favicon 这类可选资源用）
    pub async fn exists(&self, url_path: &str) -> bool {
        let Some(root) = self.root.as_deref() else {
            return false;
        };
        let Some(path) = safe_join(root, url_path) else {
            return false;
        };
        matches!(tokio::fs::metadata(&path).await, Ok(meta) if meta.is_file())
    }

    /// 资源目录缺失时的引导页，替代以前由 build.rs 生成的编译期占位页
    pub fn guide_page(&self) -> Response {
        let mut items = String::new();
        for dir in &self.candidates {
            items.push_str(&format!(
                "<li><code>{}</code></li>",
                escape_html(&dir.display().to_string())
            ));
        }

        let html = format!(
            "<!DOCTYPE html><html lang=\"zh-CN\"><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
             <title>网页资源未找到</title>\
             <body style=\"margin:0;font-family:system-ui,-apple-system,'Segoe UI',sans-serif;\
             background:#1e1e1e;color:#d4d4d4;display:flex;align-items:center;justify-content:center;\
             min-height:100vh;padding:24px;box-sizing:border-box\">\
             <div style=\"max-width:720px;line-height:1.7\">\
             <h2 style=\"margin:0 0 12px;font-size:20px;color:#f0f0f0\">「{feature}」网页资源未找到</h2>\
             <p style=\"margin:0 0 12px\">以下目录里都没有 <code>index.html</code>：</p>\
             <ul style=\"margin:0 0 16px;padding-left:20px\">{items}</ul>\
             <p style=\"margin:0 0 8px\">请把该功能的网页构建产物放到上述任一目录，或重新安装包含 \
             <code>webapps/{feature}</code> 的完整包。</p>\
             <p style=\"margin:0;color:#8a8a8a;font-size:13px\">开发期可在项目根目录运行 \
             <code>npm run build:web:{feature}</code> 后刷新。</p>\
             </div></body></html>",
            feature = escape_html(&self.feature),
            items = items
        );

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(Body::from(html))
            .unwrap()
    }
}

/// 拼出目标文件路径，同时挡掉路径穿越
fn safe_join(root: &Path, url_path: &str) -> Option<PathBuf> {
    let rel = url_path.trim_start_matches('/');
    if rel.is_empty() || rel.contains('\\') || rel.contains('\0') {
        return None;
    }

    let mut out = root.to_path_buf();
    for segment in rel.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return None,
            name => out.push(name),
        }
    }
    Some(out)
}

async fn file_response(root: &Path, url_path: &str, headers: &HeaderMap) -> Response {
    let Some(path) = safe_join(root, url_path) else {
        return not_found();
    };

    // canonicalize 之后必须仍在资源根内，符号链接跳出去也会被挡住
    let Ok(canonical) = tokio::fs::canonicalize(&path).await else {
        return not_found();
    };
    if !canonical.starts_with(root) {
        return not_found();
    }

    let Ok(meta) = tokio::fs::metadata(&canonical).await else {
        return not_found();
    };
    if !meta.is_file() {
        return not_found();
    }

    let etag = build_etag(meta.len(), meta.modified().ok());

    if let Some(raw) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
    {
        if raw
            .split(',')
            .any(|part| part.trim().trim_start_matches("W/") == etag)
        {
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .header(header::CACHE_CONTROL, CACHE_CONTROL)
                .body(Body::empty())
                .unwrap();
        }
    }

    let Ok(file) = tokio::fs::File::open(&canonical).await else {
        return not_found();
    };

    // 网页资源是手工可替换的外部文件，不能用 immutable：否则同名文件换了浏览器也不刷新
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime_type(url_path))
        .header(header::CACHE_CONTROL, CACHE_CONTROL)
        .header(header::ETAG, &etag)
        .header(header::CONTENT_LENGTH, meta.len())
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from_stream(ReaderStream::new(file)))
        .unwrap()
}

const CACHE_CONTROL: &str = "no-cache";

fn build_etag(len: u64, modified: Option<std::time::SystemTime>) -> String {
    let secs = modified
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("\"{:x}-{:x}\"", len, secs)
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

fn escape_html(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_traversal() {
        let root = Path::new("/srv/webapps/file-sharing");
        assert!(safe_join(root, "/assets/app.js").is_some());
        assert!(safe_join(root, "/assets/../config.json").is_none());
        assert!(safe_join(root, "/assets\\..\\config.json").is_none());
        assert!(safe_join(root, "/").is_none());
    }

    #[test]
    fn candidates_start_with_app_dir() {
        let list = candidates("file-sharing", Some(Path::new("/opt/app")));
        let app_entry = PathBuf::from("/opt/app/webapps/file-sharing");
        // 两种构建下 app_dir 对应的候选都必须存在
        assert!(list.contains(&app_entry));

        // debug 优先仓库产物（总是最新）；release 只有随包资源
        #[cfg(debug_assertions)]
        assert_eq!(
            list[0],
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/file-sharing/dist")
        );
        #[cfg(not(debug_assertions))]
        assert_eq!(list[0], app_entry);
    }

    #[test]
    fn etag_is_stable_for_same_input() {
        assert_eq!(build_etag(100, None), build_etag(100, None));
        assert_ne!(build_etag(100, None), build_etag(101, None));
    }

    /// 造一个符合 `<app_dir>/webapps/<feature>/` 布局的临时站点。
    /// 功能名用仓库里不存在的，避免 debug 构建下候选优先命中真实的 `web/<功能>/dist`
    async fn temp_mount(tag: &str) -> (WebMount, PathBuf) {
        let app_dir = std::env::temp_dir().join(format!(
            "jtoolsbox-web-{}-{}",
            tag,
            uuid::Uuid::new_v4()
        ));
        let dir = app_dir.join("webapps").join("test-feature");
        tokio::fs::create_dir_all(dir.join("assets")).await.unwrap();
        tokio::fs::write(dir.join("index.html"), b"<html>ok</html>")
            .await
            .unwrap();
        tokio::fs::write(dir.join("assets/app.js"), b"console.log(1)")
            .await
            .unwrap();
        let mount = WebMount::resolve("test-feature", Some(app_dir.as_path()));
        (mount, app_dir)
    }

    #[tokio::test]
    async fn serves_files_from_disk_and_blocks_traversal() {
        let (mount, dir) = temp_mount("serve").await;
        assert!(mount.is_resolved());

        let index = mount.response("/index.html", &HeaderMap::new()).await;
        assert_eq!(index.status(), StatusCode::OK);
        assert_eq!(index.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
        let body = axum::body::to_bytes(index.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"<html>ok</html>");

        let js = mount.response("/assets/app.js", &HeaderMap::new()).await;
        assert_eq!(js.status(), StatusCode::OK);
        assert_eq!(
            js.headers()[header::CONTENT_TYPE],
            "application/javascript; charset=utf-8"
        );

        let traversal = mount.response("/../config.json", &HeaderMap::new()).await;
        assert_eq!(traversal.status(), StatusCode::NOT_FOUND);

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn revalidates_with_etag() {
        let (mount, dir) = temp_mount("etag").await;
        let first = mount.response("/index.html", &HeaderMap::new()).await;
        let etag = first.headers()[header::ETAG].to_str().unwrap().to_string();

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag.parse().unwrap());
        let second = mount.response("/index.html", &headers).await;
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn missing_root_returns_guide_page() {
        // 用一个不存在的功能名，避免 debug 兜底命中仓库里的 web/file-sharing/dist
        let mount = WebMount::resolve("definitely-not-a-web-feature", None);
        assert!(!mount.is_resolved());
        let response = mount.response("/index.html", &HeaderMap::new()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8_lossy(&body);
        assert!(html.contains("网页资源未找到"));
    }
}
