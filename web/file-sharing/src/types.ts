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
