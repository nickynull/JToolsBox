<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessageBox } from 'element-plus'
import { Delete, CopyDocument, Folder, FolderOpened, Link, Search } from '@element-plus/icons-vue'
import { openUrl } from '@tauri-apps/plugin-opener'
import { notify } from '../composables/useNotify'
import QRCode from 'qrcode'
import {
  getFileSharingConfig,
  setFileSharingConfig,
  startFileSharing,
  stopFileSharing,
  getFileSharingStatus,
  getFileSharingUsers,
  forceLogoutFileSharingUser,
  openSharedFolder,
  createFileSharingUser,
  updateFileSharingUser,
  deleteFileSharingUser,
  getFileSharingLogDates,
  readFileSharingLogDate,
  deleteFileSharingLogDates,
  getNetworkAddresses,
  type FileSharingConfig,
  type FileSharingStatus,
  type FileSharingUser,
  type LogDateInfo,
  type LogEntry,
  type NetworkInterface,
} from '../composables/useSettings'

const activeTab = ref<'server' | 'network' | 'users' | 'logs'>('server')

const fsConfig = ref<FileSharingConfig>({
  port: 9800,
  shared_folders: [],
  jwt_secret: '',
  token_duration_hours: 24,
  auto_start: false,
  log_retention_days: 30,
})

const fsStatus = ref<FileSharingStatus>({
  running: false,
  port: 0,
  url: '',
  auto_start: false,
})

const users = ref<FileSharingUser[]>([])
const logs = ref<LogEntry[]>([])
const logDates = ref<LogDateInfo[]>([])
const selectedLogDate = ref('')
const logParsed = ref(0)
const logTailOnly = ref(false)
const logActionFilter = ref('')
const logKeyword = ref('')
const showLogFilesModal = ref(false)
const selectedLogFiles = ref<string[]>([])
const logFilesDeleting = ref(false)
const newFolder = ref('')
const saving = ref(false)
const userSaving = ref(false)
const serverActionLoading = ref(false)
const addressesLoading = ref(false)
const logsLoading = ref(false)
const usersLoading = ref(false)

const onlineCount = computed(() => users.value.filter((user) => user.online).length)

// 已保存配置的可序列化快照；null 表示尚未载入，避免载入前误报未保存
const savedSnapshot = ref<string | null>(null)

function currentSnapshot() {
  return JSON.stringify({
    port: fsConfig.value.port,
    shared_folders: fsConfig.value.shared_folders,
    token_duration_hours: fsConfig.value.token_duration_hours,
    auto_start: fsConfig.value.auto_start,
    log_retention_days: fsConfig.value.log_retention_days,
  })
}

const isDirty = computed(() => savedSnapshot.value !== null && currentSnapshot() !== savedSnapshot.value)

const showUserModal = ref(false)
const editingUser = ref<FileSharingUser | null>(null)
const userForm = ref({ username: '', password: '', role: 'user' })

const networkAddresses = ref<NetworkInterface[]>([])
const qrCodeUrls = ref<Record<string, string>>({})

// 仅展示已连接的物理网卡地址（虚拟网卡已在后端按 WMI PhysicalAdapter 过滤）
const activeAddresses = computed(() => networkAddresses.value.filter((iface) => iface.connected))

async function loadConfig() {
  try {
    fsConfig.value = await getFileSharingConfig()
    savedSnapshot.value = currentSnapshot()
  } catch (e) {
    console.error('Failed to load config:', e)
    notify('读取配置失败: ' + e, 'error')
  }
}

async function loadStatus() {
  try {
    fsStatus.value = await getFileSharingStatus()
  } catch (e) {
    console.error('Failed to load status:', e)
  }
}

async function loadUsers(silent = false): Promise<boolean> {
  if (!silent) usersLoading.value = true
  try {
    users.value = await getFileSharingUsers()
    return true
  } catch (e) {
    console.error('Failed to load users:', e)
    return false
  } finally {
    if (!silent) usersLoading.value = false
  }
}

// 单次展示的日志条数，更早的记录由后端按最新截断
const LOG_PAGE_LIMIT = 500

