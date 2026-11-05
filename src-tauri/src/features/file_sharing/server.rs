use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{header, HeaderMap, StatusCode},
    middleware,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio_util::io::ReaderStream;

use super::auth::{auth_middleware, create_token, hash_password, verify_password, Claims};
use super::{FileEntry, FileSharingState, SessionInfo, UploadReservation, User, SESSION_ONLINE_WINDOW};
use crate::web_support::{generated, WebApp};

/// 本功能专属网页，构建期来自 `web/file-sharing/dist`
static WEB_APP: WebApp = WebApp::new(
    generated::file_sharing::ASSETS,
    generated::file_sharing::INDEX_HTML,
);

pub fn create_router(state: Arc<FileSharingState>) -> Router {
    let api_routes = Router::new()
        .route("/files", get(list_files))
        .route(
            "/files/upload/start",
            post(upload_start),
        )
        .route(
            "/files/upload/chunk",
            post(upload_chunk).layer(DefaultBodyLimit::disable()),
        )
        .route("/files/upload/complete", post(upload_complete))
        .route("/files/upload/cancel", post(upload_cancel))
        .route("/files/download", get(download_file))
        .route("/files/preview", get(preview_file))
        .route("/files/delete", post(delete_file))
        .route("/users", get(list_users).post(create_user))
        .route("/users/update", post(update_user))
        .route("/users/delete", post(delete_user))
        .route("/users/kick", post(kick_user))
        .route("/logout", post(logout))
        .route("/logs", get(get_logs))
        .route("/server/status", get(server_status))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .route("/", get(serve_web_ui))
        .route("/favicon.ico", get(serve_favicon))
        .route("/assets/*path", get(serve_asset))
        .route("/api/login", post(login))
        .nest("/api", api_routes)
        .fallback(get(serve_spa))
        .with_state(state)
}

fn get_mime_type(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "application/javascript",
        "json" | "map" => "application/json",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "xml" => "application/xml",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "ogg" => "video/ogg",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "rar" => "application/x-rar-compressed",
        "7z" => "application/x-7z-compressed",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

fn ok_response<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
    })
}

fn err_response(msg: &str) -> Json<ApiResponse<String>> {
    Json(ApiResponse {
        success: false,
        data: None,
        error: Some(msg.to_string()),
    })
}

/// 409 冲突响应：data.reason 供前端区分「磁盘已有同名文件」与「同名文件正在上传」
fn conflict_response(reason: &str, msg: &str) -> Json<ApiResponse<serde_json::Value>> {
    Json(ApiResponse {
        success: false,
        data: Some(serde_json::json!({ "reason": reason })),
        error: Some(msg.to_string()),
    })
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    username: String,
    role: String,
}

async fn login(
    State(state): State<Arc<FileSharingState>>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let users = state.users.read().await;
    let user = users.iter().find(|u| u.username == req.username);

    let user = match user {
        Some(u) => u.clone(),
        None => {
            state.add_log(&req.username, "login_failed", "User not found").await;
            return (StatusCode::UNAUTHORIZED, err_response("用户名或密码错误")).into_response();
        }
    };

    match verify_password(&req.password, &user.password_hash) {
        Ok(true) => {}
        _ => {
            state.add_log(&req.username, "login_failed", "Wrong password").await;
            return (StatusCode::UNAUTHORIZED, err_response("用户名或密码错误")).into_response();
        }
    }

    let config = state.config.read().await;
    let secret = config.jwt_secret.clone();
    let token_hours = config.token_duration_hours;
    drop(config);

    match create_token(&user.id, &user.username, &user.role, &secret, token_hours) {
        Ok((token, token_id)) => {
            let now = Instant::now();
            let mut sessions = state.sessions.lock().await;
            sessions.retain(|_, session| session.expires_at > now);
            sessions.insert(
                token_id,
                SessionInfo {
                    user_id: user.id.clone(),
                    username: user.username.clone(),
                    created_at: chrono::Local::now().to_rfc3339(),
                    expires_at: now + Duration::from_secs(token_hours.saturating_mul(3600)),
                    last_active: now,
                },
            );
            drop(sessions);

            state.add_log(&user.username, "login", "Login successful").await;
            (StatusCode::OK, ok_response(LoginResponse {
                token,
                username: user.username,
                role: user.role,
            })).into_response()
        }
        Err(e) => {
            (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e)).into_response()
        }
    }
}

