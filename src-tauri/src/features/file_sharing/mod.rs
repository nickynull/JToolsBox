pub mod auth;
pub mod server;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileSharingConfig {
    pub port: u16,
    pub shared_folders: Vec<String>,
    pub jwt_secret: String,
    pub token_duration_hours: u64,
    pub auto_start: bool,
}

impl Default for FileSharingConfig {
    fn default() -> Self {
        Self {
            port: 9800,
            shared_folders: vec![],
            jwt_secret: uuid::Uuid::new_v4().to_string(),
            token_duration_hours: 24,
            auto_start: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UserList {
    pub users: Vec<User>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub modified: String,
    pub mime_type: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LogEntry {
    pub timestamp: String,
    pub user: String,
    pub action: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct ServerStatus {
    pub running: bool,
    pub port: u16,
    pub url: Option<String>,
}

impl Default for ServerStatus {
    fn default() -> Self {
        Self {
            running: false,
            port: 0,
            url: None,
        }
    }
}

/// 最近活动在该窗口内的会话视为在线（网页端约 60 秒一次心跳）
pub const SESSION_ONLINE_WINDOW: std::time::Duration = std::time::Duration::from_secs(180);

/// 进行中的上传占位：开始上传时登记目标文件，防止同名并发上传
#[derive(Clone, Debug)]
pub struct UploadReservation {
    pub upload_id: String,
    pub username: String,
    pub target: PathBuf,
    pub temp: PathBuf,
    pub last_active: std::time::Instant,
}

/// 登录会话：JWT 中的 jti 对应一条记录，支持查看在线状态与强制下线
#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub user_id: String,
    pub username: String,
    pub created_at: String,
    pub expires_at: std::time::Instant,
    pub last_active: std::time::Instant,
}

pub struct FileSharingState {
    pub config: RwLock<FileSharingConfig>,
    pub users: RwLock<Vec<User>>,
    pub status: RwLock<ServerStatus>,
    pub logs: RwLock<VecDeque<LogEntry>>,
    pub uploads: tokio::sync::Mutex<HashMap<String, UploadReservation>>,
    pub sessions: tokio::sync::Mutex<HashMap<String, SessionInfo>>,
    pub shutdown_tx: RwLock<Option<tokio::sync::broadcast::Sender<()>>>,
    pub data_dir: PathBuf,
}

impl FileSharingState {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            config: RwLock::new(FileSharingConfig::default()),
            users: RwLock::new(vec![]),
            status: RwLock::new(ServerStatus::default()),
            logs: RwLock::new(VecDeque::new()),
            uploads: tokio::sync::Mutex::new(HashMap::new()),
            sessions: tokio::sync::Mutex::new(HashMap::new()),
            shutdown_tx: RwLock::new(None),
            data_dir,
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }

    pub fn users_path(&self) -> PathBuf {
        self.data_dir.join("users.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub async fn load_config(&self) {
        if let Ok(content) = std::fs::read_to_string(self.config_path()) {
            if let Ok(config) = serde_json::from_str::<FileSharingConfig>(&content) {
                *self.config.write().await = config;
            }
        }
    }

    pub async fn save_config(&self) -> Result<(), String> {
        let config = self.config.read().await.clone();
        let content = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        std::fs::write(self.config_path(), content).map_err(|e| e.to_string())
    }

    pub async fn load_users(&self) {
        if let Ok(content) = std::fs::read_to_string(self.users_path()) {
            if let Ok(user_list) = serde_json::from_str::<UserList>(&content) {
                *self.users.write().await = user_list.users;
            }
        }
    }

    pub async fn save_users(&self) -> Result<(), String> {
        let users = self.users.read().await.clone();
        let user_list = UserList { users };
        let content = serde_json::to_string_pretty(&user_list).map_err(|e| e.to_string())?;
        std::fs::write(self.users_path(), content).map_err(|e| e.to_string())
    }

    pub async fn add_log(&self, user: &str, action: &str, detail: &str) {
        let entry = LogEntry {
            timestamp: chrono::Local::now().to_rfc3339(),
            user: user.to_string(),
            action: action.to_string(),
            detail: detail.to_string(),
        };

        let mut logs = self.logs.write().await;
        logs.push_back(entry.clone());
        if logs.len() > 10000 {
            logs.pop_front();
        }
        drop(logs);

        let logs_dir = self.logs_dir();
        let _ = std::fs::create_dir_all(&logs_dir);
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let log_file = logs_dir.join(format!("{}.jsonl", date));
        let line = serde_json::to_string(&entry).unwrap_or_default();
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{}", line)
            });
    }

    pub fn ensure_dirs(&self) {
        let _ = std::fs::create_dir_all(&self.data_dir);
        let _ = std::fs::create_dir_all(self.logs_dir());
    }
}

pub fn create_shared_state(data_dir: PathBuf) -> Arc<FileSharingState> {
    let state = FileSharingState::new(data_dir);
    state.ensure_dirs();
    Arc::new(state)
}
