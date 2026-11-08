mod features;
mod web_support;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager};

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
    log_retention_days: Option<u64>,
}

#[tauri::command]
async fn get_file_sharing_config(state: tauri::State<'_, AppState>) -> Result<FileSharingConfig, String> {
    Ok(state.file_sharing.config.read().await.clone())
}

#[tauri::command]
async fn set_file_sharing_config(
    state: tauri::State<'_, AppState>,
    config: FileSharingConfigInput,
) -> Result<u64, String> {
    let mut current = state.file_sharing.config.write().await;
    current.port = config.port;
    current.shared_folders = config.shared_folders;
    current.token_duration_hours = config.token_duration_hours;
    if let Some(auto_start) = config.auto_start {
        current.auto_start = auto_start;
    }
    if let Some(retention) = config.log_retention_days {
        current.log_retention_days = retention.min(3650);
    }
    let retention = current.log_retention_days;
    drop(current);

    state.file_sharing.save_config().await?;
    // 保留天数变小后立即清理，返回被自动删除的文件数供界面提示
    Ok(state.file_sharing.cleanup_logs(retention).len() as u64)
}

#[tauri::command]
async fn start_file_sharing(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<u16, String> {
    let result = fs_server::start_server(state.file_sharing.clone()).await;
    refresh_tray_menu(&app, &state.file_sharing).await;
    result
}

#[tauri::command]
async fn stop_file_sharing(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let result = fs_server::stop_server(state.file_sharing.clone()).await;
    refresh_tray_menu(&app, &state.file_sharing).await;
    result
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
                &format!("强制下线：{}（{} 个会话）", username, removed),
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
        .add_log("admin", "create_user", &format!("新建用户：{}", input.username))
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
        .add_log("admin", "delete_user", &format!("删除用户：{}", id))
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

/// 日志目录里可查看的日期，供桌面端日期下拉使用
#[tauri::command]
async fn get_file_sharing_log_dates(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<file_sharing::LogDateInfo>, String> {
    Ok(state.file_sharing.list_log_dates())
}

#[tauri::command]
async fn read_file_sharing_log_date(
    state: tauri::State<'_, AppState>,
    date: String,
    limit: Option<usize>,
) -> Result<file_sharing::LogPage, String> {
    state.file_sharing.read_log_page(&date, limit).await
}

#[tauri::command]
async fn delete_file_sharing_log_dates(
    state: tauri::State<'_, AppState>,
    dates: Vec<String>,
) -> Result<file_sharing::LogDeleteResult, String> {
    if dates.is_empty() {
        return Err("请选择要删除的日志文件".to_string());
    }
    Ok(state.file_sharing.delete_logs(&dates).await)
}

#[derive(Serialize)]
struct NetworkInterface {
    name: String,
    ip: String,
    label: String,
    connected: bool,
    recommended: bool,
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

/// 虚拟网卡名称关键字（与接口名小写形式做包含匹配），命中的接口不对外展示。
/// 覆盖 VMware / VirtualBox / Hyper-V / Docker / WSL / TAP / VPN 隧道等常见虚拟适配器，
/// 以及 macOS 的 utun、Linux 的 bridge 等系统虚拟接口。
const VIRTUAL_ADAPTER_KEYWORDS: &[&str] = &[
    "vmnet", "vmware", "virtualbox", "vbox", "vethernet", "hyper-v", "hyperv",
    "docker", "wsl", "tap", "tun", "utun", "bridge", "br-", "veth", "virbr",
    "loopback", "tunnel", "pptp", "l2tp", "zero",
];

/// 接口名是否为虚拟网卡名称
fn is_virtual_adapter_name(name_lower: &str) -> bool {
    VIRTUAL_ADAPTER_KEYWORDS.iter().any(|kw| name_lower.contains(kw))
}

/// Windows 上以接口类型为兜底判断：PPP、隧道、软件回环均视为虚拟
#[cfg(windows)]
fn is_virtual_adapter_type(index: u32) -> bool {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetIfEntry2, MIB_IF_ROW2, IF_TYPE_PPP, IF_TYPE_SOFTWARE_LOOPBACK, IF_TYPE_TUNNEL,
    };

    unsafe {
        let mut row = MIB_IF_ROW2::default();
        row.InterfaceIndex = index;
        if GetIfEntry2(&mut row).0 != 0 {
            return false;
        }
        matches!(row.Type, IF_TYPE_PPP | IF_TYPE_SOFTWARE_LOOPBACK | IF_TYPE_TUNNEL)
    }
}

#[cfg(not(windows))]
fn is_virtual_adapter_type(_index: u32) -> bool {
    false
}

/// 接口展示标签：优先归一化为 WLAN / 以太网，其余用系统接口名
fn interface_label(name_lower: &str, raw_name: &str) -> String {
    if name_lower.contains("wlan")
        || name_lower.contains("wi-fi")
        || name_lower.contains("wifi")
        || name_lower.contains("无线")
    {
        "WLAN".to_string()
    } else if name_lower.contains("ethernet")
        || name_lower.contains("eth")
        || name_lower.contains("以太网")
        || name_lower.contains("本地连接")
    {
        "以太网".to_string()
    } else {
        raw_name.to_string()
    }
}

/// 通过 WMI 查询应展示的网卡连接名集合。
/// MSFT_NetAdapter.HardwareInterface 是 PowerShell `Get-NetAdapter -Physical` 使用的判定；
/// 实测比 Win32_NetworkAdapter.PhysicalAdapter 更严格，ZeroTier、节点小宝 NodeBabyLink
/// (Wintun)、蓝牙 PAN 等驱动自报物理的虚拟网卡在这里均为 False。
/// 额外放行 InterfaceDescription 为 Wi-Fi Direct Virtual Adapter 的接口：
/// 开启移动热点时它持有热点网段地址（如 192.168.137.1），关闭时没有可用 IPv4，
/// 会被 connected 过滤自然隐藏。
/// 返回 小写连接名 -> 是否热点接口；查询失败返回 None，调用方回退到启发式过滤
#[cfg(windows)]
fn query_displayable_adapters() -> Option<std::collections::HashMap<String, bool>> {
    let wmi_con = wmi::WMIConnection::with_namespace_path("ROOT\\StandardCimv2").ok()?;
    let rows: Vec<std::collections::HashMap<String, wmi::Variant>> = wmi_con
        .raw_query("SELECT Name, InterfaceDescription, HardwareInterface FROM MSFT_NetAdapter")
        .ok()?;

    let mut adapters = std::collections::HashMap::new();
    for row in rows {
        let name = match row.get("Name") {
            Some(wmi::Variant::String(s)) => s.to_lowercase(),
            _ => continue,
        };
        let hardware = matches!(row.get("HardwareInterface"), Some(wmi::Variant::Bool(true)));
        let hotspot = match row.get("InterfaceDescription") {
            Some(wmi::Variant::String(s)) => {
                s.to_lowercase().contains("wi-fi direct virtual adapter")
            }
            _ => false,
        };
        if hardware {
            adapters.insert(name, false);
        } else if hotspot {
            adapters.insert(name, true);
        }
    }
    (!adapters.is_empty()).then_some(adapters)
}

/// 以 UDP “连接”（不发包）询问系统访问外网会选用哪个本地源地址，
/// 即当前默认路由实际使用的网卡，用于标记推荐地址
fn probe_preferred_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(("8.8.8.8", 53)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(v4) if !v4.is_unspecified() => Some(v4.to_string()),
        _ => None,
    }
}

/// 列出所有物理网卡上的 IPv4 地址（含未连接的接口，由 connected 区分；
/// Windows 以 WMI HardwareInterface 名单为准，其余平台或 WMI 不可用时退回名称/类型启发式）
#[tauri::command]
fn get_network_addresses() -> Vec<NetworkInterface> {
    use network_interface::NetworkInterfaceConfig;

    let mut result: Vec<NetworkInterface> = Vec::new();

    #[cfg(windows)]
    let displayable = query_displayable_adapters();

    if let Ok(interfaces) = network_interface::NetworkInterface::show() {
        for iface in &interfaces {
            if iface.internal {
                continue;
            }

            let name_lower = iface.name.to_lowercase();

            // 网卡准入判定，并识别热点接口；WMI 不可用时退回名称关键字 + 接口类型启发式
            #[cfg(windows)]
            let is_hotspot = match displayable.as_ref() {
                Some(adapters) => match adapters.get(&name_lower) {
                    Some(&hotspot) => hotspot,
                    None => continue,
                },
                None => {
                    if is_virtual_adapter_name(&name_lower) || is_virtual_adapter_type(iface.index) {
                        continue;
                    }
                    name_lower.contains("wi-fi direct")
                }
            };
            #[cfg(not(windows))]
            let is_hotspot: bool = {
                if is_virtual_adapter_name(&name_lower) || is_virtual_adapter_type(iface.index) {
                    continue;
                }
                false
            };

            for addr in &iface.addr {
                if let network_interface::Addr::V4(v4) = addr {
                    if v4.ip.is_loopback() || v4.ip.is_multicast() {
                        continue;
                    }

                    let ip = v4.ip.to_string();
                    if result.iter().any(|r| r.ip == ip) {
                        continue;
                    }

                    result.push(NetworkInterface {
                        name: iface.name.clone(),
                        label: if is_hotspot {
                            "热点".to_string()
                        } else {
                            interface_label(&name_lower, &iface.name)
                        },
                        ip,
                        connected: is_interface_connected(iface.index, &v4.ip),
                        recommended: false,
                    });
                }
            }
        }
    }

    if let Some(preferred) = probe_preferred_ip() {
        if let Some(target) = result.iter_mut().find(|r| r.ip == preferred) {
            target.recommended = true;
        }
    }

    result.sort_by(|a, b| {
        b.recommended
            .cmp(&a.recommended)
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.ip.cmp(&b.ip))
    });
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

/// 托盘右键菜单里需要随服务状态刷新的菜单项（MenuItem 是 Arc 句柄，克隆即共享）
struct TrayMenuItems {
    status: MenuItem<tauri::Wry>,
    start: MenuItem<tauri::Wry>,
    stop: MenuItem<tauri::Wry>,
}

/// 按服务真实状态刷新托盘菜单：状态文案 + 启停项的可用性，
/// 桌面端界面里启停时也会调用，保证菜单状态与界面一致
async fn refresh_tray_menu(app: &AppHandle, fs_state: &std::sync::Arc<FileSharingState>) {
    let (running, port) = {
        let status = fs_state.status.read().await;
        (status.running, status.port)
    };

    if let Some(items) = app.try_state::<TrayMenuItems>() {
        let text = if running {
            format!("运行中 · 端口 {}", port)
        } else {
            "已停止".to_string()
        };
        let _ = items.status.set_text(text);
        let _ = items.start.set_enabled(!running);
        let _ = items.stop.set_enabled(running);
    }
}

/// 托盘菜单被点击：启停分享服务 / 退出程序。
/// 菜单点击发生在主线程，启停是异步操作，放到异步运行时里执行，完成后回推菜单与前端状态
fn handle_tray_menu(app: &AppHandle, id: &str) {
    match id {
        "fs-start" | "fs-stop" => {
            let starting = id == "fs-start";
            let fs_state = app.state::<AppState>().file_sharing.clone();
            let app_handle = app.clone();

            tauri::async_runtime::spawn(async move {
                let outcome = if starting {
                    fs_server::start_server(fs_state.clone())
                        .await
                        .map(|port| format!("文件分享已启动，端口 {}", port))
                } else {
                    fs_server::stop_server(fs_state.clone())
                        .await
                        .map(|_| "文件分享已停止".to_string())
                };

                let level = if outcome.is_ok() { "success" } else { "error" };
                let message = outcome.unwrap_or_else(|e| e);
                let _ = app_handle.emit(
                    "tray-toast",
                    serde_json::json!({ "level": level, "message": message }),
                );

                refresh_tray_menu(&app_handle, &fs_state).await;
                // 桌面端界面据此刷新「当前状态」和访问地址
                let _ = app_handle.emit("file-sharing-status-changed", serde_json::json!({}));
            });
        }
        "quit" => app.exit(0),
        _ => {}
    }
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
        fs_state.load_logs().await;
        // 启动时先按保留天数清一次过期日志
        let retention = fs_state.config.read().await.log_retention_days;
        fs_state.cleanup_logs(retention);
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
            get_file_sharing_log_dates,
            read_file_sharing_log_date,
            delete_file_sharing_log_dates,
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

            // 右键菜单：文件分享（状态 / 开启 / 停止）+ 退出程序
            let status_item = MenuItem::with_id(app, "fs-status", "已停止", false, None::<&str>)?;
            let start_item = MenuItem::with_id(app, "fs-start", "开启服务", true, None::<&str>)?;
            let stop_item = MenuItem::with_id(app, "fs-stop", "停止服务", false, None::<&str>)?;
            let fs_menu = Submenu::with_items(
                app,
                "文件分享",
                true,
                &[
                    &status_item,
                    &PredefinedMenuItem::separator(app)?,
                    &start_item,
                    &stop_item,
                ],
            )?;
            let quit_item = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;
            let tray_menu = Menu::with_items(
                app,
                &[
                    &fs_menu,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_item,
                ],
            )?;

            // 供 refresh_tray_menu 在启停后改文案与可用状态
            app.manage(TrayMenuItems {
                status: status_item.clone(),
                start: start_item.clone(),
                stop: stop_item.clone(),
            });

            let mut builder = TrayIconBuilder::new()
                .tooltip("JToolsBox")
                .menu(&tray_menu)
                // 左键仍用于唤起主窗口，菜单只在右键弹出
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| handle_tray_menu(app, event.id().as_ref()))
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
            let tray_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                refresh_tray_menu(&tray_app, &fs_state).await;

                let should_start = {
                    let c = fs_state.config.read().await;
                    c.auto_start
                };
                if should_start {
                    let _ = fs_server::start_server(fs_state.clone()).await;
                    refresh_tray_menu(&tray_app, &fs_state).await;
                    let _ = tray_app.emit("file-sharing-status-changed", serde_json::json!({}));
                }
            });

            // 跨天长时间运行时定时清理过期日志，启动阶段已经清过一次，这里跳过首拍
            let cleanup_state = fs_state_clone.clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(6 * 60 * 60));
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let retention = cleanup_state.config.read().await.log_retention_days;
                    cleanup_state.cleanup_logs(retention);
                }
            });

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