async fn logout(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
) -> Response {
    state.sessions.lock().await.remove(&claims.jti);
    state.add_log(&claims.username, "logout", "Logout").await;
    ok_response("Logged out").into_response()
}

#[derive(Deserialize)]
struct ListFilesQuery {
    folder_index: Option<usize>,
    path: Option<String>,
}

async fn list_files(
    State(state): State<Arc<FileSharingState>>,
    Query(query): Query<ListFilesQuery>,
) -> impl IntoResponse {
    let config = state.config.read().await;
    let folders = config.shared_folders.clone();
    drop(config);

    let folder_index = query.folder_index.unwrap_or(0);
    if folder_index >= folders.len() {
        return (StatusCode::BAD_REQUEST, err_response("Invalid folder index")).into_response();
    }

    let base_folder = &folders[folder_index];
    let relative_path = query.path.unwrap_or_default();
    let target_path = if relative_path.is_empty() {
        std::path::PathBuf::from(base_folder)
    } else {
        std::path::PathBuf::from(base_folder).join(&relative_path)
    };

    let canonical_base = match std::fs::canonicalize(base_folder) {
        Ok(p) => p,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response(),
    };

    let canonical_target = match target_path.canonicalize() {
        Ok(p) => p,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response(),
    };

    if !canonical_target.starts_with(&canonical_base) {
        return (StatusCode::FORBIDDEN, err_response("Path traversal not allowed")).into_response();
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&canonical_target) {
        for entry in entries.flatten() {
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(UPLOAD_TEMP_PREFIX) {
                continue;
            }
            let relative = canonical_target
                .join(&name)
                .strip_prefix(&canonical_base)
                .unwrap_or(entry.path().as_path())
                .to_string_lossy()
                .replace('\\', "/");

            let ext = std::path::Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();

            let mime = if metadata.is_file() {
                Some(get_mime_type(&ext).to_string())
            } else {
                None
            };

            files.push(FileEntry {
                id: relative.clone(),
                name,
                path: relative,
                size: metadata.len(),
                is_dir: metadata.is_dir(),
                modified: metadata
                    .modified()
                    .ok()
                    .and_then(|t| {
                        let datetime: chrono::DateTime<chrono::Local> = t.into();
                        Some(datetime.format("%Y-%m-%d %H:%M:%S").to_string())
                    })
                    .unwrap_or_default(),
                mime_type: mime,
            });
        }
    }

    files.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    (StatusCode::OK, ok_response(files)).into_response()
}

#[derive(Deserialize)]
struct FileQuery {
    folder_index: Option<usize>,
    path: String,
}

async fn resolve_file_path(state: &FileSharingState, folder_index: usize, relative_path: &str) -> Result<std::path::PathBuf, String> {
    let config = state.config.read().await;
    let folders = config.shared_folders.clone();
    drop(config);

    if folder_index >= folders.len() {
        return Err("Invalid folder index".to_string());
    }

    let base_folder = &folders[folder_index];
    let canonical_base = std::fs::canonicalize(base_folder).map_err(|e| e.to_string())?;
    let target = canonical_base.join(relative_path);
    let canonical_target = target.canonicalize().map_err(|e| e.to_string())?;

    if !canonical_target.starts_with(&canonical_base) {
        return Err("Path traversal not allowed".to_string());
    }

    Ok(canonical_target)
}

async fn download_file(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    headers: HeaderMap,
    Query(query): Query<FileQuery>,
) -> impl IntoResponse {
    let folder_index = query.folder_index.unwrap_or(0);
    let file_path = match resolve_file_path(&state, folder_index, &query.path).await {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, err_response(&e)).into_response(),
    };

    if !file_path.is_file() {
        return (StatusCode::NOT_FOUND, err_response("File not found")).into_response();
    }

    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());

    state.add_log(&claims.username, "download", &format!("Downloaded: {}", query.path)).await;

    let encoded_name = urlencoding::encode(&file_name);
    let disposition = format!("attachment; filename*=UTF-8''{}", encoded_name);
    stream_file(&file_path, &headers, Some(disposition)).await
}

