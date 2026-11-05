<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { Download } from "@element-plus/icons-vue";
import { fileUrl } from "../api";
import type { FileEntry } from "../types";
import { formatSize, formatTime, previewKind } from "../utils";

const props = defineProps<{
  modelValue: boolean;
  file: FileEntry;
  folderIndex: number;
  token: string;
}>();

const emit = defineEmits<{
  "update:modelValue": [value: boolean];
  download: [];
}>();

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit("update:modelValue", value),
});

const kind = computed(() => previewKind(props.file.name));
const url = computed(() => fileUrl("preview", props.folderIndex, props.file.path, props.token));
const tooLargeToPreviewText = computed(() => props.file.size > 4 * 1024 * 1024);

const textContent = ref("");
const textLoading = ref(false);
const textError = ref("");
let controller: AbortController | null = null;

async function loadText() {
  controller?.abort();
  textContent.value = "";
  textError.value = "";
  if (kind.value !== "text" || tooLargeToPreviewText.value) return;

  controller = new AbortController();
  textLoading.value = true;
  try {
    const response = await fetch(url.value, {
      headers: { Authorization: `Bearer ${props.token}` },
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`加载失败 (${response.status})`);
    textContent.value = await response.text();
  } catch (error) {
    if ((error as Error).name !== "AbortError") {
      textError.value = error instanceof Error ? error.message : "加载失败";
    }
  } finally {
    textLoading.value = false;
  }
}

watch(
  () => [props.modelValue, props.file.path] as const,
  ([open]) => {
    if (open) loadText();
    else controller?.abort();
  },
);

onBeforeUnmount(() => controller?.abort());
</script>

<template>
  <el-dialog
    v-model="visible"
    :title="file.name"
    width="min(1080px, 94vw)"
    top="5vh"
    destroy-on-close
    class="preview-dialog"
  >
    <div v-loading="textLoading" class="preview-body">
      <img v-if="kind === 'image'" :src="url" :alt="file.name" class="preview-media" />

      <video v-else-if="kind === 'video'" :src="url" controls autoplay class="preview-video" />

      <audio v-else-if="kind === 'audio'" :src="url" controls autoplay class="preview-audio" />

      <iframe v-else-if="kind === 'pdf'" :src="url" class="preview-frame" :title="file.name" />

      <template v-else-if="kind === 'text'">
        <el-alert
          v-if="tooLargeToPreviewText"
          type="info"
          show-icon
          :closable="false"
          title="文件较大，暂不加载全文"
          :description="`当前 ${formatSize(file.size)}，超过 4 MB 的文本文件请下载后查看。`"
        />
        <el-alert v-else-if="textError" type="error" show-icon :closable="false" :title="textError" />
        <pre v-else class="preview-text mono">{{ textContent }}</pre>
      </template>

      <el-empty v-else description="该类型暂不支持在线预览，请下载后查看" :image-size="96" />
    </div>

    <template #footer>
      <div class="preview-footer">
        <span class="dim mono">{{ formatSize(file.size) }} · {{ formatTime(file.modified) }}</span>
        <el-button type="primary" :icon="Download" @click="emit('download')">下载</el-button>
      </div>
    </template>
  </el-dialog>
</template>

<style scoped>
.preview-body {
  min-height: 160px;
}

.preview-media {
  display: block;
  max-width: 100%;
  max-height: 72vh;
  margin: 0 auto;
}

.preview-video {
  display: block;
  width: 100%;
  max-height: 72vh;
  background: #000;
  border-radius: 6px;
}

.preview-audio {
  width: 100%;
  margin: 24px 0;
}

.preview-frame {
  width: 100%;
  height: 72vh;
  border: 0;
  border-radius: 6px;
  background: #fff;
}

.preview-text {
  max-height: 72vh;
  overflow: auto;
  margin: 0;
  padding: 12px;
  border-radius: 6px;
  background: var(--el-fill-color-light);
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 13px;
}

.preview-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
</style>