function todayDate(): string {
  const now = new Date()
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`
}

const logActionOptions = computed(() => {
  const seen = new Set(logs.value.map((log) => log.action))
  return Array.from(seen).sort()
})

const filteredLogs = computed(() => {
  const keyword = logKeyword.value.trim().toLowerCase()
  return logs.value.filter((log) => {
    if (logActionFilter.value && log.action !== logActionFilter.value) return false
    if (!keyword) return true
    return (
      log.user.toLowerCase().includes(keyword) ||
      log.detail.toLowerCase().includes(keyword) ||
      logActionLabel(log.action).toLowerCase().includes(keyword)
    )
  })
})

const logSummary = computed(() => {
  const base = `显示 ${filteredLogs.value.length} / ${logs.value.length} 条`
  if (logTailOnly.value) return `${base} · 文件较大，仅扫描尾部`
  if (logParsed.value > logs.value.length) return `${base} · 该日共 ${logParsed.value} 条`
  return base
})

async function loadLogs(): Promise<boolean> {
  logsLoading.value = true
  try {
    const page = await readFileSharingLogDate(selectedLogDate.value, LOG_PAGE_LIMIT)
    logs.value = page.entries
    logParsed.value = page.parsed
    logTailOnly.value = page.tail_only
    return true
  } catch (e) {
    console.error('Failed to load logs:', e)
    return false
  } finally {
    logsLoading.value = false
  }
}

/** 拉取可选日期；今天的文件可能还不存在，也要作为选项出现 */
async function loadLogDates(): Promise<boolean> {
  try {
    const dates = await getFileSharingLogDates()
    const today = todayDate()
    if (!dates.some((item) => item.date === today)) {
      dates.unshift({ date: today, size_bytes: 0, current: true, exists: false })
    }
    logDates.value = dates
    if (!dates.some((item) => item.date === selectedLogDate.value)) {
      selectedLogDate.value = today
    }
    return true
  } catch (e) {
    console.error('Failed to load log dates:', e)
    return false
  }
}

/** 日志页的数据源：先确定日期，再读该日期的记录 */
async function loadLogView(): Promise<boolean> {
  return (await loadLogDates()) && (await loadLogs())
}

async function refreshLogs() {
  if (await loadLogView()) {
    notify('日志已刷新')
  } else {
    notify('刷新日志失败', 'error')
  }
}

async function onLogDateChange() {
  if (!(await loadLogs())) {
    notify('读取该日期日志失败', 'error')
  }
}

/** 仅真实存在的日志文件可删除；「今天」的占位项不参与 */
const existingLogFiles = computed(() => logDates.value.filter((item) => item.exists))

const selectedLogFileSize = computed(() =>
  existingLogFiles.value
    .filter((item) => selectedLogFiles.value.includes(item.date))
    .reduce((sum, item) => sum + item.size_bytes, 0),
)

function formatSize(bytes: number): string {
  if (bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / Math.pow(1024, index)
  return `${value >= 100 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`
}

function openLogFilesModal() {
  selectedLogFiles.value = []
  showLogFilesModal.value = true
  // 打开时静默刷新一次，保证文件大小与实际一致
  void loadLogDates()
}

function onLogFileSelection(rows: LogDateInfo[]) {
  selectedLogFiles.value = rows.map((row) => row.date)
}

/** 删除日志文件；单个文件失败不阻断其余文件，结果逐项提示 */
async function deleteLogFiles(dates: string[], all: boolean) {
  if (dates.length === 0) return
  const totalSize = dates.reduce((sum, date) => {
    const item = existingLogFiles.value.find((file) => file.date === date)
    return sum + (item?.size_bytes ?? 0)
  }, 0)

  try {
    await ElMessageBox.confirm(
      all
        ? `确定要删除全部 ${dates.length} 个日志文件（共 ${formatSize(totalSize)}）吗？删除后无法恢复。`
        : `确定要删除选中的 ${dates.length} 个日志文件（共 ${formatSize(totalSize)}）吗？删除后无法恢复。`,
      all ? '删除全部日志' : '删除日志文件',
      { type: 'warning', confirmButtonText: '删除', cancelButtonText: '取消' },
    )
  } catch {
    return
  }

  logFilesDeleting.value = true
  try {
    const result = await deleteFileSharingLogDates(dates)
    if (result.failed.length > 0) {
      const detail = result.failed.map((item) => `${item.date}：${item.reason}`).join('；')
      notify(`已删除 ${result.deleted.length} 个日志文件，${result.failed.length} 个失败（${detail}）`, 'error')
    } else {
      notify(`已删除 ${result.deleted.length} 个日志文件`)
    }
    selectedLogFiles.value = []
    showLogFilesModal.value = false
    await loadLogView()
  } catch (e) {
    console.error('Failed to delete log files:', e)
    notify('删除日志文件失败: ' + e, 'error')
  } finally {
    logFilesDeleting.value = false
  }
}

function deleteSelectedLogFiles() {
  void deleteLogFiles([...selectedLogFiles.value], false)
}

function deleteAllLogFiles() {
  void deleteLogFiles(existingLogFiles.value.map((item) => item.date), true)
}

async function loadNetworkAddresses(): Promise<boolean> {
  addressesLoading.value = true
  try {
    networkAddresses.value = await getNetworkAddresses()
    await nextTick()
    await generateQRCodes()
    return true
  } catch (e) {
    console.error('Failed to load network addresses:', e)
    return false
  } finally {
    addressesLoading.value = false
  }
}

async function refreshNetworkAddresses() {
  if (await loadNetworkAddresses()) {
    notify('局域网地址已刷新')
  } else {
    notify('刷新局域网地址失败', 'error')
  }
}

async function generateQRCodes() {
  const port = fsStatus.value.port || fsConfig.value.port
  const newQrUrls: Record<string, string> = {}

  for (const iface of networkAddresses.value) {
    if (!iface.connected) continue
    const url = `http://${iface.ip}:${port}`
    try {
      const qrDataUrl = await QRCode.toDataURL(url, {
        width: 160,
        margin: 1,
        color: {
          dark: '#333333',
          light: '#ffffff',
        },
      })
      newQrUrls[iface.ip] = qrDataUrl
    } catch (e) {
      console.error('Failed to generate QR code:', e)
    }
  }

  qrCodeUrls.value = newQrUrls
}