async fn preview_file(
    State(state): State<Arc<FileSharingState>>,
    headers: HeaderMap,
    Query(query): Query<FileQuery>,
) -> impl IntoResponse {
    let folder_index = query.folder_index.unwrap_or(0);
    let file_path = match resolve_file_path(&state, folder_index, &query.path).await {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, err_response(&e)).into_response(),
    };

    if !file_path.is_file() {
        return (StatusCode::NOT_FOUND, err_response("File not found")).into_response();
    }

    let insecure_ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_lowercase().as_str(), "html" | "htm" | "svg" | "xml" | "xhtml"))
        .unwrap_or(false);

    let mut response = stream_file(&file_path, &headers, None).await;
    if insecure_ext {
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderValue::from_static("sandbox"),
        );
    }
    response
}

#[derive(Clone, Copy)]
struct ByteRange {
    start: u64,
    end: u64,
}

fn parse_range(value: &str, total: u64) -> Option<ByteRange> {
    let spec = value.strip_prefix("bytes=")?;
    let (start_str, end_str) = spec.split_once('-')?;
    if total == 0 {
        return None;
    }
    if start_str.is_empty() {
        let suffix: u64 = end_str.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        let start = total.saturating_sub(suffix);
        Some(ByteRange { start, end: total - 1 })
    } else {
        let start: u64 = start_str.parse().ok()?;
        let end = if end_str.is_empty() {
            total - 1
        } else {
            end_str.parse::<u64>().ok()?.min(total - 1)
        };
        if start > end || start >= total {
            return None;
        }
        Some(ByteRange { start, end })
    }
}

async fn stream_file(
    path: &std::path::Path,
    request_headers: &HeaderMap,
    disposition: Option<String>,
) -> Response {
    use tokio::io::{AsyncSeekExt, AsyncReadExt};

    let metadata = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response(),
    };
    let total = metadata.len();

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mime = get_mime_type(ext);

    let range_header = request_headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let requested = range_header.and_then(|h| parse_range(h, total));

    if range_header.is_some() && requested.is_none() {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{}", total))
            .body(Body::empty())
            .unwrap();
    }

    let (start, length, status) = match requested {
        Some(r) => (r.start, r.end - r.start + 1, StatusCode::PARTIAL_CONTENT),
        None => (0, total, StatusCode::OK),
    };

    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CONTENT_LENGTH, length);

    if let Some(d) = disposition {
        builder = builder.header(header::CONTENT_DISPOSITION, d);
    }
    if let Some(r) = requested {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", r.start, r.end, total),
        );
    }

    if length == 0 {
        return builder.body(Body::empty()).unwrap();
    }

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response(),
    };
    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, err_response("Seek failed")).into_response();
    }

    let stream = ReaderStream::new(file.take(length));
    builder.body(Body::from_stream(stream)).unwrap()
}

#[derive(Deserialize)]
struct DeleteFileRequest {
    folder_index: Option<usize>,
    path: String,
}

async fn delete_file(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Json(req): Json<DeleteFileRequest>,
) -> impl IntoResponse {
    if claims.role != "admin" {
        return (StatusCode::FORBIDDEN, err_response("仅管理员可删除文件")).into_response();
    }

    let folder_index = req.folder_index.unwrap_or(0);
    let file_path = match resolve_file_path(&state, folder_index, &req.path).await {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, err_response(&e)).into_response(),
    };

    if !file_path.exists() {
        return (StatusCode::NOT_FOUND, err_response("File not found")).into_response();
    }

    let result = if file_path.is_dir() {
        tokio::fs::remove_dir_all(&file_path).await
    } else {
        tokio::fs::remove_file(&file_path).await
    };

    match result {
        Ok(_) => {
            state.add_log(&claims.username, "delete", &format!("Deleted: {}", req.path)).await;
            (StatusCode::OK, ok_response("File deleted")).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response(),
    }
}

