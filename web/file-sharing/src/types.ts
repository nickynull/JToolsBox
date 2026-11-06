export interface FileEntry {
  id: string;
  name: string;
  path: string;
  size: number;
  is_dir: boolean;
  modified: string;
  mime_type: string | null;
}

export interface UserItem {
  id: string;
  username: string;
  role: string;
  created_at: string;
  online: boolean;
  last_login_at: string | null;
}

export interface LogEntry {
  timestamp: string;
  user: string;
  action: string;
  detail: string;
}

export interface LogDateInfo {
  date: string;
  size_bytes: number;
  current: boolean;
  /** 文件是否真实存在；界面为「今天」补的占位项为 false */
  exists: boolean;
}

export interface LogPage {
  date: string;
  entries: LogEntry[];
  /** 本次扫描到的有效记录数 */
  parsed: number;
  /** 文件过大，只读取了尾部 */
  tail_only: boolean;
  size_bytes: number;
}

export interface ServerStatus {
  running: boolean;
  port: number;
  url: string;
  auto_start: boolean;
  shared_folders: string[];
}

export interface Session {
  token: string;
  username: string;
  role: string;
}