async function saveConfig() {
  if (!isDirty.value) return
  saving.value = true
  try {
    const removed = await setFileSharingConfig({
      port: fsConfig.value.port,
      shared_folders: fsConfig.value.shared_folders,
      token_duration_hours: fsConfig.value.token_duration_hours,
      auto_start: fsConfig.value.auto_start,
      log_retention_days: fsConfig.value.log_retention_days,
    })
    savedSnapshot.value = currentSnapshot()
    await loadStatus()
    notify(removed > 0 ? `配置已保存，已自动清理 ${removed} 个过期日志文件` : '配置已保存')
  } catch (e) {
    console.error('Failed to save config:', e)
    notify('保存配置失败: ' + e, 'error')
  } finally {
    saving.value = false
  }
}

async function toggleServer() {
  const wasRunning = fsStatus.value.running
  serverActionLoading.value = true
  try {
    if (wasRunning) {
      await stopFileSharing()
      networkAddresses.value = []
      qrCodeUrls.value = {}
    } else {
      await startFileSharing()
      await loadNetworkAddresses()
    }
    await loadStatus()
    notify(wasRunning ? '服务已停止' : '服务已启动')
  } catch (e) {
    console.error('Failed to toggle server:', e)
    notify((wasRunning ? '停止服务失败: ' : '启动服务失败: ') + e, 'error')
  } finally {
    serverActionLoading.value = false
  }
}

function addSharedFolder(path: string) {
  if (fsConfig.value.shared_folders.includes(path)) {
    notify('该文件夹已在列表中', 'error')
    return false
  }
  fsConfig.value.shared_folders.push(path)
  notify('已添加共享文件夹')
  return true
}

async function pickFolder() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择共享文件夹',
  })
  if (selected && typeof selected === 'string') {
    addSharedFolder(selected)
  }
}

function addFolder() {
  const path = newFolder.value.trim()
  if (!path) {
    notify('请输入文件夹路径', 'error')
    return
  }
  if (addSharedFolder(path)) {
    newFolder.value = ''
  }
}

function removeFolder(index: number) {
  fsConfig.value.shared_folders.splice(index, 1)
  notify('已移除共享文件夹')
}

async function openFolder(folder: string) {
  try {
    await openSharedFolder(folder)
    notify('已在文件资源管理器中打开')
  } catch (e) {
    console.error('Failed to open folder:', e)
    notify('打开文件夹失败: ' + e, 'error')
  }
}