const UPLOAD_TEMP_PREFIX: &str = ".fs-upload-";

fn sanitize_upload_id(raw: &str) -> Option<String> {
    let valid = !raw.is_empty()
        && raw.len() <= 64
        && raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then(|| raw.to_string())
}

fn sanitize_file_name(raw: &str) -> Option<String> {
    std::path::Path::new(raw)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty() && n != "." && n != ".." && !n.starts_with(UPLOAD_TEMP_PREFIX))
}

fn temp_upload_path(dir: &std::path::Path, upload_id: &str) -> std::path::PathBuf {
    dir.join(format!("{}{}.part", UPLOAD_TEMP_PREFIX, upload_id))
}

/// 解析上传目标目录，返回 (共享根目录, 目标目录)，均已完成越权校验
async fn resolve_upload_dir(
    state: &FileSharingState,
    folder_index: usize,
    relative_dir: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Response> {
    let folders = state.config.read().await.shared_folders.clone();
    if folder_index >= folders.len() {
        return Err((StatusCode::BAD_REQUEST, err_response("Invalid folder index")).into_response());
    }

    let canonical_base = std::fs::canonicalize(&folders[folder_index])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response())?;

    let target_dir = if relative_dir.is_empty() {
        canonical_base.clone()
    } else {
        canonical_base.join(relative_dir)
    };
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response())?;

    let canonical_target = target_dir
        .canonicalize()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response())?;
    if !canonical_target.starts_with(&canonical_base) {
        return Err((StatusCode::FORBIDDEN, err_response("Path traversal not allowed")).into_response());
    }

    Ok((canonical_base, canonical_target))
}

/// 上传占位超时：长时间无分片写入（暂停/浏览器关闭）后自动释放文件名
const UPLOAD_RESERVATION_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// 宽限期：占位后临时文件尚未建立时，给客户端留出开始传输的时间
const UPLOAD_RESERVATION_GRACE: Duration = Duration::from_secs(60);

