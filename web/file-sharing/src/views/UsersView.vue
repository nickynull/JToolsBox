<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { Plus, Refresh, SwitchButton } from "@element-plus/icons-vue";
import { api } from "../api";
import type { Session, UserItem } from "../types";
import { formatTime } from "../utils";

const props = defineProps<{ session: Session }>();

const users = ref<UserItem[]>([]);
const loading = ref(false);
const dialogVisible = ref(false);
const saving = ref(false);
const editing = ref<UserItem | null>(null);
const form = reactive({ username: "", password: "", role: "user" });
let refreshTimer: number | undefined;

const dialogTitle = computed(() => (editing.value ? "编辑用户" : "新建用户"));
const passwordLabel = computed(() => (editing.value ? "密码（留空不修改）" : "密码"));
const onlineCount = computed(() => users.value.filter((user) => user.online).length);

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function load(silent = false) {
  if (!silent) loading.value = true;
  try {
    users.value = await api<UserItem[]>("/api/users", { token: props.session.token });
  } catch (error) {
    if (!silent) {
      users.value = [];
      ElMessage.error(`加载用户失败：${messageOf(error)}`);
    }
  } finally {
    if (!silent) loading.value = false;
  }
}

function openCreate() {
  editing.value = null;
  form.username = "";
  form.password = "";
  form.role = "user";
  dialogVisible.value = true;
}

function openEdit(user: UserItem) {
  editing.value = user;
  form.username = user.username;
  form.password = "";
  form.role = user.role;
  dialogVisible.value = true;
}

async function save() {
  const username = form.username.trim();
  if (!username) {
    ElMessage.warning("请输入用户名");
    return;
  }
  if (!editing.value && !form.password) {
    ElMessage.warning("新建用户时必须设置密码");
    return;
  }

  saving.value = true;
  try {
    if (editing.value) {
      await api("/api/users/update", {
        token: props.session.token,
        body: {
          id: editing.value.id,
          username,
          password: form.password || undefined,
          role: form.role,
        },
      });
      ElMessage.success(`已更新用户 ${username}`);
    } else {
      await api("/api/users", {
        token: props.session.token,
        body: { username, password: form.password, role: form.role },
      });
      ElMessage.success(`已创建用户 ${username}`);
    }
    dialogVisible.value = false;
    load();
  } catch (error) {
    ElMessage.error(`保存失败：${messageOf(error)}`);
  } finally {
    saving.value = false;
  }
}

async function remove(user: UserItem) {
  try {
    await ElMessageBox.confirm(`确定删除用户「${user.username}」吗？`, "删除确认", {
      type: "warning",
      confirmButtonText: "删除",
      cancelButtonText: "取消",
    });
  } catch {
    return;
  }

  try {
    await api("/api/users/delete", { token: props.session.token, body: { id: user.id } });
    ElMessage.success(`已删除用户 ${user.username}`);
    load();
  } catch (error) {
    ElMessage.error(`删除失败：${messageOf(error)}`);
  }
}

async function forceLogout(user: UserItem) {
  if (isSelf(user)) {
    ElMessage.warning("不能强制下线当前登录账号");
    return;
  }
  if (!user.online) {
    ElMessage.info("该用户当前未在线");
    return;
  }

  try {
    await ElMessageBox.confirm(
      `确定强制下线「${user.username}」吗？该用户所有已登录会话将立即失效。`,
      "强制下线",
      { type: "warning", confirmButtonText: "强制下线", cancelButtonText: "取消" },
    );
  } catch {
    return;
  }

  try {
    const result = await api<{ removed: number }>("/api/users/kick", {
      token: props.session.token,
      body: { username: user.username },
    });
    if (result.removed > 0) {
      ElMessage.success(`已将 ${user.username} 强制下线`);
    } else {
      ElMessage.info("该用户当前未在线");
    }
    load(true);
  } catch (error) {
    ElMessage.error(`强制下线失败：${messageOf(error)}`);
  }
}

function isSelf(user: UserItem) {
  return user.username === props.session.username;
}

onMounted(() => {
  load();
  refreshTimer = window.setInterval(() => load(true), 15000);
});

onUnmounted(() => {
  window.clearInterval(refreshTimer);
});
</script>

<template>
  <div class="users-view">
    <div class="toolbar">
      <el-button type="primary" :icon="Plus" @click="openCreate">新建用户</el-button>
      <el-button :icon="Refresh" :loading="loading" @click="load()">刷新</el-button>
      <span class="dim toolbar-grow">共 {{ users.length }} 个账号 · {{ onlineCount }} 个在线</span>
    </div>

    <div class="panel table-panel">
      <el-table v-loading="loading" :data="users" row-key="id">
        <el-table-column label="用户名" prop="username" min-width="180">
          <template #default="{ row }">
            <span>{{ row.username }}</span>
            <el-tag v-if="isSelf(row)" size="small" type="success" class="self-tag" disable-transitions>当前登录</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="角色" prop="role" width="140">
          <template #default="{ row }">
            <el-tag :type="row.role === 'admin' ? 'warning' : 'info'" disable-transitions>
              {{ row.role === "admin" ? "管理员" : "普通用户" }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="100">
          <template #default="{ row }">
            <el-tag :type="row.online ? 'success' : 'info'" size="small" disable-transitions>
              {{ row.online ? "在线" : "离线" }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="最近登录" width="180">
          <template #default="{ row }">
            <span class="dim mono">{{ row.last_login_at ? formatTime(row.last_login_at) : "-" }}</span>
          </template>
        </el-table-column>
        <el-table-column label="创建时间" prop="created_at" width="200">
          <template #default="{ row }">
            <span class="dim mono">{{ formatTime(row.created_at) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="220">
          <template #default="{ row }">
            <el-button link type="primary" @click="openEdit(row)">编辑</el-button>
            <el-tooltip
              :disabled="row.online && !isSelf(row)"
              :content="isSelf(row) ? '不能强制下线当前登录账号' : '该用户当前未在线'"
              placement="top"
            >
              <span>
                <el-button
                  link
                  type="warning"
                  :icon="SwitchButton"
                  :disabled="!row.online || isSelf(row)"
                  @click="forceLogout(row)"
                >
                  强制下线
                </el-button>
              </span>
            </el-tooltip>
            <el-tooltip :disabled="!isSelf(row)" content="不能删除当前登录账号" placement="top">
              <span>
                <el-button link type="danger" :disabled="isSelf(row)" @click="remove(row)">删除</el-button>
              </span>
            </el-tooltip>
          </template>
        </el-table-column>

        <template #empty>
          <el-empty description="暂无用户" :image-size="80" />
        </template>
      </el-table>
    </div>

    <el-dialog v-model="dialogVisible" :title="dialogTitle" width="440px" destroy-on-close>
      <el-form label-position="top" @submit.prevent="save">
        <el-form-item label="用户名">
          <el-input v-model="form.username" placeholder="请输入用户名" maxlength="32" show-word-limit />
        </el-form-item>
        <el-form-item :label="passwordLabel">
          <el-input
            v-model="form.password"
            type="password"
            show-password
            :placeholder="editing ? '不修改请留空' : '请输入密码'"
            autocomplete="new-password"
          />
        </el-form-item>
        <el-form-item label="角色">
          <el-select v-model="form.role" class="role-select">
            <el-option label="普通用户" value="user" />
            <el-option label="管理员" value="admin" />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" :loading="saving" @click="save">保存</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.users-view {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.table-panel {
  padding: 0;
  overflow: hidden;
}

.self-tag {
  margin-left: 8px;
}

.role-select {
  width: 100%;
}
</style>