function showCreateUser() {
  editingUser.value = null
  userForm.value = { username: '', password: '', role: 'user' }
  showUserModal.value = true
}

function showEditUser(user: FileSharingUser) {
  editingUser.value = user
  userForm.value = { username: user.username, password: '', role: user.role }
  showUserModal.value = true
}

async function saveUser() {
  const isEdit = !!editingUser.value
  if (!isEdit && (!userForm.value.username || !userForm.value.password)) {
    notify('用户名和密码不能为空', 'error')
    return
  }
  userSaving.value = true
  try {
    if (editingUser.value) {
      await updateFileSharingUser({
        id: editingUser.value.id,
        username: userForm.value.username || undefined,
        password: userForm.value.password || undefined,
        role: userForm.value.role,
      })
    } else {
      await createFileSharingUser({
        username: userForm.value.username,
        password: userForm.value.password,
        role: userForm.value.role,
      })
    }
    showUserModal.value = false
    await loadUsers()
    notify(isEdit ? '用户已更新' : '用户已创建')
  } catch (e) {
    console.error('Failed to save user:', e)
    notify('保存用户失败: ' + e, 'error')
  } finally {
    userSaving.value = false
  }
}

async function removeUser(user: FileSharingUser) {
  try {
    await ElMessageBox.confirm(`确定要删除用户 "${user.username}" 吗？`, '删除用户', {
      type: 'warning',
      confirmButtonText: '删除',
      cancelButtonText: '取消',
    })
  } catch {
    return
  }
  try {
    await deleteFileSharingUser(user.id)
    await loadUsers()
    notify(`用户 "${user.username}" 已删除`)
  } catch (e) {
    console.error('Failed to delete user:', e)
    notify('删除用户失败: ' + e, 'error')
  }
}

async function forceLogoutUser(user: FileSharingUser) {
  if (!user.online) {
    notify('该用户当前未在线', 'error')
    return
  }
  try {
    await ElMessageBox.confirm(
      `确定要强制下线用户 "${user.username}" 吗？该用户所有已登录会话将立即失效。`,
      '强制下线',
      { type: 'warning', confirmButtonText: '强制下线', cancelButtonText: '取消' },
    )
  } catch {
    return
  }
  try {
    const removed = await forceLogoutFileSharingUser(user.username)
    await loadUsers(true)
    notify(removed > 0 ? `用户 "${user.username}" 已强制下线` : '该用户当前未在线')
  } catch (e) {
    console.error('Failed to force logout user:', e)
    notify('强制下线失败: ' + e, 'error')
  }
}

watch(activeTab, async (tab) => {
  if (tab === 'users' && !(await loadUsers())) notify('加载用户列表失败', 'error')
  if (tab === 'logs' && !(await loadLogView())) notify('加载操作日志失败', 'error')
  if (tab === 'network' && !(await loadNetworkAddresses())) notify('加载局域网地址失败', 'error')
})

type LogActionType = 'success' | 'danger' | 'primary' | 'warning' | 'info'

const logActionTypes: Record<string, LogActionType> = {
  login: 'success',
  logout: 'info',
  force_logout: 'danger',
  login_failed: 'danger',
  upload: 'primary',
  download: 'warning',
  delete: 'danger',
  create_user: 'success',
  update_user: 'warning',
  delete_user: 'danger',
  server_start: 'primary',
  server_stop: 'warning',
}

const logActionLabels: Record<string, string> = {
  login: '登录',
  logout: '退出登录',
  force_logout: '强制下线',
  login_failed: '登录失败',
  upload: '上传',
  download: '下载',
  delete: '删除',
  create_user: '新建用户',
  update_user: '修改用户',
  delete_user: '删除用户',
  server_start: '服务启动',
  server_stop: '服务停止',
}

function logActionType(action: string): LogActionType {
  return logActionTypes[action] ?? 'info'
}

function logActionLabel(action: string): string {
  return logActionLabels[action] ?? action
}

function formatTime(ts: string) {
  try {
    return new Date(ts).toLocaleString('zh-CN')
  } catch {
    return ts
  }
}

function lastLoginText(user: FileSharingUser) {
  return user.last_login_at ? formatTime(user.last_login_at) : '-'
}