fn upload_key(path: &std::path::Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

#[derive(Deserialize)]
struct UploadStartRequest {
    upload_id: String,
    folder_index: Option<usize>,
    dir: Option<String>,
    name: String,
}

/// 上传开始：在真正传输前占用目标文件名，同名冲突立即返回，避免大文件传完才失败
async fn upload_start(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Json(req): Json<UploadStartRequest>,
) -> Response {
    let Some(upload_id) = sanitize_upload_id(&req.upload_id) else {
        return (StatusCode::BAD_REQUEST, err_response("Invalid upload id")).into_response();
    };
    let Some(file_name) = sanitize_file_name(&req.name) else {
        return (StatusCode::BAD_REQUEST, err_response("Invalid file name")).into_response();
    };

    let (_, dir) = match resolve_upload_dir(
        &state,
        req.folder_index.unwrap_or(0),
        req.dir.as_deref().unwrap_or(""),
    )
    .await
    {
        Ok(v) => v,
        Err(response) => return response,
    };

    let target = dir.join(&file_name);
    let key = upload_key(&target);
    let temp = temp_upload_path(&dir, &upload_id);

    let mut uploads = state.uploads.lock().await;

    let now = Instant::now();
    uploads.retain(|_, reservation| {
        let idle = now.duration_since(reservation.last_active);
        idle < UPLOAD_RESERVATION_TTL
            && (reservation.temp.exists() || idle < UPLOAD_RESERVATION_GRACE)
    });

    if target.exists() {
        return (StatusCode::CONFLICT, conflict_response("exists", "目标目录中已存在同名文件"))
            .into_response();
    }

    if let Some(conflict) = uploads
        .values()
        .find(|reservation| reservation.upload_id != upload_id && upload_key(&reservation.target) == key)
    {
        let msg = if conflict.username == claims.username {
            "同名文件正在上传中，请等待完成或取消后再试"
        } else {
            "同名文件正在由其他用户上传，请稍后再试"
        };
        return (StatusCode::CONFLICT, conflict_response("busy", msg)).into_response();
    }

    uploads.insert(
        upload_id.clone(),
        UploadReservation {
            upload_id: upload_id.clone(),
            username: claims.username.clone(),
            target,
            temp: temp.clone(),
            last_active: now,
        },
    );
    drop(uploads);

    // 预先创建临时文件，空文件也能正常完成，同时让占位在清理时可见
    if let Err(e) = tokio::fs::File::create(&temp).await {
        state.uploads.lock().await.remove(&upload_id);
        return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response();
    }

    ok_response(serde_json::json!({ "upload_id": upload_id, "name": file_name })).into_response()
}

#[derive(Deserialize)]
struct UploadChunkQuery {
    upload_id: String,
    offset: u64,
}

/// 分片写入：按 offset 覆盖写临时文件，重复/中断后重传同一片不会产生重复数据
async fn upload_chunk(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Query(query): Query<UploadChunkQuery>,
    body: axum::body::Bytes,
) -> Response {
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};

    let Some(upload_id) = sanitize_upload_id(&query.upload_id) else {
        return (StatusCode::BAD_REQUEST, err_response("Invalid upload id")).into_response();
    };
    if body.is_empty() {
        return (StatusCode::BAD_REQUEST, err_response("Empty chunk")).into_response();
    }

    // 只接受已通过 upload_start 占位的会话，避免绕过同名检查写入
    let reservation = {
        let mut uploads = state.uploads.lock().await;
        match uploads.get_mut(&upload_id) {
            Some(reservation) if reservation.username == claims.username => {
                reservation.last_active = Instant::now();
                reservation.clone()
            }
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    err_response("上传会话不存在或已过期，请重新开始上传"),
                )
                    .into_response();
            }
        }
    };

    let mut file = match tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&reservation.temp)
        .await
    {
        Ok(f) => f,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response(),
    };

    let write_result = async {
        file.seek(std::io::SeekFrom::Start(query.offset)).await?;
        file.write_all(&body).await?;
        file.flush().await
    }
    .await;

    match write_result {
        Ok(_) => ok_response(serde_json::json!({ "received": body.len() })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response(),
    }
}

#[derive(Deserialize)]
struct UploadCompleteRequest {
    upload_id: String,
    folder_index: Option<usize>,
    dir: Option<String>,
    name: String,
    size: u64,
}

async fn upload_complete(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Json(req): Json<UploadCompleteRequest>,
) -> Response {
    let Some(upload_id) = sanitize_upload_id(&req.upload_id) else {
        return (StatusCode::BAD_REQUEST, err_response("Invalid upload id")).into_response();
    };
    let Some(file_name) = sanitize_file_name(&req.name) else {
        return (StatusCode::BAD_REQUEST, err_response("Invalid file name")).into_response();
    };

    let reservation = {
        let uploads = state.uploads.lock().await;
        match uploads.get(&upload_id) {
            Some(reservation) if reservation.username == claims.username => reservation.clone(),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    err_response("上传会话不存在或已过期，请重新开始上传"),
                )
                    .into_response();
            }
        }
    };

    let (canonical_base, dir) = match resolve_upload_dir(
        &state,
        req.folder_index.unwrap_or(0),
        req.dir.as_deref().unwrap_or(""),
    )
    .await
    {
        Ok(v) => v,
        Err(response) => return response,
    };

    let target_file = dir.join(&file_name);
    if upload_key(&target_file) != upload_key(&reservation.target) {
        state.uploads.lock().await.remove(&upload_id);
        return (StatusCode::BAD_REQUEST, err_response("上传会话与目标文件不一致")).into_response();
    }

    let metadata = match tokio::fs::metadata(&reservation.temp).await {
        Ok(m) => m,
        Err(_) => {
            state.uploads.lock().await.remove(&upload_id);
            return (StatusCode::BAD_REQUEST, err_response("上传数据不存在或已过期")).into_response();
        }
    };

    if metadata.len() != req.size {
        state.uploads.lock().await.remove(&upload_id);
        let _ = tokio::fs::remove_file(&reservation.temp).await;
        return (
            StatusCode::BAD_REQUEST,
            err_response(&format!(
                "上传数据不完整：已接收 {} 字节，应为 {} 字节",
                metadata.len(),
                req.size
            )),
        )
            .into_response();
    }

    // 占位后仍可能有外部进程放入同名文件，此时不覆盖
    if target_file.exists() {
        state.uploads.lock().await.remove(&upload_id);
        let _ = tokio::fs::remove_file(&reservation.temp).await;
        return (StatusCode::CONFLICT, conflict_response("exists", "目标目录中已存在同名文件"))
            .into_response();
    }

    if let Err(e) = tokio::fs::rename(&reservation.temp, &target_file).await {
        state.uploads.lock().await.remove(&upload_id);
        let _ = tokio::fs::remove_file(&reservation.temp).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e.to_string())).into_response();
    }

    state.uploads.lock().await.remove(&upload_id);

    let display_path = target_file
        .strip_prefix(&canonical_base)
        .unwrap_or(target_file.as_path())
        .to_string_lossy()
        .replace('\\', "/");

    state.add_log(
        &claims.username,
        "upload",
        &format!("Uploaded: {} ({} bytes)", display_path, req.size),
    )
    .await;

    ok_response(serde_json::json!({
        "name": file_name,
        "path": display_path,
        "size": req.size,
    }))
    .into_response()
}

