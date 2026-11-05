<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessageBox } from 'element-plus'
import { Delete, CopyDocument, Folder, FolderOpened } from '@element-plus/icons-vue'
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
  getFileSharingLogs,
  getNetworkAddresses,
  type FileSharingConfig,
  type FileSharingStatus,
  type FileSharingUser,
  type LogEntry,
  type NetworkInterface,
} from '../composables/useSettings'

const activeTab = ref<'server' | 'users' | 'logs'>('server')

const fsConfig = ref<FileSharingConfig>({
  port: 9800,
  shared_folders: [],
  jwt_secret: '',
  token_duration_hours: 24,
  auto_start: false,
})

const fsStatus = ref<FileSharingStatus>({
  running: false,
  port: 0,
  url: '',
  auto_start: false,
})

const users = ref<FileSharingUser[]>([])
const logs = ref<LogEntry[]>([])
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
  })
}

const isDirty = computed(() => savedSnapshot.value !== null && currentSnapshot() !== savedSnapshot.value)

const showUserModal = ref(false)
const editingUser = ref<FileSharingUser | null>(null)
const userForm = ref({ username: '', password: '', role: 'user' })

const networkAddresses = ref<NetworkInterface[]>([])
const qrCodeUrls = ref<Record<string, string>>({})

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

async function loadLogs(): Promise<boolean> {
  logsLoading.value = true
  try {
    logs.value = await getFileSharingLogs(200)
    return true
  } catch (e) {
    console.error('Failed to load logs:', e)
    return false
  } finally {
    logsLoading.value = false
  }
}

async function refreshLogs() {
  if (await loadLogs()) {
    notify('日志已刷新')
  } else {
    notify('刷新日志失败', 'error')
  }
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
      newQrUrls[iface.label] = qrDataUrl
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
    await setFileSharingConfig({
      port: fsConfig.value.port,
      shared_folders: fsConfig.value.shared_folders,
      token_duration_hours: fsConfig.value.token_duration_hours,
      auto_start: fsConfig.value.auto_start,
    })
    savedSnapshot.value = currentSnapshot()
    await loadStatus()
    notify('配置已保存')
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
  if (tab === 'logs' && !(await loadLogs())) notify('加载操作日志失败', 'error')
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

function logActionType(action: string): LogActionType {
  return logActionTypes[action] ?? 'info'
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
              <div v-if="fsStatus.running && networkAddresses.length > 0" class="network-addresses-section">
                <div class="network-addresses-header">
                  <span class="status-label">局域网访问地址</span>
                  <el-button size="small" text :loading="addressesLoading" @click="refreshNetworkAddresses">
                    刷新
                  </el-button>
                </div>
                <div class="network-addresses-grid">
                  <div v-for="iface in networkAddresses" :key="iface.label"
                    :class="['network-address-card', { offline: !iface.connected }]">
                    <div class="network-card-header">
                      <div class="network-card-badges">
                        <el-tag size="small">{{ iface.label }}</el-tag>
                        <el-tag :type="iface.connected ? 'success' : 'info'" size="small" effect="plain">
                          {{ iface.connected ? '已连接' : '未连接' }}
                        </el-tag>
                      </div>
                      <el-button size="small" text title="复制"
                        @click="copyUrl(`http://${iface.ip}:${fsStatus.port}`)">
                        <el-icon :size="14">
                          <CopyDocument />
                        </el-icon>
                      </el-button>
                    </div>
                    <code class="network-url">http://{{ iface.ip }}:{{ fsStatus.port }}</code>
                    <div v-if="iface.connected" class="qr-code-container">
                      <img v-if="qrCodeUrls[iface.label]" :src="qrCodeUrls[iface.label]" alt="QR Code"
                        class="qr-code-image" />
                    </div>
                    <div v-else class="network-offline-hint">网络未连接，该地址无法访问</div>
                  </div>
                </div>
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
            <div class="panel-toolbar">
              <el-button size="small" :loading="logsLoading" @click="refreshLogs">刷新</el-button>
            </div>
            <div v-if="logs.length === 0" class="empty-hint">暂无日志</div>
            <div v-for="(log, i) in logs" :key="i" class="log-item">
              <span class="log-time">{{ formatTime(log.timestamp) }}</span>
              <el-tag :type="logActionType(log.action)" size="small" effect="plain">{{ log.action }}</el-tag>
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

.network-addresses-section {
  padding: 12px 0;
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
}

.network-address-card.offline .network-url {
  color: var(--text-secondary);
}

.network-offline-hint {
  font-size: 12px;
  color: var(--text-secondary);
  padding: 4px 0;
  text-align: center;
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

.log-time {
  color: var(--text-secondary);
  font-size: 12px;
  flex-shrink: 0;
}

.log-detail {
  color: var(--text-secondary);
}
</style>
