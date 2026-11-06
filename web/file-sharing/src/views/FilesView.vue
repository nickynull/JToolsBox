<script setup lang="ts">
import { computed, markRaw, onMounted, reactive, ref } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import type { UploadRequestOptions } from "element-plus";
import {
  Box,
  Document,
  DocumentCopy,
  Download,
  Folder,
  Headset,
  Picture,
  Refresh,
  Search,
  UploadFilled,
  VideoCamera,
  View,
  Delete,
} from "@element-plus/icons-vue";
import { api, reserveUpload, startUpload, triggerDownload, UploadConflictError } from "../api";
import type { UploadController } from "../api";
import type { FileEntry, ServerStatus, Session } from "../types";
import { extensionOf, formatSize, formatTime, previewKind } from "../utils";
import PreviewDialog from "../components/PreviewDialog.vue";

const props = defineProps<{ session: Session }>();

type TaskStatus = "uploading" | "paused" | "success" | "error" | "canceled";

interface UploadTask {
  id: string;
  name: string;
  location: string;
  size: number;
  loaded: number;
  speed: number;
  status: TaskStatus;
  error?: string;
  controller?: UploadController;
}

const isAdmin = computed(() => props.session.role === "admin");

const folders = ref<string[]>([]);
const folderIndex = ref(0);
const currentPath = ref("");
const files = ref<FileEntry[]>([]);
const loading = ref(false);
const statusLoading = ref(false);
const search = ref("");
const sortBy = ref("name-asc");
const uploadTasks = ref<UploadTask[]>([]);
const previewTarget = ref<FileEntry | null>(null);
const previewVisible = ref(false);
let requestSeq = 0;

const hasFolder = computed(() => folders.value.length > 0);
const breadcrumbs = computed(() => currentPath.value.split("/").filter(Boolean));
const currentLocation = computed(() => {
  const base = folders.value[folderIndex.value] ?? "";
  return currentPath.value ? `${base} / ${currentPath.value}` : base;
});

const filteredFiles = computed(() => {
  const keyword = search.value.trim().toLowerCase();
  if (!keyword) return files.value;
  return files.value.filter((entry) => entry.name.toLowerCase().includes(keyword));
});

const sortedFiles = computed(() => {
  const list = [...filteredFiles.value];
  const dirs = list.filter((e) => e.is_dir);
  const nonDirs = list.filter((e) => !e.is_dir);
  const [field, order] = sortBy.value.split("-") as [string, "asc" | "desc"];
  const cmp = (a: FileEntry, b: FileEntry) => {
    let result = 0;
    switch (field) {
      case "size":
        result = a.size - b.size;
        break;
      case "modified":
        result = new Date(a.modified).getTime() - new Date(b.modified).getTime();
        break;
      case "name":
      default:
        result = a.name.localeCompare(b.name, "zh-CN");
        break;
    }
    return order === "desc" ? -result : result;
  };
  dirs.sort(cmp);
  nonDirs.sort(cmp);
  return [...dirs, ...nonDirs];
});