#[derive(Deserialize)]
struct UploadCancelRequest {
    upload_id: String,
    folder_index: Option<usize>,
    dir: Option<String>,
}

async fn upload_cancel(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Json(req): Json<UploadCancelRequest>,
) -> Response {
    let Some(upload_id) = sanitize_upload_id(&req.upload_id) else {
        return (StatusCode::BAD_REQUEST, err_response("Invalid upload id")).into_response();
    };

    let (_, dir) = match resolve_upload_dir(
        &state,
        req.folder_index.unwrap_or(0),
        req.dir.as_deref().unwrap_or(""),
    )
    .await
    {
        Ok(v) => v,
        Err(response) => return response,
    };

    let mut temp_paths = vec![temp_upload_path(&dir, &upload_id)];
    {
        let mut uploads = state.uploads.lock().await;
        if let Some(reservation) = uploads.get(&upload_id) {
            if reservation.username != claims.username {
                return (StatusCode::FORBIDDEN, err_response("无权取消该上传")).into_response();
            }
            if !temp_paths.contains(&reservation.temp) {
                temp_paths.push(reservation.temp.clone());
            }
        }
        uploads.remove(&upload_id);
    }

    for path in temp_paths {
        let _ = tokio::fs::remove_file(&path).await;
    }
    ok_response("Upload cancelled").into_response()
}

async fn list_users(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
) -> impl IntoResponse {
    if claims.role != "admin" {
        return (StatusCode::FORBIDDEN, err_response("Admin access required")).into_response();
    }

    let users = state.users.read().await;
    let sessions = state.sessions.lock().await;
    let now = Instant::now();

    let safe_users: Vec<serde_json::Value> = users
        .iter()
        .map(|u| {
            let mut online = false;
            let mut last_login_at: Option<&str> = None;
            for session in sessions
                .values()
                .filter(|s| {
                    s.username == u.username
                        && s.expires_at > now
                        && now.duration_since(s.last_active) < SESSION_ONLINE_WINDOW
                })
            {
                online = true;
                if last_login_at.map(|last| session.created_at.as_str() > last).unwrap_or(true) {
                    last_login_at = Some(&session.created_at);
                }
            }
            serde_json::json!({
                "id": u.id,
                "username": u.username,
                "role": u.role,
                "created_at": u.created_at,
                "online": online,
                "last_login_at": last_login_at,
            })
        })
        .collect();

    (StatusCode::OK, ok_response(safe_users)).into_response()
}

#[derive(Deserialize)]
struct KickUserRequest {
    username: String,
}

