<script setup lang="ts">
import { ref, onMounted, watch, nextTick } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { Folder } from '@element-plus/icons-vue'
import { notify } from '../composables/useNotify'
import {
  useSettings,
  type ThemeMode,
  type CloseAction,
  getDataRootPath,
  setDataRootPath,
  openDataFolder,
} from '../composables/useSettings'

const props = defineProps<{
  activeSettingId?: string
}>()

const { config } = useSettings()

const currentDataPath = ref('')
const saving = ref(false)

onMounted(async () => {
  try {
    currentDataPath.value = await getDataRootPath()
  } catch (e) {
    console.error('Failed to load data path:', e)
    notify('读取数据目录失败: ' + e, 'error')
  }
})

watch(() => props.activeSettingId, (newId) => {
  if (newId) {
    nextTick(() => {
      const element = document.getElementById(`setting-${newId}`)
      if (element) {
        element.scrollIntoView({ behavior: 'smooth', block: 'center' })
        element.classList.add('setting-highlight')
        setTimeout(() => {
          element.classList.remove('setting-highlight')
        }, 2000)
      }
    })
  }
})

async function browseDataPath() {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择数据文件存放目录',
  })
  if (!selected || typeof selected !== 'string') return

  saving.value = true
  try {
    await setDataRootPath(selected)
    currentDataPath.value = await getDataRootPath()
    notify('数据目录已切换')
  } catch (e) {
    console.error('Failed to set data path:', e)
    notify('设置失败: ' + e, 'error')
  } finally {
    saving.value = false
  }
}

async function resetDataPath() {
  saving.value = true
  try {
    await setDataRootPath('')
    currentDataPath.value = await getDataRootPath()
    notify('已重置为默认目录')
  } catch (e) {
    console.error('Failed to reset data path:', e)
    notify('重置失败: ' + e, 'error')
  } finally {
    saving.value = false
  }
}

async function handleOpenDataFolder() {
  try {
    await openDataFolder()
    notify('已打开数据目录')
  } catch (e) {
    console.error('Failed to open data folder:', e)
    notify('打开数据目录失败: ' + e, 'error')
  }
}

const themeOptions: { value: ThemeMode; label: string }[] = [
  { value: 'light', label: '浅色' },
  { value: 'dark', label: '深色' },
  { value: 'system', label: '跟随系统' },
]

const closeOptions: { value: CloseAction; label: string; desc: string }[] = [
  { value: 'hide', label: '隐藏到托盘', desc: '点击关闭按钮时最小化到系统托盘' },
  { value: 'exit', label: '退出程序', desc: '点击关闭按钮时直接退出应用' },
]
</script>

<template>
  <div class="settings-view">
    <div class="settings-content">
      <section class="settings-section">
        <h2 class="section-title">外观</h2>
        <div class="setting-row" id="setting-theme">
          <div class="setting-info">
            <span class="setting-label">主题模式</span>
            <span class="setting-desc">选择界面的显示主题</span>
          </div>
          <el-radio-group v-model="config.theme" size="small">
            <el-radio-button v-for="opt in themeOptions" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </el-radio-button>
          </el-radio-group>
        </div>
      </section>

      <section class="settings-section">
        <h2 class="section-title">系统</h2>
        <div class="setting-row setting-row-vertical" id="setting-close-action">
          <div class="setting-info">
            <span class="setting-label">关闭按钮行为</span>
            <span class="setting-desc">点击窗口右上角关闭按钮时的操作</span>
          </div>
          <el-radio-group v-model="config.close_action" class="option-group">
            <el-radio v-for="opt in closeOptions" :key="opt.value" :value="opt.value" border class="option-radio">
              <div class="radio-content">
                <span class="radio-label">{{ opt.label }}</span>
                <span class="radio-desc">{{ opt.desc }}</span>
              </div>
            </el-radio>
          </el-radio-group>
        </div>
      </section>

      <section class="settings-section">
        <h2 class="section-title">数据存储</h2>
        <div class="setting-row setting-row-vertical" id="setting-data-path">
          <div class="setting-info">
            <span class="setting-label">数据文件存放路径</span>
            <span class="setting-desc">各功能的数据文件存放根目录。修改后已有数据将自动迁移到新路径。</span>
            <el-link class="current-path" title="点击打开文件夹" @click="handleOpenDataFolder">
              <el-icon class="path-icon" :size="12">
                <Folder />
              </el-icon>
              <span class="path-text">{{ currentDataPath || '加载中...' }}</span>
            </el-link>
          </div>
          <div class="data-path-row">
            <el-button type="primary" :loading="saving" @click="browseDataPath">
              <el-icon class="btn-icon">
                <Folder />
              </el-icon>
              <span>切换目录</span>
            </el-button>
            <el-button :loading="saving" @click="resetDataPath">重置</el-button>
          </div>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.settings-view {
  height: 100%;
  padding: 24px 32px;
}

.settings-content {
  max-width: 600px;
  margin: 0 auto;
}

.settings-section {
  margin-bottom: 32px;
}

.section-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-color);
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 0;
  gap: 16px;
}

.setting-row-vertical {
  flex-direction: column;
  align-items: flex-start;
}

.setting-row.setting-highlight {
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  border-radius: 6px;
  padding: 12px 8px;
  margin: 0 -8px;
  transition: background 0.3s ease;
}

.setting-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
}

.setting-label {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.setting-desc {
  font-size: 12px;
  color: var(--text-secondary);
}

.current-path {
  align-self: flex-start;
  margin-top: 4px;
  font-size: 12px;
  font-family: monospace;
  gap: 4px;
}

.current-path .path-text {
  white-space: normal;
  word-break: break-all;
  line-height: 1.5;
}

.path-icon {
  flex-shrink: 0;
}

.option-group {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 6px;
  width: 100%;
  margin-top: 8px;
}

.el-radio.option-radio {
  height: auto;
  width: 100%;
  margin-right: 0;
  align-items: flex-start;
  white-space: normal;
  border-radius: 6px;
}

.el-radio.option-radio.is-bordered {
  padding: 10px 12px;
}

.option-radio :deep(.el-radio__input) {
  margin-top: 2px;
}

.option-radio :deep(.el-radio__label) {
  padding-left: 8px;
  font-weight: 400;
  line-height: 1.5;
  white-space: normal;
}

.radio-content {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.radio-label {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.radio-desc {
  font-size: 12px;
  color: var(--text-secondary);
}

.data-path-row {
  display: flex;
  gap: 8px;
  width: 100%;
  margin-top: 8px;
}

.data-path-row .el-button+.el-button {
  margin-left: 0;
}

.btn-icon {
  margin-right: 6px;
}
</style>
