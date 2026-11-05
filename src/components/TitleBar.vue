<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue'
import type { Component } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { AutocompleteInstance } from 'element-plus'
import { Check, Expand, Fold, Grid, Moon, Monitor, Search, Setting, Sunny } from '@element-plus/icons-vue'
import { useSettings, type ThemeMode } from '../composables/useSettings'

defineProps<{
  title: string
  sidebarVisible: boolean
}>()

const emit = defineEmits<{
  toggleSidebar: []
  openSettings: []
  navigateToView: [viewId: string]
  navigateToSetting: [settingId: string]
}>()

const { config, setTheme } = useSettings()

const appWindow = getCurrentWindow()
const isMaximized = ref(false)

const searchQuery = ref('')
const searchRef = ref<AutocompleteInstance | null>(null)

type SearchItem = {
  id: string
  label: string
  description: string
  type: 'feature' | 'setting'
  target: string
  keywords: string[]
}

const searchIndex: SearchItem[] = [
  {
    id: 'file-sharing',
    label: '文件分享',
    description: '文件共享服务管理',
    type: 'feature',
    target: 'file-sharing',
    keywords: ['文件', '分享', '共享', 'server', 'file'],
  },
  {
    id: 'settings',
    label: '设置',
    description: '应用设置',
    type: 'feature',
    target: 'settings',
    keywords: ['设置', '配置', 'settings', 'config'],
  },
  {
    id: 'theme',
    label: '主题模式',
    description: '选择界面的显示主题',
    type: 'setting',
    target: 'theme',
    keywords: ['主题', '模式', '浅色', '深色', '自动', 'theme', 'light', 'dark'],
  },
  {
    id: 'close-action',
    label: '关闭按钮行为',
    description: '点击窗口右上角关闭按钮时的操作',
    type: 'setting',
    target: 'close-action',
    keywords: ['关闭', '按钮', '隐藏', '托盘', '退出', 'close', 'hide', 'exit'],
  },
  {
    id: 'data-path',
    label: '数据文件存放路径',
    description: '各功能的数据文件存放根目录',
    type: 'setting',
    target: 'data-path',
    keywords: ['数据', '路径', '目录', '文件夹', 'data', 'path', 'folder'],
  },
]

function fetchSuggestions(query: string, cb: (items: SearchItem[]) => void) {
  const q = query.trim().toLowerCase()
  if (!q) {
    cb([])
    return
  }
  cb(searchIndex.filter(item =>
    `${item.label} ${item.description} ${item.keywords.join(' ')}`.toLowerCase().includes(q)
  ))
}

function selectSearchResult(item: SearchItem) {
  searchQuery.value = ''
  searchRef.value?.close()

  if (item.type === 'feature') {
    emit('navigateToView', item.target)
  } else if (item.type === 'setting') {
    emit('navigateToView', 'settings')
    setTimeout(() => {
      emit('navigateToSetting', item.target)
    }, 100)
  }
}

function clearSearch() {
  searchQuery.value = ''
  searchRef.value?.blur()
}

function focusSearch() {
  searchRef.value?.focus()
}

function handleGlobalKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key === 'f') {
    e.preventDefault()
    focusSearch()
  }
}

const themeOptions: { value: ThemeMode; label: string; icon: Component }[] = [
  { value: 'light', label: '浅色', icon: Sunny },
  { value: 'dark', label: '深色', icon: Moon },
  { value: 'system', label: '跟随系统', icon: Monitor },
]

const currentTheme = computed(
  () => themeOptions.find(o => o.value === config.value.theme) ?? themeOptions[2]
)

function selectTheme(mode: ThemeMode) {
  setTheme(mode)
}

async function updateMaximized() {
  isMaximized.value = await appWindow.isMaximized()
}

let unlisten: (() => void) | null = null

onMounted(async () => {
  await updateMaximized()
  unlisten = await appWindow.onResized(async () => {
    await updateMaximized()
  })
  document.addEventListener('keydown', handleGlobalKeydown)
})

