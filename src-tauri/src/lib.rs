mod features;
mod web_support;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::Manager;

use features::file_sharing;
use file_sharing::{create_shared_state, server as fs_server, FileSharingConfig, FileSharingState};

#[derive(Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub theme: String,
    pub close_action: String,
    pub data_root_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            close_action: "exit".to_string(),
            data_root_path: None,
        }
    }
}

fn get_data_root(app_config: &AppConfig) -> PathBuf {
    if let Some(ref custom_path) = app_config.data_root_path {
        if !custom_path.is_empty() {
            return PathBuf::from(custom_path);
        }
    }
    let exe_path = std::env::current_exe().expect("Failed to get exe path");
    let exe_dir = exe_path.parent().expect("Failed to get exe directory");
    exe_dir.join("function-datas")
}

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
    pub file_sharing: std::sync::Arc<FileSharingState>,
}

fn get_config_path() -> PathBuf {
    let exe_path = std::env::current_exe().expect("Failed to get exe path");
    let exe_dir = exe_path.parent().expect("Failed to get exe directory");
    exe_dir.join("config.json")
}

fn load_config(path: &PathBuf) -> AppConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_config(path: &PathBuf, config: &AppConfig) -> Result<(), String> {
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
fn set_config(state: tauri::State<AppState>, config: AppConfig) -> Result<(), String> {
    {
        let mut current = state.config.lock().unwrap();
        *current = config.clone();
    }
    save_config(&state.config_path, &config)
}

#[derive(Serialize, Deserialize)]
struct FileSharingConfigInput {
    port: u16,
    shared_folders: Vec<String>,
    token_duration_hours: u64,
    auto_start: Option<bool>,
}

#[tauri::command]
async fn get_file_sharing_config(state: tauri::State<'_, AppState>) -> Result<FileSharingConfig, String> {
    Ok(state.file_sharing.config.read().await.clone())
}

#[tauri::command]
async fn set_file_sharing_config(
    state: tauri::State<'_, AppState>,
    config: FileSharingConfigInput,
) -> Result<(), String> {
    let mut current = state.file_sharing.config.write().await;
    current.port = config.port;
    current.shared_folders = config.shared_folders;
    current.token_duration_hours = config.token_duration_hours;
    if let Some(auto_start) = config.auto_start {
        current.auto_start = auto_start;
    }
    drop(current);
    state.file_sharing.save_config().await
}

#[tauri::command]
async fn start_file_sharing(state: tauri::State<'_, AppState>) -> Result<u16, String> {
    fs_server::start_server(state.file_sharing.clone()).await
}

#[tauri::command]
async fn stop_file_sharing(state: tauri::State<'_, AppState>) -> Result<(), String> {
    fs_server::stop_server(state.file_sharing.clone()).await
}

#[tauri::command]
async fn get_file_sharing_status(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let status = state.file_sharing.status.read().await;
    let config = state.file_sharing.config.read().await;
    Ok(serde_json::json!({
        "running": status.running,
        "port": if status.running { status.port } else { config.port },
        "url": status.url.clone().unwrap_or_default(),
        "auto_start": config.auto_start,
    }))
}

#[tauri::command]
async fn get_file_sharing_users(state: tauri::State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let users = state.file_sharing.users.read().await;
    let sessions = state.file_sharing.sessions.lock().await;
    let now = std::time::Instant::now();

    Ok(users
        .iter()
        .map(|u| {
            let mut online = false;
            let mut last_login_at: Option<&str> = None;
            for session in sessions
                .values()
                .filter(|s| {
                    s.username == u.username
                        && s.expires_at > now
                        && now.duration_since(s.last_active) < file_sharing::SESSION_ONLINE_WINDOW
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
        .collect())
}

#[tauri::command]
async fn force_logout_file_sharing_user(
    state: tauri::State<'_, AppState>,
    username: String,
) -> Result<u64, String> {
    let removed = {
        let mut sessions = state.file_sharing.sessions.lock().await;
        let before = sessions.len();
        sessions.retain(|_, session| session.username != username);
        (before - sessions.len()) as u64
    };

    if removed > 0 {
        state
            .file_sharing
            .add_log(
                "admin",
                "force_logout",
                &format!("Force logout: {} ({} sessions)", username, removed),
            )
            .await;
    }

    Ok(removed)
}

#[tauri::command]
fn open_shared_folder(path: String) -> Result<(), String> {
    let folder = PathBuf::from(&path);
    if !folder.is_dir() {
        return Err("文件夹不存在或已被移动".to_string());
    }
    tauri_plugin_opener::open_path(&folder, None::<&str>).map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize)]
struct CreateUserInput {
    username: String,
    password: String,
    role: Option<String>,
}

#[tauri::command]
async fn create_file_sharing_user(
    state: tauri::State<'_, AppState>,
    input: CreateUserInput,
) -> Result<serde_json::Value, String> {
    if input.username.is_empty() || input.password.is_empty() {
        return Err("用户名和密码不能为空".to_string());
    }

    let mut users = state.file_sharing.users.write().await;
    if users.iter().any(|u| u.username == input.username) {
        return Err("用户名已存在".to_string());
    }

    let password_hash = file_sharing::auth::hash_password(&input.password)?;
    let user = file_sharing::User {
        id: uuid::Uuid::new_v4().to_string(),
        username: input.username.clone(),
        password_hash,
        role: input.role.unwrap_or_else(|| "user".to_string()),
        created_at: chrono::Local::now().to_rfc3339(),
    };

    users.push(user.clone());
    drop(users);

    state.file_sharing.save_users().await?;
    state
        .file_sharing
        .add_log("admin", "create_user", &format!("Created user: {}", input.username))
        .await;

    Ok(serde_json::json!({
        "id": user.id,
        "username": user.username,
        "role": user.role,
        "created_at": user.created_at,
    }))
}

#[derive(Serialize, Deserialize)]
struct UpdateUserInput {
    id: String,
    username: Option<String>,
    password: Option<String>,
    role: Option<String>,
}

#[tauri::command]
async fn update_file_sharing_user(
    state: tauri::State<'_, AppState>,
    input: UpdateUserInput,
) -> Result<(), String> {
    let mut users = state.file_sharing.users.write().await;
    let user = users
        .iter_mut()
        .find(|u| u.id == input.id)
        .ok_or_else(|| "用户不存在".to_string())?;

    if let Some(ref username) = input.username {
        if !username.is_empty() {
            user.username = username.clone();
        }
    }

    if let Some(ref password) = input.password {
        if !password.is_empty() {
            user.password_hash = file_sharing::auth::hash_password(password)?;
        }
    }

    if let Some(ref role) = input.role {
        if role == "admin" || role == "user" {
            user.role = role.clone();
        }
    }

    drop(users);
    state.file_sharing.save_users().await?;

    // 资料变更后使该用户已有会话失效，避免旧 token 沿用旧角色
    state
        .file_sharing
        .sessions
        .lock()
        .await
        .retain(|_, session| session.user_id != input.id);

    Ok(())
}

#[tauri::command]
async fn delete_file_sharing_user(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let mut users = state.file_sharing.users.write().await;
    let before = users.len();
    users.retain(|u| u.id != id);

    if users.len() == before {
        return Err("用户不存在".to_string());
    }

    drop(users);
    state.file_sharing.save_users().await?;
    state
        .file_sharing
        .sessions
        .lock()
        .await
        .retain(|_, session| session.user_id != id);
    state
        .file_sharing
        .add_log("admin", "delete_user", &format!("Deleted user: {}", id))
        .await;

    Ok(())
}

#[tauri::command]
async fn get_file_sharing_logs(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<file_sharing::LogEntry>, String> {
    let limit = limit.unwrap_or(100).min(1000);
    let logs = state.file_sharing.logs.read().await;
    Ok(logs.iter().rev().take(limit).cloned().collect())
}

#[derive(Serialize)]
struct NetworkInterface {
    name: String,
    ip: String,
    label: String,
    connected: bool,
}

/// 网卡是否真正接入网络：只有链路本地地址（如 169.254.x.x）视为未连接，
/// 其余以系统的介质连接状态为准，取不到状态时默认视为可用
fn is_interface_connected(index: u32, ip: &std::net::Ipv4Addr) -> bool {
    if ip.is_link_local() {
        return false;
    }
    media_connected(index).unwrap_or(true)
}

#[cfg(windows)]
fn media_connected(index: u32) -> Option<bool> {
    use windows::Win32::NetworkManagement::IpHelper::{GetIfEntry2, MIB_IF_ROW2};
    use windows::Win32::NetworkManagement::Ndis::{IfOperStatusUp, MediaConnectStateDisconnected};

    unsafe {
        let mut row = MIB_IF_ROW2::default();
        row.InterfaceIndex = index;
        if GetIfEntry2(&mut row).0 != 0 {
            return None;
        }
        let oper_up = row.OperStatus == IfOperStatusUp;
        let media_ok = row.MediaConnectState != MediaConnectStateDisconnected;
        Some(oper_up && media_ok)
    }
}

#[cfg(not(windows))]
fn media_connected(_index: u32) -> Option<bool> {
    None
}

#[tauri::command]
fn get_network_addresses() -> Vec<NetworkInterface> {
    use network_interface::NetworkInterfaceConfig;

    let mut result = Vec::new();
    let mut wlan_found = false;
    let mut eth_found = false;

    if let Ok(interfaces) = network_interface::NetworkInterface::show() {
        // 第一遍：查找 WLAN 和以太网
        for iface in &interfaces {
            for addr in &iface.addr {
                if let network_interface::Addr::V4(v4) = addr {
                    if v4.ip.is_loopback() {
                        continue;
                    }

                    let name_lower = iface.name.to_lowercase();

                    if !wlan_found && (name_lower.contains("wlan") || name_lower.contains("wi-fi") || name_lower.contains("wifi") || name_lower.contains("无线")) {
                        result.push(NetworkInterface {
                            name: iface.name.clone(),
                            ip: v4.ip.to_string(),
                            label: "WLAN".to_string(),
                            connected: is_interface_connected(iface.index, &v4.ip),
                        });
                        wlan_found = true;
                    } else if !eth_found && (name_lower.contains("ethernet") || name_lower.contains("eth") || name_lower.contains("以太网") || name_lower.contains("本地连接")) {
                        result.push(NetworkInterface {
                            name: iface.name.clone(),
                            ip: v4.ip.to_string(),
                            label: "以太网".to_string(),
                            connected: is_interface_connected(iface.index, &v4.ip),
                        });
                        eth_found = true;
                    }
                }
            }
        }

        // 第二遍：如果没找到 WLAN 或以太网，添加其他非回环接口
        if !wlan_found || !eth_found {
            for iface in &interfaces {
                for addr in &iface.addr {
                    if let network_interface::Addr::V4(v4) = addr {
                        if v4.ip.is_loopback() {
                            continue;
                        }

                        let ip = v4.ip.to_string();

                        // 跳过已经添加过的 IP
                        if result.iter().any(|r| r.ip == ip) {
                            continue;
                        }

                        let name_lower = iface.name.to_lowercase();

                        // 跳过已识别的 WLAN 和以太网接口
                        if name_lower.contains("wlan") || name_lower.contains("wi-fi") || name_lower.contains("wifi") || name_lower.contains("无线") {
                            continue;
                        }
                        if name_lower.contains("ethernet") || name_lower.contains("eth") || name_lower.contains("以太网") || name_lower.contains("本地连接") {
                            continue;
                        }

                        // 添加其他接口
                        result.push(NetworkInterface {
                            name: iface.name.clone(),
                            ip,
                            label: iface.name.clone(),
                            connected: is_interface_connected(iface.index, &v4.ip),
                        });
                    }
                }
            }
        }
    }

    result.sort_by(|a, b| a.label.cmp(&b.label));
    result
}

#[tauri::command]
fn get_data_root_path(state: tauri::State<AppState>) -> String {
    let config = state.config.lock().unwrap();
    get_data_root(&config).to_string_lossy().to_string()
}

#[tauri::command]
fn open_data_folder(state: tauri::State<AppState>) -> Result<(), String> {
    let config = state.config.lock().unwrap();
    let path = get_data_root(&config);
    drop(config);

    if !path.exists() {
        let _ = fs::create_dir_all(&path);
    }

    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_data_root_path(
    state: tauri::State<AppState>,
    path: String,
) -> Result<(), String> {
    let old_root = {
        let config = state.config.lock().unwrap();
        get_data_root(&config)
    };

    let new_root = if path.is_empty() {
        let exe_path = std::env::current_exe().expect("Failed to get exe path");
        let exe_dir = exe_path.parent().expect("Failed to get exe directory");
        exe_dir.join("function-datas")
    } else {
        PathBuf::from(&path)
    };

    {
        let mut config = state.config.lock().unwrap();
        config.data_root_path = if path.is_empty() { None } else { Some(path) };
        save_config(&state.config_path, &config)?;
    }

    if old_root != new_root && old_root.exists() {
        let _ = fs::create_dir_all(&new_root);

        if let Ok(entries) = fs::read_dir(&old_root) {
            for entry in entries.flatten() {
                let src = entry.path();
                if src.is_dir() {
                    let folder_name = src.file_name().unwrap_or_default();
                    let dest = new_root.join(folder_name);
                    if !dest.exists() {
                        let _ = fs::rename(&src, &dest);
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_path = get_config_path();
    let config = load_config(&config_path);
    let data_root = get_data_root(&config);
    let fs_data_dir = data_root.join("file-sharing");

    let fs_state = create_shared_state(fs_data_dir);

    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    rt.block_on(async {
        fs_state.load_config().await;
        fs_state.load_users().await;
    });

    let fs_state_clone = fs_state.clone();

    tauri::Builder::default()
        .manage(AppState {
            config: Mutex::new(config),
            config_path,
            file_sharing: fs_state,
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            get_data_root_path,
            set_data_root_path,
            open_data_folder,
            get_file_sharing_config,
            set_file_sharing_config,
            start_file_sharing,
            stop_file_sharing,
            get_file_sharing_status,
            get_file_sharing_users,
            force_logout_file_sharing_user,
            open_shared_folder,
            create_file_sharing_user,
            update_file_sharing_user,
            delete_file_sharing_user,
            get_file_sharing_logs,
            get_network_addresses,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                let config = state.config.lock().unwrap();
                if config.close_action == "hide" {
                    api.prevent_close();
                    drop(config);
                    let _ = window.hide();
                }
            }
        })
        .setup(move |app| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

            let icon = app.default_window_icon().cloned();
            let mut builder = TrayIconBuilder::new()
                .tooltip("JToolsBox")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                });

            if let Some(icon) = icon {
                builder = builder.icon(icon);
            }

            let _tray = builder.build(app)?;

            let fs_state = fs_state_clone.clone();
            tauri::async_runtime::spawn(async move {
                let should_start = {
                    let c = fs_state.config.read().await;
                    c.auto_start
                };
                if should_start {
                    let _ = fs_server::start_server(fs_state.clone()).await;
                }
            });

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