function copyUrl(url?: string) {
  const textToCopy = url || fsStatus.value.url
  if (textToCopy) {
    navigator.clipboard.writeText(textToCopy).then(() => {
      notify('复制成功')
    }).catch(() => {
      notify('复制失败', 'error')
    })
  }
}

function openLink(url: string) {
  openUrl(url).catch((e) => {
    notify('打开链接失败: ' + e, 'error')
  })
}

let usersTimer: number | undefined

onMounted(async () => {
  await loadConfig()
  await loadStatus()
  if (fsStatus.value.running) {
    await loadNetworkAddresses()
  }
  usersTimer = window.setInterval(() => {
    if (activeTab.value === 'users') {
      void loadUsers(true)
    }
  }, 15000)
})

onUnmounted(() => {
  window.clearInterval(usersTimer)
})
</script>

<template>
  <div class="fs-view">
    <div class="fs-content">
      <el-tabs v-model="activeTab" class="fs-tabs">
        <el-tab-pane label="服务配置" name="server">
          <div class="panel">
            <section class="section">
              <h3 class="section-title">服务状态</h3>
              <div class="status-row">
                <span class="status-label">当前状态</span>
                <el-tag :type="fsStatus.running ? 'success' : 'danger'" size="small">
                  {{ fsStatus.running ? '运行中' : '已停止' }}
                </el-tag>
              </div>
              <div class="status-actions">
                <el-button :type="fsStatus.running ? 'danger' : 'primary'" :loading="serverActionLoading"
                  @click="toggleServer">
                  {{ fsStatus.running ? '停止服务' : '启动服务' }}
                </el-button>
              </div>
            </section>

            <section class="section">
              <h3 class="section-title">基本设置</h3>
              <div class="setting-row">
                <label class="setting-label">服务端口</label>
                <el-input-number v-model="fsConfig.port" :min="1024" :max="65535" :controls="false"
                  class="num-input" />
              </div>
              <div class="setting-row">
                <label class="setting-label">登录有效时长（小时）</label>
                <el-input-number v-model="fsConfig.token_duration_hours" :min="1" :max="8760" :controls="false"
                  class="num-input" />
              </div>
              <div class="setting-row">
                <label class="setting-label">日志保留天数</label>
                <el-input-number v-model="fsConfig.log_retention_days" :min="0" :max="3650" :controls="false"
                  class="num-input" />
              </div>
              <p class="setting-hint">0 表示永久保留；超期日志会在启动、保存配置和运行期间自动清理。</p>
              <div class="setting-row">
                <label class="setting-label">程序启动时自动开启服务</label>
                <el-switch v-model="fsConfig.auto_start" />
              </div>
            </section>

            <section class="section">
              <h3 class="section-title">共享文件夹</h3>
              <div class="folder-list">
                <div v-if="fsConfig.shared_folders.length === 0" class="empty-hint">
                  暂无共享文件夹，请添加
                </div>
                <div v-for="(folder, i) in fsConfig.shared_folders" :key="i" class="folder-item">
                  <span class="folder-path">{{ folder }}</span>
                  <div class="folder-actions">
                    <el-button size="small" text title="打开文件夹" @click="openFolder(folder)">
                      <el-icon :size="14">
                        <FolderOpened />
                      </el-icon>
                    </el-button>
                    <el-button size="small" text type="danger" title="移除" @click="removeFolder(i)">
                      <el-icon :size="14">
                        <Delete />
                      </el-icon>
                    </el-button>
                  </div>
                </div>
              </div>
              <div class="folder-add">
                <el-input v-model="newFolder" placeholder="输入文件夹路径，如 D:\Shared" @keydown.enter="addFolder" />
                <el-button @click="addFolder">添加</el-button>
                <el-button title="浏览文件夹" @click="pickFolder">
                  <el-icon :size="14">
                    <Folder />
                  </el-icon>
                </el-button>
              </div>
            </section>
          </div>
        </el-tab-pane>

        <el-tab-pane label="局域网地址" name="network">
          <div class="panel">
            <section class="section">
              <div class="network-addresses-header">
                <span class="status-label">局域网访问地址</span>
                <el-button size="small" text :loading="addressesLoading" @click="refreshNetworkAddresses">
                  刷新
                </el-button>
              </div>
              <div v-if="!fsStatus.running" class="empty-hint">
                服务未启动，请先在「服务配置」页启动服务
              </div>
              <div v-else-if="activeAddresses.length === 0" class="empty-hint">
                未检测到已连接的物理网卡，请检查网络连接后点击刷新
              </div>
              <div v-else class="network-addresses-grid">
                <div v-for="iface in activeAddresses" :key="iface.ip"
                  :class="['network-address-card', { recommended: iface.recommended }]">
                  <div class="network-card-header">
                    <div class="network-card-badges">
                      <el-tag v-if="iface.recommended" size="small" type="success">推荐</el-tag>
                      <el-tag size="small">{{ iface.label }}</el-tag>
                      <span class="network-card-name">{{ iface.name }}</span>
                    </div>
                    <div class="network-card-actions">
                      <el-button size="small" text title="打开链接"
                        @click="openLink(`http://${iface.ip}:${fsStatus.port}`)">
                        <el-icon :size="14">
                          <Link />
                        </el-icon>
                      </el-button>
                      <el-button size="small" text title="复制"
                        @click="copyUrl(`http://${iface.ip}:${fsStatus.port}`)">
                        <el-icon :size="14">
                          <CopyDocument />
                        </el-icon>
                      </el-button>
                    </div>
                  </div>
                  <code class="network-url">http://{{ iface.ip }}:{{ fsStatus.port }}</code>
                  <div class="qr-code-container">
                    <img v-if="qrCodeUrls[iface.ip]" :src="qrCodeUrls[iface.ip]" alt="QR Code"
                      class="qr-code-image" />
                  </div>
                </div>
              </div>
            </section>
          </div>
        </el-tab-pane>

        <el-tab-pane label="用户管理" name="users">
          <div class="panel">
            <div class="panel-toolbar">
              <el-button type="primary" @click="showCreateUser">新建用户</el-button>
              <el-button size="small" :loading="usersLoading" @click="loadUsers()">刷新</el-button>
              <span class="text-secondary users-summary">
                共 {{ users.length }} 个账号 · {{ onlineCount }} 个在线
              </span>
            </div>
            <el-table v-loading="usersLoading" :data="users" empty-text="暂无用户" size="small">
              <el-table-column prop="username" label="用户名" min-width="110" />
              <el-table-column label="角色" width="90">
                <template #default="{ row }">
                  <el-tag :type="(row as FileSharingUser).role === 'admin' ? 'primary' : 'info'" size="small"
                    effect="plain">
                    {{ (row as FileSharingUser).role === 'admin' ? '管理员' : '普通用户' }}
                  </el-tag>
                </template>
              </el-table-column>
              <el-table-column label="状态" width="80">
                <template #default="{ row }">
                  <el-tag :type="(row as FileSharingUser).online ? 'success' : 'info'" size="small" effect="plain">
                    {{ (row as FileSharingUser).online ? '在线' : '离线' }}
                  </el-tag>
                </template>
              </el-table-column>
              <el-table-column label="最近登录" min-width="150">
                <template #default="{ row }">
                  <span class="text-secondary">{{ lastLoginText(row as FileSharingUser) }}</span>
                </template>
              </el-table-column>
              <el-table-column label="操作" width="180">
                <template #default="{ row }">
                  <div class="actions-cell">
                    <el-button size="small" text @click="showEditUser(row as FileSharingUser)">编辑</el-button>
                    <el-tooltip :disabled="(row as FileSharingUser).online" content="该用户当前未在线"
                      placement="top">
                      <span>
                        <el-button size="small" text type="warning"
                          :disabled="!(row as FileSharingUser).online"
                          @click="forceLogoutUser(row as FileSharingUser)">强制下线</el-button>
                      </span>
                    </el-tooltip>
                    <el-button size="small" text type="danger"
                      @click="removeUser(row as FileSharingUser)">删除</el-button>
                  </div>
                </template>
              </el-table-column>
            </el-table>
          </div>
        </el-tab-pane>

        <el-tab-pane label="操作日志" name="logs">
          <div class="panel">
            <div class="log-toolbar">
              <el-select
                v-model="selectedLogDate"
                size="small"
                class="log-date-select"
                @change="onLogDateChange"
              >
                <el-option
                  v-for="item in logDates"
                  :key="item.date"
                  :label="item.current ? `${item.date}（今天）` : item.date"
                  :value="item.date"
                />
              </el-select>
              <el-select
                v-model="logActionFilter"
                size="small"
                placeholder="全部操作"
                clearable
                class="log-action-select"
              >
                <el-option
                  v-for="action in logActionOptions"
                  :key="action"
                  :label="logActionLabel(action)"
                  :value="action"
                />
              </el-select>
              <el-input
                v-model="logKeyword"
                size="small"
                :prefix-icon="Search"
                placeholder="搜索用户、内容或操作"
                clearable
                class="log-search"
              />
              <el-button size="small" :loading="logsLoading" @click="refreshLogs">刷新</el-button>
              <el-tooltip :disabled="existingLogFiles.length > 0" content="暂无可删除的日志文件" placement="top">
                <span>
                  <el-button size="small" type="danger" plain :disabled="existingLogFiles.length === 0"
                    @click="openLogFilesModal">删除日志文件…</el-button>
                </span>
              </el-tooltip>
              <span class="text-secondary log-summary">{{ logSummary }}</span>
            </div>
            <div v-if="filteredLogs.length === 0" class="empty-hint">
              {{ logKeyword || logActionFilter ? '没有匹配的日志' : '该日期暂无日志' }}
            </div>
            <div v-for="(log, i) in filteredLogs" :key="i" class="log-item">
              <span class="log-time">{{ formatTime(log.timestamp) }}</span>
              <el-tag :type="logActionType(log.action)" size="small" effect="plain">{{ logActionLabel(log.action) }}</el-tag>
              <strong>{{ log.user }}</strong>
              <span class="log-detail">{{ log.detail }}</span>
            </div>
          </div>
        </el-tab-pane>
      </el-tabs>

      <div v-if="activeTab === 'server'" class="save-bar">
        <span v-if="isDirty" class="dirty-hint">当前配置未保存</span>
        <el-button type="primary" :disabled="!isDirty" :loading="saving" @click="saveConfig">
          保存配置
        </el-button>
      </div>
    </div>

    <el-dialog v-model="showUserModal" :title="editingUser ? '编辑用户' : '新建用户'" width="420px">
      <el-form label-position="top" @submit.prevent>
        <el-form-item label="用户名">
          <el-input v-model="userForm.username" />
        </el-form-item>
        <el-form-item :label="editingUser ? '密码（留空不修改）' : '密码'">
          <el-input v-model="userForm.password" type="password" show-password />
        </el-form-item>
        <el-form-item label="角色">
          <el-select v-model="userForm.role" class="role-select">
            <el-option label="普通用户" value="user" />
            <el-option label="管理员" value="admin" />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="showUserModal = false">取消</el-button>
        <el-button type="primary" :loading="userSaving" @click="saveUser">保存</el-button>
      </template>
    </el-dialog>

    <el-dialog v-model="showLogFilesModal" title="删除日志文件" width="480px" destroy-on-close>
      <p class="log-files-hint">日志按日期存放，删除后无法恢复；「今天」的文件会在产生新记录时重新生成。</p>
      <el-table :data="logDates" row-key="date" size="small" max-height="320" @selection-change="onLogFileSelection">
        <el-table-column type="selection" width="44" :selectable="(row: LogDateInfo) => row.exists" />
        <el-table-column label="日期" min-width="170">
          <template #default="{ row }">
            <span>{{ row.date }}</span>
            <el-tag v-if="row.current" size="small" type="success" effect="plain" class="today-tag">今天</el-tag>
            <el-tag v-else-if="!row.exists" size="small" type="info" effect="plain" class="today-tag">不存在</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="大小" width="100">
          <template #default="{ row }">
            {{ row.exists ? formatSize(row.size_bytes) : '—' }}
          </template>
        </el-table-column>
        <template #empty>
          <div class="empty-hint">暂无可删除的日志文件</div>
        </template>
      </el-table>
      <template #footer>
        <div class="log-files-footer">
          <el-button type="danger" text :disabled="existingLogFiles.length === 0" :loading="logFilesDeleting"
            @click="deleteAllLogFiles">全部删除</el-button>
          <span class="log-files-summary">
            {{ selectedLogFiles.length > 0 ? `已选 ${selectedLogFiles.length} 个，共 ${formatSize(selectedLogFileSize)}` : '请选择要删除的文件' }}
          </span>
          <el-button @click="showLogFilesModal = false">取消</el-button>
          <el-button type="danger" :disabled="selectedLogFiles.length === 0" :loading="logFilesDeleting"
            @click="deleteSelectedLogFiles">删除所选</el-button>
        </div>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.fs-view {
  height: 100%;
  padding: 24px 32px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.fs-content {
  flex: 1;
  min-height: 0;
  width: 100%;
  max-width: 700px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
}

.fs-tabs {
  flex: 1;
  min-height: 0;
}

.fs-tabs :deep(.el-tabs__header) {
  margin: 0 0 16px;
}

.fs-tabs :deep(.el-tabs__content) {
  padding: 0;
  min-height: 0;
  overflow-y: auto;
}

.panel {
  padding-right: 8px;
}

.section {
  margin-bottom: 24px;
  padding-bottom: 16px;
  border-bottom: 1px solid var(--border-color);
}

.section:last-child {
  border-bottom: none;
}

.section-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 12px;
}

