pub mod auth;
pub mod server;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileSharingConfig {
    pub port: u16,
    pub shared_folders: Vec<String>,
    pub jwt_secret: String,
    pub token_duration_hours: u64,
    pub auto_start: bool,
    /// 日志保留天数（含今天），0 表示永久保留
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u64,
}

fn default_log_retention_days() -> u64 {
    30
}

impl Default for FileSharingConfig {
    fn default() -> Self {
        Self {
            port: 9800,
            shared_folders: vec![],
            jwt_secret: uuid::Uuid::new_v4().to_string(),
            token_duration_hours: 24,
            auto_start: false,
            log_retention_days: default_log_retention_days(),
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

/// 读取某个日期日志文件的结果
#[derive(Serialize, Clone, Debug)]
pub struct LogFileRead {
    /// 时间正序的有效记录，已按上限丢弃最早的
    pub entries: Vec<LogEntry>,
    /// 本次扫描到的有效记录总数
    pub parsed: usize,
    /// 文件超过扫描上限，只读取了尾部
    pub tail_only: bool,
    pub size_bytes: u64,
}

/// 日志目录里可用的日期文件
#[derive(Serialize, Clone, Debug)]
pub struct LogDateInfo {
    pub date: String,
    pub size_bytes: u64,
    /// 是否为今天的文件
    pub current: bool,
    /// 文件是否真实存在；界面为「今天」补的占位项该字段为 false
    pub exists: bool,
}

/// 按日期展示的一页日志，桌面命令与网页端接口共用
#[derive(Serialize, Clone, Debug)]
pub struct LogPage {
    pub date: String,
    /// 时间倒序，最新在前
    pub entries: Vec<LogEntry>,
    /// 本次扫描到的有效记录数
    pub parsed: usize,
    /// 文件超过扫描上限，只读取了尾部
    pub tail_only: bool,
    pub size_bytes: u64,
}

/// 删除日志文件的结果：逐项回报，部分失败不影响其余文件
#[derive(Serialize, Clone, Debug)]
pub struct LogDeleteResult {
    pub deleted: Vec<String>,
    pub failed: Vec<LogDeleteFailure>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LogDeleteFailure {
    pub date: String,
    pub reason: String,
}

/// 单次返回的日志条数默认值与上限
pub const LOG_PAGE_DEFAULT_LIMIT: usize = 500;
pub const LOG_PAGE_MAX_LIMIT: usize = 2000;

/// 内存里保留的日志条数上限，超出后丢弃最早的记录
pub const LOG_MAX_ENTRIES: usize = 10_000;

/// 单次扫描的字节上限，更大的文件只读尾部，避免启动或查询卡住界面
pub const LOG_SCAN_LIMIT_BYTES: u64 = 32 * 1024 * 1024;

/// 流式读取 JSONL 日志：内存只保留最后 keep 条，坏行跳过，超大文件只扫描尾部
fn read_log_file(path: &Path, keep: usize) -> Result<LogFileRead, String> {
    read_log_file_with_limit(path, keep, LOG_SCAN_LIMIT_BYTES)
}

fn read_log_file_with_limit(
    path: &Path,
    keep: usize,
    scan_limit_bytes: u64,
) -> Result<LogFileRead, String> {
    use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};

    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        // 该日期没有文件不算错误，界面显示空列表即可
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LogFileRead {
                entries: vec![],
                parsed: 0,
                tail_only: false,
                size_bytes: 0,
            })
        }
        Err(e) => return Err(e.to_string()),
    };

    let size_bytes = file.metadata().map_err(|e| e.to_string())?.len();
    let mut reader = BufReader::new(file);
    let tail_only = size_bytes > scan_limit_bytes;

    // 落点不在行首时，尾部扫描的第一行是残片，需要丢弃
    let mut skip_first_line = false;
    if tail_only {
        let start = size_bytes - scan_limit_bytes;
        let mut probe = [0u8; 1];
        let at_line_start = reader
            .seek(SeekFrom::Start(start - 1))
            .and_then(|_| reader.read_exact(&mut probe))
            .map(|_| probe[0] == b'\n')
            .unwrap_or(false);
        reader.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
        skip_first_line = !at_line_start;
    }

