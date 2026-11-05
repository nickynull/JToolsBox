<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { ElMessage } from "element-plus";
import { Refresh, Search } from "@element-plus/icons-vue";
import { api } from "../api";
import type { LogEntry, Session } from "../types";
import { formatTime } from "../utils";

const props = defineProps<{ session: Session }>();

const logs = ref<LogEntry[]>([]);
const loading = ref(false);
const keyword = ref("");
const actionFilter = ref("");

const ACTION_LABELS: Record<string, string> = {
  login: "登录",
  logout: "退出登录",
  force_logout: "强制下线",
  login_failed: "登录失败",
  upload: "上传",
  download: "下载",
  delete: "删除",
  create_user: "新建用户",
  update_user: "修改用户",
  delete_user: "删除用户",
  server_start: "服务启动",
  server_stop: "服务停止",
};

const ACTION_TYPES: Record<string, "success" | "info" | "warning" | "danger" | "primary"> = {
  login: "success",
  logout: "info",
  force_logout: "danger",
  login_failed: "danger",
  upload: "primary",
  download: "info",
  delete: "danger",
  create_user: "warning",
  update_user: "warning",
  delete_user: "danger",
  server_start: "success",
  server_stop: "info",
};

const actionOptions = computed(() => {
  const seen = new Set(logs.value.map((log) => log.action));
  return Array.from(seen).sort();
});

const filtered = computed(() => {
  const keywordText = keyword.value.trim().toLowerCase();
  return logs.value.filter((log) => {
    if (actionFilter.value && log.action !== actionFilter.value) return false;
    if (!keywordText) return true;
    return (
      log.user.toLowerCase().includes(keywordText) ||
      log.detail.toLowerCase().includes(keywordText) ||
      actionLabel(log.action).toLowerCase().includes(keywordText)
    );
  });
});

function actionLabel(action: string) {
  return ACTION_LABELS[action] ?? action;
}

function actionType(action: string) {
  return ACTION_TYPES[action] ?? "info";
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function load() {
  loading.value = true;
  try {
    logs.value = await api<LogEntry[]>("/api/logs?limit=200", { token: props.session.token });
  } catch (error) {
    logs.value = [];
    ElMessage.error(`加载日志失败：${messageOf(error)}`);
  } finally {
    loading.value = false;
  }
}

onMounted(load);
</script>

<template>
  <div class="logs-view">
    <div class="toolbar">
      <el-select v-model="actionFilter" placeholder="全部操作" clearable class="action-select">
        <el-option v-for="action in actionOptions" :key="action" :label="actionLabel(action)" :value="action" />
      </el-select>
      <el-input
        v-model="keyword"
        :prefix-icon="Search"
        placeholder="搜索用户、内容或操作"
        clearable
        class="search-input"
      />
      <el-button :icon="Refresh" :loading="loading" @click="load">刷新</el-button>
      <span class="dim toolbar-grow">显示 {{ filtered.length }} / {{ logs.length }} 条（最多 200 条）</span>
    </div>

    <div class="panel table-panel">
      <el-table v-loading="loading" :data="filtered" max-height="calc(100vh - 210px)">
        <el-table-column label="时间" width="180">
          <template #default="{ row }">
            <span class="dim mono">{{ formatTime(row.timestamp) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="120">
          <template #default="{ row }">
            <el-tag :type="actionType(row.action)" size="small" disable-transitions>
              {{ actionLabel(row.action) }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="用户" prop="user" width="160" />
        <el-table-column label="详情" prop="detail" min-width="320">
          <template #default="{ row }">
            <span class="detail">{{ row.detail }}</span>
          </template>
        </el-table-column>

        <template #empty>
          <el-empty :description="keyword || actionFilter ? '没有匹配的日志' : '暂无日志'" :image-size="80" />
        </template>
      </el-table>
    </div>
  </div>
</template>

<style scoped>
.logs-view {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.action-select {
  width: 150px;
}

.search-input {
  width: 240px;
}

.table-panel {
  padding: 0;
  overflow: hidden;
}

.detail {
  word-break: break-all;
}
</style>