.status-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 0;
}

.status-label {
  font-size: 13px;
  color: var(--text-secondary);
}

.status-actions {
  margin-top: 12px;
}

.network-addresses-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
}

.network-addresses-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 16px;
}

.network-address-card {
  background: var(--segment-bg);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 12px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
}

.network-address-card.recommended {
  border-color: var(--el-color-success);
}

.network-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
}

.network-card-badges {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}

.network-card-name {
  font-size: 12px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.network-card-actions {
  display: flex;
  align-items: center;
  gap: 0;
}

.network-card-actions .el-button + .el-button {
  margin-left: 0;
}

.network-url {
  font-size: 12px;
  color: var(--text-primary);
  background: var(--bg-primary);
  padding: 4px 8px;
  border-radius: 4px;
  word-break: break-all;
  text-align: center;
  width: 100%;
}

.qr-code-container {
  display: flex;
  justify-content: center;
  align-items: center;
  padding: 8px;
  background: #fff;
  border-radius: 4px;
}

.qr-code-image {
  width: 140px;
  height: 140px;
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 0;
}

.setting-label {
  font-size: 13px;
  color: var(--text-primary);
}

.num-input {
  width: 130px;
}

.num-input :deep(.el-input__inner) {
  text-align: left;
}

.setting-hint {
  margin: -2px 0 8px;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-secondary);
}

