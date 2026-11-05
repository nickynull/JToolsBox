<script setup lang="ts">
import { reactive, ref } from "vue";
import { ElMessage } from "element-plus";
import { Lock, User } from "@element-plus/icons-vue";
import { api } from "../api";
import type { Session } from "../types";

const emit = defineEmits<{ success: [session: Session] }>();

const form = reactive({
  username: localStorage.getItem("fs_last_user") ?? "",
  password: "",
});
const loading = ref(false);
const error = ref("");

interface LoginResult {
  token: string;
  username: string;
  role: string;
}

async function submit() {
  if (loading.value) return;
  if (!form.username.trim()) {
    error.value = "请输入用户名";
    return;
  }
  if (!form.password) {
    error.value = "请输入密码";
    return;
  }

  loading.value = true;
  error.value = "";
  try {
    const result = await api<LoginResult>("/api/login", {
      body: { username: form.username.trim(), password: form.password },
    });
    localStorage.setItem("fs_last_user", result.username);
    ElMessage.success(`欢迎回来，${result.username}`);
    emit("success", { token: result.token, username: result.username, role: result.role });
  } catch (e) {
    error.value = e instanceof Error ? e.message : "登录失败";
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="login-page">
    <div class="login-card">
      <h1 class="login-title">
        <span class="brand-mark">J</span>
        <span>文件分享</span>
      </h1>

      <el-form label-position="top" @submit.prevent="submit">
        <el-form-item label="用户名">
          <el-input
            v-model="form.username"
            :prefix-icon="User"
            placeholder="请输入用户名"
            autocomplete="username"
            size="large"
            @keyup.enter="submit"
          />
        </el-form-item>

        <el-form-item label="密码">
          <el-input
            v-model="form.password"
            type="password"
            show-password
            :prefix-icon="Lock"
            placeholder="请输入密码"
            autocomplete="current-password"
            size="large"
            @keyup.enter="submit"
          />
        </el-form-item>

        <el-alert v-if="error" :title="error" type="error" show-icon :closable="false" class="login-alert" />

        <el-button type="primary" size="large" class="login-submit" :loading="loading" @click="submit">
          {{ loading ? "登录中..." : "登录" }}
        </el-button>
      </el-form>

      <p class="login-hint">请使用管理员分配的账号登录，登录状态会保留 24 小时</p>
    </div>
  </div>
</template>

<style scoped>
.login-alert {
  margin-bottom: 14px;
}

.login-submit {
  width: 100%;
}
</style>
