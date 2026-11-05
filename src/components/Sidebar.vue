<script setup lang="ts">
import type { Component } from 'vue'
import { Setting, Upload } from '@element-plus/icons-vue'

defineProps<{
  currentView: string
}>()

const emit = defineEmits<{
  navigate: [viewId: string]
}>()

interface NavItem {
  id: string
  name: string
  icon: Component
}

const navItems: NavItem[] = [
  { id: 'file-sharing', name: '文件分享', icon: Upload },
  { id: 'settings', name: '设置', icon: Setting },
]
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-nav">
      <div class="sidebar-section-title">功能</div>
      <el-menu :default-active="currentView" class="sidebar-menu"
        @select="(index: string) => emit('navigate', index)">
        <el-menu-item v-for="item in navItems" :key="item.id" :index="item.id">
          <el-icon class="sidebar-item-icon" :size="16">
            <component :is="item.icon" />
          </el-icon>
          <span class="sidebar-item-label">{{ item.name }}</span>
        </el-menu-item>
      </el-menu>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 180px;
  min-width: 180px;
  height: 100%;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-color);
  overflow-y: auto;
  overflow-x: hidden;
  transition: width 0.2s ease, min-width 0.2s ease, opacity 0.2s ease;
}

.sidebar-nav {
  padding: 8px 6px;
}

.sidebar-section-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  padding: 0 10px 4px;
}

.sidebar-menu {
  --el-menu-bg-color: transparent;
  --el-menu-text-color: var(--text-primary);
  --el-menu-hover-bg-color: var(--segment-hover-bg);
  --el-menu-active-color: var(--accent);
  --el-menu-item-height: 32px;
  --el-menu-base-level-padding: 10px;
  --el-menu-level-padding: 10px;

  background: transparent;
  border-right: none;
}

.sidebar-menu :deep(.el-menu-item) {
  border-radius: 4px;
  font-size: 13px;
  gap: 8px;
  margin-top: 1px;
}

.sidebar-menu :deep(.el-menu-item .el-icon) {
  margin-right: 0;
  opacity: 0.8;
}

.sidebar-menu :deep(.el-menu-item.is-active) {
  background: var(--segment-active-bg);
  font-weight: 500;
}

.sidebar-menu :deep(.el-menu-item.is-active .el-icon) {
  opacity: 1;
}

.sidebar-item-label {
  flex: 1;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