onUnmounted(() => {
  unlisten?.()
  document.removeEventListener('keydown', handleGlobalKeydown)
})

async function minimize() {
  await appWindow.minimize()
}

async function toggleMaximize() {
  if (isMaximized.value) {
    await appWindow.unmaximize()
  } else {
    await appWindow.maximize()
  }
  await updateMaximized()
}

async function closeWindow() {
  await appWindow.close()
}

async function startDrag(e: MouseEvent) {
  if (e.detail === 2) return
  await appWindow.startDragging()
}
</script>

<template>
  <div class="titlebar" @mousedown="startDrag">
    <div class="titlebar-left">
      <span class="title-text">{{ title }}</span>
    </div>

    <div class="titlebar-center" @mousedown.stop>
      <el-autocomplete ref="searchRef" v-model="searchQuery" class="titlebar-search" size="small"
        :fetch-suggestions="fetchSuggestions" value-key="label" placeholder="搜索功能或设置项... (Ctrl+F)"
        :trigger-on-focus="false" highlight-first-item :teleported="false" popper-class="search-popper"
        @select="(item: unknown) => selectSearchResult(item as unknown as SearchItem)" @keydown.esc="clearSearch">
        <template #prefix>
          <el-icon class="search-prefix-icon" :size="14">
            <Search />
          </el-icon>
        </template>
        <template #default="{ item }">
          <div class="search-result-item">
            <el-icon class="search-result-icon" :size="14">
              <component :is="item.type === 'feature' ? Grid : Setting" />
            </el-icon>
            <div class="search-result-info">
              <span class="search-result-label">{{ item.label }}</span>
              <span class="search-result-desc">{{ item.description }}</span>
            </div>
            <span class="search-result-type">{{ item.type === 'feature' ? '功能' : '设置' }}</span>
          </div>
        </template>
      </el-autocomplete>
    </div>

    <div class="titlebar-right" @mousedown.stop>
      <el-button class="toolbar-btn" :class="{ active: sidebarVisible }" text
        :title="sidebarVisible ? '收起侧边栏' : '展开侧边栏'" @click="emit('toggleSidebar')">
        <el-icon :size="14">
          <component :is="sidebarVisible ? Fold : Expand" />
        </el-icon>
      </el-button>

      <el-dropdown trigger="click" placement="bottom-end" :teleported="false" popper-class="theme-popper">
        <el-button class="toolbar-btn" text :title="'主题: ' + currentTheme.label">
          <el-icon :size="14">
            <component :is="currentTheme.icon" />
          </el-icon>
        </el-button>
        <template #dropdown>
          <el-dropdown-menu class="theme-menu">
            <el-dropdown-item v-for="opt in themeOptions" :key="opt.value"
              :class="{ active: config.theme === opt.value }" @click="selectTheme(opt.value)">
              <el-icon :size="14">
                <component :is="opt.icon" />
              </el-icon>
              <span class="theme-menu-label">{{ opt.label }}</span>
              <el-icon v-if="config.theme === opt.value" class="theme-menu-check" :size="12">
                <Check />
              </el-icon>
            </el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>

      <el-button class="toolbar-btn" text title="设置" @click="emit('openSettings')">
        <el-icon :size="14">
          <Setting />
        </el-icon>
      </el-button>

      <div class="toolbar-divider" />

      <div class="win-controls">
        <button class="win-btn" title="最小化" @click="minimize">
          <svg width="10" height="10" viewBox="0 0 10 10">
            <rect x="0" y="4.5" width="10" height="1" fill="currentColor" />
          </svg>
        </button>
        <button class="win-btn" :title="isMaximized ? '还原' : '最大化'" @click="toggleMaximize">
          <svg v-if="!isMaximized" width="10" height="10" viewBox="0 0 10 10">
            <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1" />
          </svg>
          <svg v-else width="10" height="10" viewBox="0 0 10 10">
            <rect x="2.5" y="0.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1" />
            <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" stroke-width="1" />
          </svg>
        </button>
        <button class="win-btn win-btn-close" title="关闭" @click="closeWindow">
          <svg width="10" height="10" viewBox="0 0 10 10">
            <line x1="0" y1="0" x2="10" y2="10" stroke="currentColor" stroke-width="1.2" />
            <line x1="10" y1="0" x2="0" y2="10" stroke="currentColor" stroke-width="1.2" />
          </svg>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.titlebar {
  display: flex;
  align-items: center;
  height: 36px;
  padding: 0 4px 0 12px;
  user-select: none;
  -webkit-user-select: none;
  flex-shrink: 0;
  background: var(--titlebar-bg);
  border-bottom: 1px solid var(--border-color);
  position: relative;
}

