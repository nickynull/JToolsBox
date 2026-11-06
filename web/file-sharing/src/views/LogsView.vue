<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { ElMessage } from "element-plus";
import { Refresh, Search } from "@element-plus/icons-vue";
import { api } from "../api";
import type { LogEntry, LogDateInfo, LogPage, Session } from "../types";
import { formatTime } from "../utils";

const props = defineProps<{ session: Session }>();

// 单次拉取的日志条数，更早的记录由后端按最新截断
const LOG_PAGE_LIMIT = 500;

const logs = ref<LogEntry[]>([]);
const logDates = ref<LogDateInfo[]>([]);
const selectedDate = ref("");
const parsedCount = ref(0);
const tailOnly = ref(false);
const loading = ref(false);
const keyword = ref("");
const actionFilter = ref("");

function todayDate(): string {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

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

const summary = computed(() => {
  const base = `显示 ${filtered.value.length} / ${logs.value.length} 条`;
  if (tailOnly.value) return `${base} · 文件较大，仅扫描尾部`;
  if (parsedCount.value > logs.value.length) return `${base} · 该日共 ${parsedCount.value} 条`;
  return base;
});

/** 可选日期来自日志目录；今天的文件可能还没生成，也要出现在列表里 */
async function loadDates(): Promise<boolean> {
  try {
    const dates = await api<LogDateInfo[]>("/api/logs/dates", { token: props.session.token });
    const today = todayDate();
    if (!dates.some((item) => item.date === today)) {
      dates.unshift({ date: today, size_bytes: 0, current: true, exists: false });
    }
    logDates.value = dates;
    if (!dates.some((item) => item.date === selectedDate.value)) {
      selectedDate.value = today;
    }
    return true;
  } catch (error) {
    ElMessage.error(`读取日志日期失败：${messageOf(error)}`);
    return false;
  }
}

async function loadLogs(): Promise<boolean> {
  if (!selectedDate.value) return false;
  loading.value = true;
  try {
    const query = `date=${encodeURIComponent(selectedDate.value)}&limit=${LOG_PAGE_LIMIT}`;
    const page = await api<LogPage>(`/api/logs/date?${query}`, { token: props.session.token });
    logs.value = page.entries;
    parsedCount.value = page.parsed;
    tailOnly.value = page.tail_only;
    return true;
  } catch (error) {
    logs.value = [];
    parsedCount.value = 0;
    tailOnly.value = false;
    ElMessage.error(`加载日志失败：${messageOf(error)}`);
    return false;
  } finally {
    loading.value = false;
  }
}

async function refresh() {
  if (await loadDates()) await loadLogs();
}

onMounted(refresh);
</script>

<template>
  <div class="logs-view">
    <div class="toolbar">
      <el-select v-model="selectedDate" class="date-select" @change="loadLogs">
        <el-option
          v-for="item in logDates"
          :key="item.date"
          :label="item.current ? `${item.date}（今天）` : item.date"
          :value="item.date"
        />
      </el-select>
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
      <el-button :icon="Refresh" :loading="loading" @click="refresh">刷新</el-button>
      <span class="dim toolbar-grow">{{ summary }}</span>
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
          <el-empty :description="keyword || actionFilter ? '没有匹配的日志' : '该日期暂无日志'" :image-size="80" />
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

.date-select {
  width: 150px;
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
