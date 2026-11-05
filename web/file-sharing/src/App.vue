<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { ElMessage } from "element-plus";
import { Moon, Sunny, SwitchButton, User } from "@element-plus/icons-vue";
import LoginView from "./views/LoginView.vue";
import FilesView from "./views/FilesView.vue";
import UsersView from "./views/UsersView.vue";
import LogsView from "./views/LogsView.vue";
import { api, onUnauthorized } from "./api";
import type { ServerStatus, Session } from "./types";

const session = ref<Session | null>(null);
const activeTab = ref("files");
const dark = ref(document.documentElement.classList.contains("dark"));
const verifying = ref(false);
let heartbeatTimer: number | undefined;

function restoreSession(): Session | null {
  const token = localStorage.getItem("fs_token");
  const raw = localStorage.getItem("fs_user");
  if (!token || !raw) return null;
  try {
    const user = JSON.parse(raw) as { username: string; role: string };
    return { token, username: user.username, role: user.role };
  } catch {
    return null;
  }
}

function clearSession() {
  localStorage.removeItem("fs_token");
  localStorage.removeItem("fs_user");
  session.value = null;
  activeTab.value = "files";
}

function handleLogin(next: Session) {
  session.value = next;
  localStorage.setItem("fs_token", next.token);
  localStorage.setItem("fs_user", JSON.stringify({ username: next.username, role: next.role }));
}

function logout() {
  const token = session.value?.token;
  if (token) {
    void api("/api/logout", { token, method: "POST" }).catch(() => undefined);
  }
  clearSession();
  ElMessage.success("已退出登录");
}

function toggleTheme() {
  dark.value = !dark.value;
  document.documentElement.classList.toggle("dark", dark.value);
  localStorage.setItem("fs_theme", dark.value ? "dark" : "light");
}

onMounted(async () => {
  onUnauthorized(() => {
    ElMessage.warning("登录已失效，请重新登录");
    clearSession();
  });

  session.value = restoreSession();
  heartbeatTimer = window.setInterval(() => {
    const token = session.value?.token;
    if (!token) return;
    void api("/api/server/status", { token }).catch(() => undefined);
  }, 60000);

  if (!session.value) return;

  verifying.value = true;
  try {
    await api<ServerStatus>("/api/server/status", { token: session.value.token });
  } catch (error) {
    if (error instanceof Error && error.message.includes("登录已失效")) clearSession();
  } finally {
    verifying.value = false;
  }
});

onUnmounted(() => {
  window.clearInterval(heartbeatTimer);
});
</script>

<template>
  <LoginView v-if="!session" @success="handleLogin" />

  <div v-else class="app-shell">
    <header class="app-header">
      <div class="brand">
        <span class="brand-mark">J</span>
        <span>文件分享</span>
      </div>

      <div class="header-actions">
        <el-tooltip :content="dark ? '切换到浅色' : '切换到深色'" placement="bottom">
          <el-button circle size="small" :icon="dark ? Sunny : Moon" @click="toggleTheme" />
        </el-tooltip>
        <div class="user-chip">
          <el-icon><User /></el-icon>
          <span>{{ session.username }}</span>
          <el-tag size="small" :type="session.role === 'admin' ? 'warning' : 'info'" disable-transitions>
            {{ session.role === "admin" ? "管理员" : "普通用户" }}
          </el-tag>
        </div>
        <el-button size="small" :icon="SwitchButton" @click="logout">退出</el-button>
      </div>
    </header>

    <main v-loading="verifying" class="app-body">
      <el-tabs v-model="activeTab" class="app-tabs">
        <el-tab-pane label="文件浏览" name="files">
          <div class="tab-pane-content">
            <FilesView :session="session" />
          </div>
        </el-tab-pane>
        <el-tab-pane v-if="session.role === 'admin'" label="用户管理" name="users" lazy>
          <div class="tab-pane-content">
            <UsersView :session="session" />
          </div>
        </el-tab-pane>
        <el-tab-pane v-if="session.role === 'admin'" label="操作日志" name="logs" lazy>
          <div class="tab-pane-content">
            <LogsView :session="session" />
          </div>
        </el-tab-pane>
      </el-tabs>
    </main>
  </div>
</template>