.titlebar-left {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  z-index: 1;
}

.title-text {
  font-size: 12px;
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
}

.titlebar-center {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  justify-content: center;
  width: 40%;
  max-width: 400px;
  z-index: 0;
}

.search-prefix-icon {
  opacity: 0.6;
}

.search-result-item {
  display: flex;
  align-items: center;
  width: 100%;
  padding: 7px 8px;
  gap: 10px;
  font-size: 13px;
}

.search-result-icon {
  flex-shrink: 0;
  color: var(--text-secondary);
  opacity: 0.7;
}

.search-result-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.search-result-label {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.search-result-desc {
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.search-result-type {
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--segment-bg);
  padding: 2px 6px;
  border-radius: 3px;
  flex-shrink: 0;
}

.titlebar-right {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  gap: 1px;
  margin-left: auto;
}

.toolbar-btn {
  --el-button-text-color: var(--text-secondary);
  --el-button-hover-text-color: var(--text-primary);
  --el-fill-color-light: var(--win-btn-hover-bg);
  --el-fill-color: var(--win-btn-hover-bg);

  width: 28px;
  height: 28px;
  padding: 0;
  margin: 0;
  border-radius: 4px;
}

.toolbar-btn.active {
  --el-button-text-color: var(--accent);
  --el-button-hover-text-color: var(--accent);

  color: var(--accent);
}

.toolbar-divider {
  width: 1px;
  height: 16px;
  background: var(--border-color);
  margin: 0 4px;
}

.theme-menu-label {
  flex: 1;
}

.theme-menu-check {
  flex-shrink: 0;
  color: var(--accent);
}

.win-controls {
  display: flex;
  align-items: center;
}

.win-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color 0.1s ease;
  padding: 0;
  border-radius: 4px;
}

.win-btn:hover {
  background: var(--win-btn-hover-bg);
  color: var(--text-primary);
}

.win-btn-close:hover {
  background: #e81123;
  color: #fff;
}
</style>

<!-- 搜索框与两个弹层的内部节点由 Element 渲染，不带本组件的 scope 属性，:deep() 选择器匹配不到，只能放在非 scoped 样式里 -->
<style>
.titlebar-search {
  --el-border-radius-base: 4px;

  width: 100%;
}

.titlebar-search .el-input__wrapper {
  padding: 0 8px;
}

.titlebar-search .el-input__inner {
  height: 22px;
  font-size: 12px;
}

.search-popper {
  min-width: 300px;
}

.search-popper .el-autocomplete-suggestion {
  width: 100%;
}

.search-popper .el-autocomplete-suggestion__wrap {
  max-height: 300px;
  padding: 4px;
}

.search-popper .el-autocomplete-suggestion__list li {
  padding: 0;
  line-height: normal;
  border-radius: 4px;
}

.theme-popper {
  --el-dropdown-menuItem-hover-fill: var(--segment-hover-bg);
  --el-dropdown-menuItem-hover-color: var(--text-primary);
}

.theme-menu {
  min-width: 150px;
  padding: 4px;
}

.theme-menu .el-dropdown-menu__item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  line-height: normal;
  font-size: 12px;
  border-radius: 4px;
}

.theme-menu .el-dropdown-menu__item.active {
  color: var(--accent);
}
</style>