/// 强制下线：移除该用户全部会话，已签发的 token 立即失效
async fn kick_user(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Json(req): Json<KickUserRequest>,
) -> Response {
    if claims.role != "admin" {
        return (StatusCode::FORBIDDEN, err_response("Admin access required")).into_response();
    }
    if req.username == claims.username {
        return (StatusCode::BAD_REQUEST, err_response("不能强制下线当前登录账号")).into_response();
    }

    let removed = {
        let mut sessions = state.sessions.lock().await;
        let before = sessions.len();
        sessions.retain(|_, session| session.username != req.username);
        before - sessions.len()
    };

    if removed > 0 {
        state
            .add_log(
                &claims.username,
                "force_logout",
                &format!("Force logout: {} ({} sessions)", req.username, removed),
            )
            .await;
    }

    ok_response(serde_json::json!({ "removed": removed })).into_response()
}

#[derive(Deserialize)]
struct CreateUserRequest {
    username: String,
    password: String,
    role: Option<String>,
}

async fn create_user(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    if claims.role != "admin" {
        return (StatusCode::FORBIDDEN, err_response("Admin access required")).into_response();
    }

    if req.username.is_empty() || req.password.is_empty() {
        return (StatusCode::BAD_REQUEST, err_response("Username and password required")).into_response();
    }

    let mut users = state.users.write().await;
    if users.iter().any(|u| u.username == req.username) {
        return (StatusCode::CONFLICT, err_response("Username already exists")).into_response();
    }

    let password_hash = match hash_password(&req.password) {
        Ok(h) => h,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e)).into_response(),
    };

    let user = User {
        id: uuid::Uuid::new_v4().to_string(),
        username: req.username.clone(),
        password_hash,
        role: req.role.unwrap_or_else(|| "user".to_string()),
        created_at: chrono::Local::now().to_rfc3339(),
    };

    users.push(user.clone());
    drop(users);

    if let Err(e) = state.save_users().await {
        return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e)).into_response();
    }

    state.add_log(&claims.username, "create_user", &format!("Created user: {}", req.username)).await;

    (StatusCode::CREATED, ok_response(serde_json::json!({
        "id": user.id,
        "username": user.username,
        "role": user.role,
        "created_at": user.created_at,
    }))).into_response()
}

#[derive(Deserialize)]
struct UpdateUserRequest {
    id: String,
    username: Option<String>,
    password: Option<String>,
    role: Option<String>,
}

async fn update_user(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Json(req): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    if claims.role != "admin" {
        return (StatusCode::FORBIDDEN, err_response("Admin access required")).into_response();
    }

    let mut users = state.users.write().await;
    let user = match users.iter_mut().find(|u| u.id == req.id) {
        Some(u) => u,
        None => return (StatusCode::NOT_FOUND, err_response("User not found")).into_response(),
    };

    if let Some(username) = req.username {
        if !username.is_empty() {
            user.username = username;
        }
    }

    if let Some(password) = req.password {
        if !password.is_empty() {
            match hash_password(&password) {
                Ok(h) => user.password_hash = h,
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e)).into_response(),
            }
        }
    }

    if let Some(role) = req.role {
        if role == "admin" || role == "user" {
            user.role = role;
        }
    }

    drop(users);

    if let Err(e) = state.save_users().await {
        return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e)).into_response();
    }

    // 资料变更后使该用户已有会话失效，避免旧 token 继续沿用旧角色
    state
        .sessions
        .lock()
        .await
        .retain(|_, session| session.user_id != req.id);

    state.add_log(&claims.username, "update_user", &format!("Updated user: {}", req.id)).await;

    (StatusCode::OK, ok_response("User updated")).into_response()
}

#[derive(Deserialize)]
struct DeleteUserRequest {
    id: String,
}