    let mut entries: VecDeque<LogEntry> = VecDeque::new();
    let mut parsed = 0usize;
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);

    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf).map_err(|e| e.to_string())? == 0 {
            break;
        }
        if skip_first_line {
            skip_first_line = false;
            continue;
        }
        // 截断落点可能切断多字节字符，用 lossy 解码而不是直接报错
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 单行损坏不影响其余记录
        let Ok(entry) = serde_json::from_str::<LogEntry>(line) else {
            continue;
        };
        parsed += 1;
        entries.push_back(entry);
        if entries.len() > keep {
            entries.pop_front();
        }
    }

    Ok(LogFileRead {
        entries: entries.into_iter().collect(),
        parsed,
        tail_only,
        size_bytes,
    })
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

    /// 当天日志文件的日期名
    pub fn today_log_date() -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }

    /// 校验日期形如 YYYY-MM-DD，防止拼出越出日志目录的路径
    pub fn log_file_path(&self, date: &str) -> Option<PathBuf> {
        if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            return None;
        }
        Some(self.logs_dir().join(format!("{}.jsonl", date)))
    }

    /// 启动时把当天日志回读进内存缓冲，否则每次打开软件操作日志都是空的
    pub async fn load_logs(&self) {
        let path = match self.log_file_path(&Self::today_log_date()) {
            Some(path) => path,
            None => return,
        };
        let read = match tokio::task::spawn_blocking(move || read_log_file(&path, LOG_MAX_ENTRIES)).await
        {
            Ok(Ok(read)) => read,
            // 回读失败只影响历史记录，不能阻止软件启动
            _ => return,
        };

        let mut logs = self.logs.write().await;
        // 仅在缓冲为空时回填，避免覆盖运行期间新写入的记录
        if logs.is_empty() {
            *logs = read.entries.into_iter().collect();
        }
    }

    /// 读取指定日期的日志文件，供桌面端按日期查看
    pub async fn read_logs_of_date(&self, date: &str, max_entries: usize) -> Result<LogFileRead, String> {
        let path = self
            .log_file_path(date)
            .ok_or_else(|| "日期格式不正确".to_string())?;
        let keep = max_entries.clamp(1, LOG_MAX_ENTRIES);
        tokio::task::spawn_blocking(move || read_log_file(&path, keep))
            .await
            .map_err(|_| "读取日志失败".to_string())?
    }

    /// 读取指定日期的一页日志，桌面命令与网页端接口共用
    pub async fn read_log_page(&self, date: &str, limit: Option<usize>) -> Result<LogPage, String> {
        let limit = limit.unwrap_or(LOG_PAGE_DEFAULT_LIMIT).clamp(1, LOG_PAGE_MAX_LIMIT);
        let read = self.read_logs_of_date(date, limit).await?;
        Ok(LogPage {
            date: date.to_string(),
            entries: read.entries.iter().rev().take(limit).cloned().collect(),
            parsed: read.parsed,
            tail_only: read.tail_only,
            size_bytes: read.size_bytes,
        })
    }

    /// 删除指定日期的日志文件；文件已不存在视为删除成功，便于重复操作
    pub async fn delete_logs(&self, dates: &[String]) -> LogDeleteResult {
        let mut deleted: Vec<String> = Vec::new();
        let mut failed: Vec<LogDeleteFailure> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for date in dates {
            if !seen.insert(date.as_str()) {
                continue;
            }
            let Some(path) = self.log_file_path(date) else {
                failed.push(LogDeleteFailure {
                    date: date.clone(),
                    reason: "日期格式不正确".to_string(),
                });
                continue;
            };
            match std::fs::remove_file(&path) {
                Ok(()) => deleted.push(date.clone()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => deleted.push(date.clone()),
                // 文件正被写入时 Windows 会拒绝删除，原样回报系统错误
                Err(e) => failed.push(LogDeleteFailure {
                    date: date.clone(),
                    reason: e.to_string(),
                }),
            }
        }

        // 当天的文件被删掉后同步清空内存缓冲，避免接口还能读到已删除的记录
        if deleted.iter().any(|date| *date == Self::today_log_date()) {
            self.logs.write().await.clear();
        }

        LogDeleteResult { deleted, failed }
    }

    /// 删除超出保留天数的日志文件；保留最近 N 天（含今天），0 表示永久保留
    pub fn cleanup_logs(&self, retention_days: u64) -> Vec<String> {
        if retention_days == 0 {
            return vec![];
        }
        let today = chrono::Local::now().date_naive();
        // 保留今天在内的 N 天，早于截止日的文件才删除
        let Some(cutoff) = today.checked_sub_days(chrono::Days::new(retention_days.saturating_sub(1)))
        else {
            return vec![];
        };

        let mut deleted = Vec::new();
        for info in self.list_log_dates() {
            let Ok(date) = chrono::NaiveDate::parse_from_str(&info.date, "%Y-%m-%d") else {
                continue;
            };
            if date >= cutoff {
                continue;
            }
            // 列表里的日期都已校验过，这里只用文件名删除，不会越出日志目录
            if let Some(path) = self.log_file_path(&info.date) {
                if std::fs::remove_file(path).is_ok() {
                    deleted.push(info.date);
                }
            }
        }
        deleted
    }

    /// 列出日志目录里可用的日期，按日期倒序
    pub fn list_log_dates(&self) -> Vec<LogDateInfo> {
        let today = Self::today_log_date();
        let Ok(dir) = std::fs::read_dir(self.logs_dir()) else {
            return vec![];
        };

        let mut dates: Vec<LogDateInfo> = dir
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let date = name.strip_suffix(".jsonl")?;
                chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
                Some(LogDateInfo {
                    date: date.to_string(),
                    size_bytes: entry.metadata().ok().map(|meta| meta.len()).unwrap_or(0),
                    current: date == today,
                    exists: true,
                })
            })
            .collect();
        dates.sort_by(|a, b| b.date.cmp(&a.date));
        dates
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
        if logs.len() > LOG_MAX_ENTRIES {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_line(user: &str, action: &str, detail: &str) -> String {
        serde_json::to_string(&LogEntry {
            timestamp: "2026-11-06T13:19:33+08:00".to_string(),
            user: user.to_string(),
            action: action.to_string(),
            detail: detail.to_string(),
        })
        .unwrap()
    }

    fn temp_state(tag: &str) -> (Arc<FileSharingState>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("jtoolsbox-log-{}-{}", tag, uuid::Uuid::new_v4()));
        let state = create_shared_state(dir.clone());
        (state, dir)
    }

    fn write_log_file(state: &FileSharingState, date: &str, content: &str) -> PathBuf {
        let path = state.log_file_path(date).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parses_valid_lines_and_skips_broken_ones() {
        let (state, dir) = temp_state("parse");
        let content = format!(
            "{}\n{}\n不是 JSON\n   \n{}\n",
            sample_line("system", "server_start", "服务已启动，监听端口 9898"),
            sample_line("jimmy", "upload", "上传：a.txt（12 字节）"),
            sample_line("admin", "delete", "删除：b.txt"),
        );
        let path = write_log_file(&state, "2026-11-06", &content);

        let read = read_log_file(&path, 100).unwrap();
        assert_eq!(read.parsed, 3);
        assert_eq!(read.entries.len(), 3);
        assert_eq!(read.entries[1].detail, "上传：a.txt（12 字节）");
        assert!(!read.tail_only);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_file_reads_empty_instead_of_erroring() {
        let (state, dir) = temp_state("missing");
        let path = state.log_file_path("2026-01-01").unwrap();

        let read = read_log_file(&path, 100).unwrap();
        assert!(read.entries.is_empty());
        assert_eq!(read.size_bytes, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keep_only_retains_the_latest_entries() {
        let (state, dir) = temp_state("keep");
        let lines: Vec<String> = (0..10)
            .map(|i| sample_line("user", "download", &format!("下载：文件 {}", i)))
            .collect();
        let content = lines.iter().map(|l| format!("{}\n", l)).collect::<String>();
        let path = write_log_file(&state, "2026-11-06", &content);

        let read = read_log_file(&path, 3).unwrap();
        // parsed 统计全部有效行，entries 只保留最后 3 条
        assert_eq!(read.parsed, 10);
        assert_eq!(read.entries.len(), 3);
        assert_eq!(read.entries[2].detail, "下载：文件 9");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tail_scan_drops_the_partial_first_line() {
        let (state, dir) = temp_state("tail");
        let lines: Vec<String> = (0..6)
            .map(|i| sample_line("user", "upload", &format!("上传：第 {} 个文件", i)))
            .collect();
        let content = lines.iter().map(|l| format!("{}\n", l)).collect::<String>();
        let path = write_log_file(&state, "2026-11-06", &content);
        let size = content.len() as u64;
        let line_len = lines[0].len() as u64 + 1;

        // 落点落在第 2 行中间：残片被丢弃，只能读到第 3 行之后
        let read = read_log_file_with_limit(&path, 100, size - (line_len + 3)).unwrap();
        assert!(read.tail_only);
        assert_eq!(read.entries.len(), 4);
        assert_eq!(read.entries[0].detail, "上传：第 2 个文件");

        // 落点正好在行首：该行不应被当成残片丢掉
        let aligned = read_log_file_with_limit(&path, 100, size - line_len).unwrap();
        assert!(aligned.tail_only);
        assert_eq!(aligned.entries.len(), 5);
        assert_eq!(aligned.entries[0].detail, "上传：第 1 个文件");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_date_and_traversal_are_rejected() {
        let (state, dir) = temp_state("date");
        assert!(state.log_file_path("2026-11-06").is_some());
        assert!(state.log_file_path("../../windows/system32/config").is_none());
        assert!(state.log_file_path("2026-13-45").is_none());
        assert!(state.log_file_path("").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn startup_loads_todays_log_into_memory() {
        let (state, dir) = temp_state("startup");
        let today = FileSharingState::today_log_date();
        let content = format!(
            "{}\n{}\n",
            sample_line("system", "server_start", "服务已启动，监听端口 9898"),
            sample_line("system", "server_stop", "服务已停止"),
        );
        std::fs::write(state.log_file_path(&today).unwrap(), content).unwrap();

        state.load_logs().await;
        let logs = state.logs.read().await;
        assert_eq!(logs.len(), 2);
        assert_eq!(logs.back().unwrap().detail, "服务已停止");
        drop(logs);

        // 缓冲非空时不重复回填，避免覆盖运行期新写入的记录
        state.add_log("system", "login", "登录成功").await;
        state.load_logs().await;
        let logs = state.logs.read().await;
        assert_eq!(logs.len(), 3);
        assert_eq!(logs.back().unwrap().action, "login");
        drop(logs);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn reads_history_by_date_and_lists_dates() {
        let (state, dir) = temp_state("dates");
        write_log_file(&state, "2020-01-02", &format!("{}\n", sample_line("a", "login", "登录成功")));
        write_log_file(&state, "2020-01-01", &format!("{}\n", sample_line("b", "logout", "退出登录")));
        std::fs::write(dir.join("logs").join("notes.txt"), "忽略").unwrap();

        let dates = state.list_log_dates();
        assert_eq!(dates.len(), 2);
        assert_eq!(dates[0].date, "2020-01-02");
        assert_eq!(dates[1].date, "2020-01-01");
        assert!(dates.iter().all(|d| !d.current));

        let read = state.read_logs_of_date("2020-01-01", 500).await.unwrap();
        assert_eq!(read.entries[0].user, "b");

        // 非法日期直接报错，不会去读目录外的文件
        assert!(state.read_logs_of_date("../config", 500).await.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn log_page_is_newest_first_and_respects_the_limit() {
        let (state, dir) = temp_state("page");
        let lines: Vec<String> = (0..5)
            .map(|i| sample_line("user", "download", &format!("下载：文件 {}", i)))
            .collect();
        let content = lines.iter().map(|l| format!("{}\n", l)).collect::<String>();
        write_log_file(&state, "2020-02-02", &content);

        let page = state.read_log_page("2020-02-02", Some(2)).await.unwrap();
        assert_eq!(page.entries.len(), 2);
        // 最新在前，parsed 仍是全部有效条数
        assert_eq!(page.entries[0].detail, "下载：文件 4");
        assert_eq!(page.parsed, 5);
        assert_eq!(page.date, "2020-02-02");

        // 超出上限的 limit 会被夹到最大值，而不是原样使用
        let clamped = state.read_log_page("2020-02-02", Some(999_999)).await.unwrap();
        assert_eq!(clamped.entries.len(), 5);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn delete_logs_reports_per_file_result() {
        let (state, dir) = temp_state("delete");
        write_log_file(&state, "2020-05-05", "{}\n");
        write_log_file(&state, "2020-05-01", "{}\n");
        let keep = write_log_file(&state, "2020-05-03", "{}\n");

        let input = vec![
            "2020-05-05".to_string(),
            "2020-05-01".to_string(),
            "2020-05-05".to_string(), // 重复日期只处理一次
            "2020-06-06".to_string(), // 文件不存在，按已删除处理
            "../../windows".to_string(),
        ];
        let result = state.delete_logs(&input).await;

        assert_eq!(result.deleted, vec!["2020-05-05", "2020-05-01", "2020-06-06"]);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].date, "../../windows");
        assert_eq!(result.failed[0].reason, "日期格式不正确");

        // 列表里只剩没被选中的那天，且文件确实还在
        let dates: Vec<String> = state.list_log_dates().into_iter().map(|d| d.date).collect();
        assert_eq!(dates, vec!["2020-05-03"]);
        assert!(keep.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn deleting_todays_file_clears_the_memory_buffer() {
        let (state, dir) = temp_state("delete-today");
        let today = FileSharingState::today_log_date();
        state.add_log("system", "server_start", "服务已启动").await;
        assert_eq!(state.logs.read().await.len(), 1);

        let result = state.delete_logs(&[today.clone()]).await;
        assert_eq!(result.deleted, vec![today.clone()]);
        assert!(state.logs.read().await.is_empty());
        assert!(!state.log_file_path(&today).unwrap().exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cleanup_removes_files_outside_the_retention_window() {
        let (state, dir) = temp_state("cleanup");
        let today = chrono::Local::now().date_naive();
        let day = |offset: u64| (today - chrono::Days::new(offset)).format("%Y-%m-%d").to_string();

        // 保留最近 3 天（含今天）：今天、-1、-2 保留，-3、-10 删除
        for offset in [0u64, 1, 2, 3, 10] {
            write_log_file(&state, &day(offset), "{}\n");
        }

        let deleted = state.cleanup_logs(3);
        assert_eq!(deleted.len(), 2);
        let remaining: Vec<String> = state.list_log_dates().into_iter().map(|item| item.date).collect();
        assert_eq!(remaining, vec![day(0), day(1), day(2)]);

        // 0 表示永久保留，不会删任何文件
        assert!(state.cleanup_logs(0).is_empty());
        assert_eq!(state.list_log_dates().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn old_config_without_retention_field_keeps_other_values() {
        // 老版本 config.json 没有 log_retention_days，反序列化必须成功并补默认值
        let json = r#"{"port":9900,"shared_folders":["D:\\share"],"jwt_secret":"s","token_duration_hours":12,"auto_start":true}"#;
        let config: FileSharingConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.port, 9900);
        assert_eq!(config.token_duration_hours, 12);
        assert!(config.auto_start);
        assert_eq!(config.log_retention_days, 30);
    }
}