const IMAGE_EXT = ["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "avif"];
const VIDEO_EXT = ["mp4", "webm", "ogg", "mov", "m4v"];
const AUDIO_EXT = ["mp3", "wav", "flac", "aac", "m4a"];
const ARCHIVE_EXT = ["zip", "rar", "7z", "tar", "gz", "bz2", "xz"];
const CODE_EXT = ["js", "ts", "json", "xml", "html", "css", "md", "rs", "py", "sh", "yml", "yaml", "toml", "ini", "log"];

function iconFor(entry: FileEntry) {
  if (entry.is_dir) return Folder;
  const ext = extensionOf(entry.name);
  if (IMAGE_EXT.includes(ext)) return Picture;
  if (VIDEO_EXT.includes(ext)) return VideoCamera;
  if (AUDIO_EXT.includes(ext)) return Headset;
  if (ARCHIVE_EXT.includes(ext)) return Box;
  if (CODE_EXT.includes(ext)) return DocumentCopy;
  return Document;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function loadStatus() {
  statusLoading.value = true;
  try {
    const status = await api<ServerStatus>("/api/server/status", { token: props.session.token });
    folders.value = status.shared_folders ?? [];
    if (folderIndex.value >= folders.value.length) folderIndex.value = 0;
    if (hasFolder.value) {
      await loadFiles();
    } else {
      files.value = [];
    }
  } catch (error) {
    ElMessage.error(`获取共享目录失败：${messageOf(error)}`);
  } finally {
    statusLoading.value = false;
  }
}

async function loadFiles() {
  if (!hasFolder.value) return;
  const seq = ++requestSeq;
  loading.value = true;
  try {
    const query = new URLSearchParams({
      folder_index: String(folderIndex.value),
      path: currentPath.value,
    });
    const list = await api<FileEntry[]>(`/api/files?${query.toString()}`, {
      token: props.session.token,
    });
    if (seq === requestSeq) files.value = list;
  } catch (error) {
    if (seq === requestSeq) {
      files.value = [];
      ElMessage.error(`读取目录失败：${messageOf(error)}`);
    }
  } finally {
    if (seq === requestSeq) loading.value = false;
  }
}

function navigateTo(path: string) {
  currentPath.value = path;
  search.value = "";
  loadFiles();
}

function switchFolder(value: number) {
  folderIndex.value = value;
  currentPath.value = "";
  search.value = "";
  loadFiles();
}

function refresh() {
  if (!hasFolder.value) return;
  loadFiles();
}

function openEntry(entry: FileEntry) {
  if (entry.is_dir) navigateTo(entry.path);
  else if (previewKind(entry.name) !== "none") openPreview(entry);
}

function openPreview(entry: FileEntry) {
  previewTarget.value = entry;
  previewVisible.value = true;
}

function download(entry: FileEntry) {
  triggerDownload(folderIndex.value, entry.path, props.session.token);
  ElMessage.success(`开始下载 ${entry.name}`);
}

async function remove(entry: FileEntry) {
  if (!isAdmin.value) {
    ElMessage.warning("仅管理员可删除文件");
    return;
  }

  try {
    await ElMessageBox.confirm(
      entry.is_dir
        ? `确定删除文件夹「${entry.name}」及其中全部内容吗？此操作不可恢复。`
        : `确定删除「${entry.name}」吗？此操作不可恢复。`,
      "删除确认",
      { type: "warning", confirmButtonText: "删除", cancelButtonText: "取消" },
    );
  } catch {
    return;
  }

  try {
    await api("/api/files/delete", {
      token: props.session.token,
      body: { folder_index: folderIndex.value, path: entry.path },
    });
    ElMessage.success(`已删除 ${entry.name}`);
    loadFiles();
  } catch (error) {
    ElMessage.error(`删除失败：${messageOf(error)}`);
  }
}

function appendTimestamp(name: string) {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  const stamp =
    `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}` +
    `_${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return `${name}_${stamp}`;
  return `${name.slice(0, dot)}_${stamp}${name.slice(dot)}`;
}

// el-upload 会把 http-request 返回的 Promise 再次接到 onSuccess/onError 上，
// 这里返回 undefined，成功/失败回调统一由上传任务自己触发
function handleUpload(options: UploadRequestOptions) {
  void beginUpload(options);
}

async function beginUpload(options: UploadRequestOptions) {
  const file = options.file;
  const targetFolder = folderIndex.value;
  const targetDir = currentPath.value;
  const base = folders.value[targetFolder] ?? "";
  const location = targetDir ? `${base} / ${targetDir}` : base;
  let name = file.name;
  let uploadId = "";

  try {
    uploadId = await reserveUpload({
      token: props.session.token,
      folderIndex: targetFolder,
      dir: targetDir,
      name,
    });
  } catch (error) {
    if (!(error instanceof UploadConflictError)) {
      ElMessage.error(`无法开始上传：${messageOf(error)}`);
      return;
    }

    const busy = error.reason === "busy";
    try {
      await ElMessageBox.confirm(
        busy
          ? `「${name}」正在上传中。可以改名为带时间戳的文件后继续上传，避免等待。`
          : `目标目录中已存在「${name}」。继续上传会追加时间戳改名，原文件不会被覆盖。`,
        busy ? "文件正在上传" : "文件已存在",
        { type: "warning", confirmButtonText: "改名继续", cancelButtonText: "取消" },
      );
    } catch {
      ElMessage.info(`已取消上传 ${file.name}`);
      return;
    }

    name = appendTimestamp(name);
    try {
      uploadId = await reserveUpload({
        token: props.session.token,
        folderIndex: targetFolder,
        dir: targetDir,
        name,
      });
    } catch (retryError) {
      ElMessage.error(`无法开始上传：${messageOf(retryError)}`);
      return;
    }
  }

  const task = reactive<UploadTask>({
    id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
    name,
    location,
    size: file.size,
    loaded: 0,
    speed: 0,
    status: "uploading",
  });
  uploadTasks.value.push(task);

  let lastTime = Date.now();
  let lastLoaded = 0;

  const controller = startUpload({
    uploadId,
    file,
    fileName: name,
    folderIndex: targetFolder,
    dir: targetDir,
    token: props.session.token,
    onProgress: (loaded, total) => {
      task.loaded = loaded;
      if (total) task.size = total;
      const now = Date.now();
      if (now - lastTime >= 300) {
        task.speed = ((loaded - lastLoaded) * 1000) / (now - lastTime);
        lastTime = now;
        lastLoaded = loaded;
      }
    },
    onStateChange: (state) => {
      task.status = state;
      if (state === "paused") task.speed = 0;
      lastTime = Date.now();
      lastLoaded = task.loaded;
    },
  });
  task.controller = markRaw(controller);

  controller.done
    .then(() => {
      if (task.status === "canceled") return;
      task.status = "success";
      task.loaded = task.size;
      task.speed = 0;
      options.onSuccess({});
      ElMessage.success(`${name} 上传完成`);
      loadFiles();
      window.setTimeout(() => dismissTask(task), 4000);
    })
    .catch((error) => {
      task.speed = 0;
      if (task.status === "canceled") return;
      task.status = "error";
      task.error = messageOf(error);
      options.onError({
        name: "UploadAjaxError",
        message: task.error,
        status: 0,
        method: "POST",
        url: "/api/files/upload/chunk",
      } as Parameters<typeof options.onError>[0]);
      ElMessage.error(`${name} 上传失败：${task.error}`);
    });
}

function pauseTask(task: UploadTask) {
  if (!task.controller || task.status !== "uploading") return;
  task.controller.pause();
}

function resumeTask(task: UploadTask) {
  if (!task.controller || task.status !== "paused") return;
  task.controller.resume();
}

function cancelTask(task: UploadTask) {
  if (!task.controller || task.status === "success" || task.status === "canceled") return;
  task.controller.cancel();
  task.status = "canceled";
  task.speed = 0;
  task.error = undefined;
  ElMessage.info(`已取消上传 ${task.name}`);
}

const STATUS_LABEL: Record<TaskStatus, string> = {
  uploading: "上传中",
  paused: "已暂停",
  success: "已完成",
  error: "上传失败",
  canceled: "已取消",
};

function statusLabel(status: TaskStatus) {
  return STATUS_LABEL[status];
}

function progressStatus(task: UploadTask): "" | "success" | "exception" | "warning" {
  if (task.status === "success") return "success";
  if (task.status === "error") return "exception";
  if (task.status === "paused") return "warning";
  if (task.status === "canceled") return "warning";
  return "";
}

function dismissTask(task: UploadTask) {
  uploadTasks.value = uploadTasks.value.filter((item) => item.id !== task.id);
}

function percentOf(task: UploadTask) {
  if (!task.size) return 0;
  return Math.min(100, Math.round((task.loaded / task.size) * 100));
}

onMounted(loadStatus);
</script>

<template>
  <div class="files-view">
    <el-alert
      v-if="!statusLoading && !hasFolder"
      title="管理员尚未配置共享文件夹"
      description="请先在客户端「文件分享 - 服务配置」中添加共享目录，然后点击刷新。"
      type="warning"
      show-icon
      :closable="false"
      class="folder-alert"
    />

    <el-upload
      v-if="hasFolder"
      drag
      multiple
      :show-file-list="false"
      :http-request="handleUpload"
      class="uploader"
    >
      <el-icon class="el-icon--upload"><UploadFilled /></el-icon>
      <div class="el-upload__text">拖拽文件到此处，或 <em>点击选择文件</em></div>
      <template #tip>
        <div class="el-upload__tip">
          上传到：{{ currentLocation || "根目录" }}；支持大文件分片上传，可暂停、继续或取消
        </div>
      </template>
    </el-upload>

    <div class="toolbar">
      <div class="folder-row">
        <el-select
          :model-value="folderIndex"
          :disabled="!hasFolder"
          :loading="statusLoading"
          class="folder-select"
          placeholder="选择共享目录"
          @change="switchFolder"
        >
          <el-option v-for="(folder, index) in folders" :key="folder" :label="folder" :value="index" />
        </el-select>
        <el-button :icon="Refresh" :loading="loading" :disabled="!hasFolder" @click="refresh">刷新</el-button>
      </div>

      <div class="breadcrumb-row">
        <el-breadcrumb separator="/" class="toolbar-grow">
          <el-breadcrumb-item>
            <a class="crumb" @click="navigateTo('')">根目录</a>
          </el-breadcrumb-item>
          <el-breadcrumb-item v-for="(part, index) in breadcrumbs" :key="index">
            <a
              v-if="index < breadcrumbs.length - 1"
              class="crumb"
              @click="navigateTo(breadcrumbs.slice(0, index + 1).join('/'))"
            >
              {{ part }}
            </a>
            <span v-else class="crumb-current">{{ part }}</span>
          </el-breadcrumb-item>
        </el-breadcrumb>
      </div>

      <div class="toolbar-actions">
        <el-select v-model="sortBy" class="sort-select" placeholder="排序方式">
          <el-option label="按名称升序" value="name-asc" />
          <el-option label="按名称降序" value="name-desc" />
          <el-option label="按大小升序" value="size-asc" />
          <el-option label="按大小降序" value="size-desc" />
          <el-option label="按时间升序" value="modified-asc" />
          <el-option label="按时间降序" value="modified-desc" />
        </el-select>

        <el-input
          v-model="search"
          :prefix-icon="Search"
          placeholder="搜索当前目录"
          clearable
          class="search-input"
        />
      </div>
    </div>

    <div v-if="uploadTasks.length" class="upload-tasks">
      <div v-for="task in uploadTasks" :key="task.id" class="upload-task">
        <div class="upload-task-head">
          <span class="upload-task-name" :title="task.name">{{ task.name }}</span>
          <span class="task-status" :class="`is-${task.status}`">{{ statusLabel(task.status) }}</span>
          <span class="dim mono">
            {{ formatSize(task.loaded) }} / {{ formatSize(task.size) }}
            <template v-if="task.status === 'uploading' && task.speed > 0">
              · {{ formatSize(task.speed) }}/s
            </template>
          </span>
          <div class="upload-task-actions">
            <el-button v-if="task.status === 'uploading'" link size="small" @click="pauseTask(task)">
              暂停
            </el-button>
            <el-button
              v-else-if="task.status === 'paused'"
              link
              size="small"
              type="primary"
              @click="resumeTask(task)"
            >
              继续
            </el-button>
            <el-button
              v-if="task.status === 'uploading' || task.status === 'paused'"
              link
              size="small"
              type="danger"
              @click="cancelTask(task)"
            >
              取消
            </el-button>
            <el-button v-else link size="small" @click="dismissTask(task)">移除</el-button>
          </div>
        </div>
        <div class="upload-task-path" :title="task.location">
          上传到：<span class="mono">{{ task.location || "根目录" }}</span>
        </div>
        <el-progress
          :percentage="percentOf(task)"
          :status="progressStatus(task)"
          :stroke-width="6"
        />
        <div v-if="task.error" class="upload-task-error">{{ task.error }}</div>
      </div>
    </div>

    <div class="file-table-wrap">
      <!-- Desktop table view -->
      <el-table
        v-loading="loading || statusLoading"
        :data="sortedFiles"
        row-key="path"
        size="default"
        class="file-table desktop-table"
        @row-dblclick="openEntry"
      >
        <el-table-column label="名称" prop="name" min-width="260">
          <template #default="{ row }">
            <div class="name-cell">
              <el-icon class="name-icon"><component :is="iconFor(row)" /></el-icon>
              <a v-if="row.is_dir" class="name-link" @click="navigateTo(row.path)">{{ row.name }}</a>
              <span v-else class="name-text">{{ row.name }}</span>
            </div>
          </template>
        </el-table-column>

        <el-table-column label="大小" prop="size" width="120" align="right">
          <template #default="{ row }">
            <span class="dim">{{ row.is_dir ? "-" : formatSize(row.size) }}</span>
          </template>
        </el-table-column>

        <el-table-column label="修改时间" prop="modified" width="180">
          <template #default="{ row }">
            <span class="dim mono">{{ formatTime(row.modified) }}</span>
          </template>
        </el-table-column>

        <el-table-column label="操作" width="210" fixed="right">
          <template #default="{ row }">
            <div class="row-actions">
              <el-button
                v-if="!row.is_dir && previewKind(row.name) !== 'none'"
                link
                type="primary"
                :icon="View"
                @click="openPreview(row)"
              >
                预览
              </el-button>
              <el-button v-if="!row.is_dir" link :icon="Download" @click="download(row)">下载</el-button>
              <el-tooltip
                :disabled="isAdmin"
                content="仅管理员可删除文件"
                placement="top"
              >
                <span class="tip-wrap">
                  <el-button
                    link
                    type="danger"
                    :icon="Delete"
                    :disabled="!isAdmin"
                    @click="remove(row)"
                  >
                    删除
                  </el-button>
                </span>
              </el-tooltip>
            </div>
          </template>
        </el-table-column>

        <template #empty>
          <el-empty
            :description="search ? '没有匹配的文件' : '当前目录为空，拖拽文件即可上传'"
            :image-size="80"
          />
        </template>
      </el-table>

      <!-- Mobile list view -->
      <div v-loading="loading || statusLoading" class="mobile-file-list">
        <div v-if="sortedFiles.length === 0 && !loading && !statusLoading" class="mobile-empty">
          {{ search ? '没有匹配的文件' : '当前目录为空，拖拽文件即可上传' }}
        </div>
        <div
          v-for="entry in sortedFiles"
          :key="entry.path"
          class="mobile-file-item"
          @click="openEntry(entry)"
        >
          <div class="mobile-file-icon">
            <el-icon :size="20"><component :is="iconFor(entry)" /></el-icon>
          </div>
          <div class="mobile-file-info">
            <div class="mobile-file-name">{{ entry.name }}</div>
            <div class="mobile-file-meta">
              <span v-if="!entry.is_dir">{{ formatSize(entry.size) }}</span>
              <span v-if="!entry.is_dir" class="meta-sep">·</span>
              <span>{{ formatTime(entry.modified) }}</span>
            </div>
          </div>
          <div class="mobile-file-actions" @click.stop>
            <el-button
              v-if="!entry.is_dir && previewKind(entry.name) !== 'none'"
              link
              type="primary"
              size="small"
              @click="openPreview(entry)"
            >
              预览
            </el-button>
            <el-button
              v-if="!entry.is_dir"
              link
              size="small"
              @click="download(entry)"
            >
              下载
            </el-button>
            <el-tooltip
              :disabled="isAdmin"
              content="仅管理员可删除文件"
              placement="top"
            >
              <span class="tip-wrap">
                <el-button
                  link
                  type="danger"
                  size="small"
                  :disabled="!isAdmin"
                  @click="remove(entry)"
                >
                  删除
                </el-button>
              </span>
            </el-tooltip>
          </div>
        </div>
      </div>
    </div>

    <PreviewDialog
      v-if="previewTarget"
      v-model="previewVisible"
      :file="previewTarget"
      :folder-index="folderIndex"
      :token="session.token"
      @download="download(previewTarget)"
    />
  </div>
</template>

<style scoped>
.files-view {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.folder-alert {
  margin-bottom: 0;
}

.folder-select {
  width: 260px;
  max-width: 100%;
}

.folder-row {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
}

.search-input {
  width: 220px;
}

.crumb {
  color: var(--fs-text-secondary);
  cursor: pointer;
  text-decoration: none;
}

.crumb:hover {
  color: var(--fs-accent);
}

.crumb-current {
  color: var(--fs-text);
}

.breadcrumb-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 0;
  flex-wrap: wrap;
}

.toolbar-grow {
  flex: 1;
  min-width: 120px;
}

.sort-select {
  width: 140px;
  flex-shrink: 0;
}

.uploader :deep(.el-upload-dragger) {
  padding: 22px 16px;
}

.uploader {
  border-radius: 8px;
}

.uploader :deep(.el-upload-dragger) {
  border-radius: 8px;
}

/* Responsive: mobile */
@media (max-width: 640px) {
  .uploader :deep(.el-upload-dragger) {
    padding: 14px 10px;
  }

  .uploader :deep(.el-icon--upload) {
    font-size: 28px;
    margin-bottom: 6px;
  }

  .uploader :deep(.el-upload__text) {
    font-size: 13px;
  }

  .uploader :deep(.el-upload__tip) {
    font-size: 11px;
    padding: 0 8px;
  }
}

.upload-tasks {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  border: 1px solid var(--fs-border);
  border-radius: 8px;
  background: var(--fs-surface);
}

.upload-task-head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 4px;
}

.upload-task-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}

.upload-task-error {
  margin-top: 4px;
  font-size: 12px;
  color: var(--el-color-danger);
}

.upload-task-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.upload-task-path {
  margin-bottom: 6px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.task-status {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  white-space: nowrap;
}

.task-status.is-paused {
  color: var(--el-color-warning);
}

.task-status.is-success {
  color: var(--el-color-success);
}

.task-status.is-error {
  color: var(--el-color-danger);
}

.task-status.is-canceled {
  color: var(--el-text-color-placeholder);
}

.tip-wrap {
  display: inline-flex;
}

.file-table-wrap {
  border: 1px solid var(--fs-border);
  border-radius: 8px;
  background: var(--fs-surface);
  overflow: hidden;
}

.file-table :deep(.el-table__inner-wrapper::before) {
  display: none;
}

.name-cell {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.name-icon {
  flex-shrink: 0;
  color: var(--fs-accent);
}

.name-link {
  color: var(--fs-text);
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.name-link:hover {
  color: var(--fs-accent);
  text-decoration: underline;
}

.name-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-wrap: wrap;
}

/* Mobile list view */
.mobile-file-list {
  display: none;
}

.mobile-empty {
  text-align: center;
  padding: 32px 16px;
  color: var(--fs-text-secondary);
  font-size: 14px;
}

.mobile-file-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  border-bottom: 1px solid var(--fs-border);
  cursor: pointer;
  transition: background 0.15s;
}

.mobile-file-item:last-child {
  border-bottom: none;
}

.mobile-file-item:active {
  background: var(--fs-bg);
}

.mobile-file-icon {
  flex-shrink: 0;
  color: var(--fs-accent);
  display: flex;
  align-items: center;
  justify-content: center;
}

.mobile-file-info {
  flex: 1;
  min-width: 0;
}

.mobile-file-name {
  font-size: 14px;
  color: var(--fs-text);
  word-break: break-word;
  line-height: 1.4;
}

.mobile-file-meta {
  font-size: 12px;
  color: var(--fs-text-secondary);
  margin-top: 4px;
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
}

.meta-sep {
  color: var(--fs-text-secondary);
  opacity: 0.5;
}

.mobile-file-actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 2px;
}

.mobile-file-actions .el-button {
  font-size: 13px;
  padding: 4px 6px;
}

/* Responsive: mobile */
@media (max-width: 640px) {
  .desktop-table {
    display: none;
  }

  .mobile-file-list {
    display: block;
  }

  .folder-select {
    flex: 1;
    min-width: 0;
  }

  .toolbar {
    gap: 8px;
  }

  .breadcrumb-row {
    width: 100%;
  }

  .toolbar-actions {
    width: 100%;
    gap: 6px;
  }

  .sort-select {
    width: 130px;
  }

  .search-input {
    flex: 1;
    min-width: 0;
  }

  .mobile-file-item {
    padding: 10px 12px;
    gap: 10px;
  }

  .mobile-file-name {
    font-size: 14px;
  }

  .mobile-file-meta {
    font-size: calc(14px * 0.8);
  }

  .mobile-file-actions {
    gap: 0;
  }

  .mobile-file-actions .el-button {
    padding: 4px 4px;
    font-size: 12px;
  }
}

/* Responsive: desktop - hide mobile list */
@media (min-width: 641px) {
  .mobile-file-list {
    display: none;
  }

  .desktop-table {
    display: table;
  }
}
</style>