async fn delete_user(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Json(req): Json<DeleteUserRequest>,
) -> impl IntoResponse {
    if claims.role != "admin" {
        return (StatusCode::FORBIDDEN, err_response("Admin access required")).into_response();
    }

    if req.id == claims.sub {
        return (StatusCode::BAD_REQUEST, err_response("Cannot delete yourself")).into_response();
    }

    let mut users = state.users.write().await;
    let before_len = users.len();
    users.retain(|u| u.id != req.id);

    if users.len() == before_len {
        return (StatusCode::NOT_FOUND, err_response("User not found")).into_response();
    }

    drop(users);

    if let Err(e) = state.save_users().await {
        return (StatusCode::INTERNAL_SERVER_ERROR, err_response(&e)).into_response();
    }

    state
        .sessions
        .lock()
        .await
        .retain(|_, session| session.user_id != req.id);

    state.add_log(&claims.username, "delete_user", &format!("Deleted user: {}", req.id)).await;

    (StatusCode::OK, ok_response("User deleted")).into_response()
}

#[derive(Deserialize)]
struct LogsQuery {
    limit: Option<usize>,
}

async fn get_logs(
    State(state): State<Arc<FileSharingState>>,
    claims: axum::Extension<Claims>,
    Query(query): Query<LogsQuery>,
) -> impl IntoResponse {
    if claims.role != "admin" {
        return (StatusCode::FORBIDDEN, err_response("Admin access required")).into_response();
    }

    let limit = query.limit.unwrap_or(100).min(1000);
    let logs = state.logs.read().await;
    let entries: Vec<_> = logs.iter().rev().take(limit).collect();

    (StatusCode::OK, ok_response(entries)).into_response()
}

async fn server_status(
    State(state): State<Arc<FileSharingState>>,
) -> impl IntoResponse {
    let status = state.status.read().await;
    let config = state.config.read().await;

    (StatusCode::OK, ok_response(serde_json::json!({
        "running": status.running,
        "port": if status.running { config.port } else { 0 },
        "url": status.url.clone().unwrap_or_default(),
        "auto_start": config.auto_start,
        "shared_folders": config.shared_folders,
    }))).into_response()
}

pub async fn start_server(fs_state: Arc<FileSharingState>) -> Result<u16, String> {
    let config = fs_state.config.read().await;
    let port = config.port;
    drop(config);

    {
        let status = fs_state.status.read().await;
        if status.running {
            return Err("Server is already running".to_string());
        }
    }

    let router = create_router(fs_state.clone());

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .map_err(|e| format!("Failed to bind port {}: {}", port, e))?;

    let actual_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

    {
        let mut status = fs_state.status.write().await;
        status.running = true;
        status.port = actual_port;
        status.url = Some(format!("http://localhost:{}", actual_port));
    }

    {
        let mut tx = fs_state.shutdown_tx.write().await;
        *tx = Some(shutdown_tx);
    }

    let fs_state_clone = fs_state.clone();
    tokio::spawn(async move {
        let rx = shutdown_rx;
        let shutdown_future = async move {
            let mut rx = rx;
            let _ = rx.recv().await;
        };

        let result = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_future)
            .await;

        if let Err(e) = result {
            eprintln!("Server error: {}", e);
        }

        let mut status = fs_state_clone.status.write().await;
        status.running = false;
        status.port = 0;
        status.url = None;
    });

    fs_state.add_log("system", "server_start", &format!("Server started on port {}", actual_port)).await;

    Ok(actual_port)
}

pub async fn stop_server(fs_state: Arc<FileSharingState>) -> Result<(), String> {
    let shutdown_tx = {
        let tx = fs_state.shutdown_tx.read().await;
        tx.clone()
    };

    if let Some(tx) = shutdown_tx {
        let _ = tx.send(());
        fs_state.add_log("system", "server_stop", "Server stopped").await;
        Ok(())
    } else {
        Err("Server is not running".to_string())
    }
}

async fn serve_web_ui() -> Response {
    WEB_APP.index_response()
}

async fn serve_spa(uri: axum::http::Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    WEB_APP.index_response()
}

async fn serve_favicon() -> Response {
    if WEB_APP.asset("/favicon.ico").is_some() {
        WEB_APP.asset_response("/favicon.ico")
    } else {
        StatusCode::NO_CONTENT.into_response()
    }
}

async fn serve_asset(AxumPath(path): AxumPath<String>) -> Response {
    WEB_APP.asset_response(&format!("/assets/{}", path))
}

