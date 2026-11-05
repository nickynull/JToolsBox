import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export type ThemeMode = 'light' | 'dark' | 'system'
export type CloseAction = 'hide' | 'exit'

export interface AppConfig {
  theme: ThemeMode
  close_action: CloseAction
  data_root_path: string | null
}

export interface FileSharingConfig {
  port: number
  shared_folders: string[]
  jwt_secret: string
  token_duration_hours: number
  auto_start: boolean
}

export interface FileSharingStatus {
  running: boolean
  port: number
  url: string
  auto_start: boolean
}

export interface FileSharingUser {
  id: string
  username: string
  role: string
  created_at: string
  online: boolean
  last_login_at: string | null
}

export interface LogEntry {
  timestamp: string
  user: string
  action: string
  detail: string
}

export interface NetworkInterface {
  name: string
  ip: string
  label: string
  connected: boolean
}

const config = ref<AppConfig>({
  theme: 'system',
  close_action: 'exit',
  data_root_path: null,
})

const resolvedTheme = ref<'light' | 'dark'>('light')

let initialized = false
let mediaQuery: MediaQueryList | null = null
let mediaHandler: (() => void) | null = null

function getSystemTheme(): 'light' | 'dark' {
  if (typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: dark)').matches) {
    return 'dark'
  }
  return 'light'
}

function applyTheme(theme: 'light' | 'dark') {
  const root = document.documentElement
  root.setAttribute('data-theme', theme)
  // Element Plus 的暗色变量挂在 html.dark 上，需要与 data-theme 同步
  root.classList.toggle('dark', theme === 'dark')
  resolvedTheme.value = theme
}

function resolveAndApplyTheme() {
  const theme = config.value.theme === 'system' ? getSystemTheme() : config.value.theme
  applyTheme(theme)
}

export async function initSettings() {
  if (initialized) return
  initialized = true

  try {
    const saved = await invoke<AppConfig>('get_config')
    config.value = saved
  } catch {
    // use defaults
  }

  resolveAndApplyTheme()

  mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
  mediaHandler = () => {
    if (config.value.theme === 'system') {
      resolveAndApplyTheme()
    }
  }
  mediaQuery.addEventListener('change', mediaHandler)

  watch(config, (newConfig) => {
    invoke('set_config', { config: newConfig }).catch(console.error)
    resolveAndApplyTheme()
  }, { deep: true })
}

export async function getDataRootPath(): Promise<string> {
  return invoke<string>('get_data_root_path')
}

export async function setDataRootPath(path: string): Promise<void> {
  return invoke('set_data_root_path', { path })
}

export async function openDataFolder(): Promise<void> {
  return invoke('open_data_folder')
}

export function useSettings() {
  function setTheme(mode: ThemeMode) {
    config.value.theme = mode
  }

  function setCloseAction(action: CloseAction) {
    config.value.close_action = action
  }

  return {
    config,
    resolvedTheme,
    setTheme,
    setCloseAction,
  }
}

export async function getFileSharingConfig(): Promise<FileSharingConfig> {
  return invoke<FileSharingConfig>('get_file_sharing_config')
}

export async function setFileSharingConfig(cfg: {
  port: number
  shared_folders: string[]
  token_duration_hours: number
  auto_start?: boolean
}): Promise<void> {
  return invoke('set_file_sharing_config', { config: cfg })
}

export async function startFileSharing(): Promise<number> {
  return invoke<number>('start_file_sharing')
}

export async function stopFileSharing(): Promise<void> {
  return invoke('stop_file_sharing')
}

export async function getFileSharingStatus(): Promise<FileSharingStatus> {
  return invoke<FileSharingStatus>('get_file_sharing_status')
}

export async function getFileSharingUsers(): Promise<FileSharingUser[]> {
  return invoke<FileSharingUser[]>('get_file_sharing_users')
}

export async function forceLogoutFileSharingUser(username: string): Promise<number> {
  return invoke<number>('force_logout_file_sharing_user', { username })
}

export async function openSharedFolder(path: string): Promise<void> {
  return invoke('open_shared_folder', { path })
}

export async function createFileSharingUser(input: {
  username: string
  password: string
  role?: string
}): Promise<FileSharingUser> {
  return invoke<FileSharingUser>('create_file_sharing_user', { input })
}

export async function updateFileSharingUser(input: {
  id: string
  username?: string
  password?: string
  role?: string
}): Promise<void> {
  return invoke('update_file_sharing_user', { input })
}

export async function deleteFileSharingUser(id: string): Promise<void> {
  return invoke('delete_file_sharing_user', { id })
}

export async function getFileSharingLogs(limit?: number): Promise<LogEntry[]> {
  return invoke<LogEntry[]>('get_file_sharing_logs', { limit })
}

export async function getNetworkAddresses(): Promise<NetworkInterface[]> {
  return invoke<NetworkInterface[]>('get_network_addresses')
}
