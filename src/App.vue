<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import TitleBar from './components/TitleBar.vue'
import Sidebar from './components/Sidebar.vue'
import SettingsView from './views/SettingsView.vue'
import FileSharingView from './views/FileSharingView.vue'
import { notify } from './composables/useNotify'

const currentView = ref('file-sharing')
const sidebarVisible = ref(true)
const activeSettingId = ref('')

const viewTitle = computed(() => {
  const titles: Record<string, string> = {
    settings: '设置',
    'file-sharing': '文件分享',
  }
  return titles[currentView.value] ?? 'JToolsBox'
})

function toggleSidebar() {
  sidebarVisible.value = !sidebarVisible.value
}

function navigateTo(viewId: string) {
  currentView.value = viewId
  activeSettingId.value = ''
}

function openSettings() {
  currentView.value = 'settings'
  activeSettingId.value = ''
}

function handleNavigateToView(viewId: string) {
  currentView.value = viewId
  activeSettingId.value = ''
}

function handleNavigateToSetting(settingId: string) {
  activeSettingId.value = settingId
}

// 托盘菜单启停服务的结果提示：窗口隐藏时消息会留在页面里，打开即见
let unlistenTrayToast: UnlistenFn | null = null

onMounted(async () => {
  unlistenTrayToast = await listen<{ level: string; message: string }>('tray-toast', (event) => {
    notify(event.payload.message, event.payload.level === 'error' ? 'error' : 'success')
  })
})

onUnmounted(() => {
  unlistenTrayToast?.()
})
</script>

<template>
  <div class="app-layout">
    <TitleBar
      :title="viewTitle"
      :sidebar-visible="sidebarVisible"
      @toggle-sidebar="toggleSidebar"
      @open-settings="openSettings"
      @navigate-to-view="handleNavigateToView"
      @navigate-to-setting="handleNavigateToSetting"
    />
    <div class="app-body">
      <Transition name="sidebar">
        <Sidebar
          v-if="sidebarVisible"
          :current-view="currentView"
          @navigate="navigateTo"
        />
      </Transition>
      <main class="app-main">
        <FileSharingView v-if="currentView === 'file-sharing'" />
        <SettingsView v-if="currentView === 'settings'" :active-setting-id="activeSettingId" />
      </main>
    </div>
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.app-body {
  display: flex;
  flex: 1;
  overflow: hidden;
}

.app-main {
  flex: 1;
  overflow: auto;
  background: var(--bg-primary);
}

/* Sidebar transition */
.sidebar-enter-active,
.sidebar-leave-active {
  transition: width 0.2s ease, min-width 0.2s ease, opacity 0.2s ease;
}

.sidebar-enter-from,
.sidebar-leave-to {
  width: 0;
  min-width: 0;
  opacity: 0;
}

.sidebar-enter-to,
.sidebar-leave-from {
  width: 180px;
  min-width: 180px;
  opacity: 1;
}
</style>