.folder-list {
  margin-bottom: 12px;
}

.folder-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  margin-bottom: 4px;
}

.folder-path {
  font-size: 13px;
  color: var(--text-primary);
  font-family: monospace;
}

.folder-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.folder-actions .el-button + .el-button {
  margin-left: 0;
}

.folder-add {
  display: flex;
  gap: 8px;
}

.folder-add .el-input {
  flex: 1;
}

.folder-add .el-button+.el-button {
  margin-left: 0;
}

.role-select {
  width: 100%;
}

.save-bar {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 16px;
  padding-top: 16px;
  border-top: 1px solid var(--border-color);
}

.save-bar .el-button {
  margin-left: auto;
}

.dirty-hint {
  font-size: 13px;
  color: var(--warning-text);
}

.panel-toolbar {
  margin-bottom: 12px;
}

.users-summary {
  margin-left: 8px;
  font-size: 12px;
}

.text-secondary {
  color: var(--text-secondary);
}

.actions-cell {
  display: flex;
  gap: 6px;
}

.actions-cell .el-button+.el-button {
  margin-left: 0;
}

.log-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  margin-bottom: 12px;
}

.log-date-select {
  width: 140px;
}

.log-action-select {
  width: 108px;
}

.log-search {
  width: 180px;
}

.log-summary {
  font-size: 12px;
}

.empty-hint {
  text-align: center;
  padding: 24px;
  color: var(--text-secondary);
  font-size: 13px;
}

.log-item {
  padding: 8px 12px;
  border-bottom: 1px solid var(--border-color);
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.log-files-hint {
  margin: 0 0 12px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-secondary);
}

.today-tag {
  margin-left: 6px;
}

.log-files-footer {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
}

.log-files-footer .el-button+.el-button {
  margin-left: 0;
}

.log-files-summary {
  flex: 1;
  min-width: 0;
  text-align: left;
  font-size: 12px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.log-time {
  color: var(--text-secondary);
  font-size: 12px;
  flex-shrink: 0;
}

.log-detail {
  color: var(--text-secondary);
}
</style>
