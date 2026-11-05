export interface Envelope<T> {
  success: boolean;
  data: T | null;
  error: string | null;
}

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly data: unknown = null,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export class UploadConflictError extends ApiError {
  constructor(
    message: string,
    readonly reason: "exists" | "busy",
  ) {
    super(message, 409);
    this.name = "UploadConflictError";
  }
}

let unauthorizedHandler: (() => void) | null = null;

export function onUnauthorized(handler: () => void) {
  unauthorizedHandler = handler;
}

interface RequestOptions {
  method?: "GET" | "POST";
  body?: unknown;
  token?: string;
  signal?: AbortSignal;
}

export async function api<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = {};
  if (options.token) headers.Authorization = `Bearer ${options.token}`;
  if (options.body !== undefined) headers["Content-Type"] = "application/json";

  const response = await fetch(path, {
    method: options.method ?? (options.body !== undefined ? "POST" : "GET"),
    headers,
    body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
    signal: options.signal,
  });

  let payload: Envelope<T> | null = null;
  try {
    payload = (await response.json()) as Envelope<T>;
  } catch {
    payload = null;
  }

  if (response.status === 401 && options.token) {
    unauthorizedHandler?.();
    throw new ApiError(payload?.error ?? "登录已过期，请重新登录", 401);
  }

  if (!response.ok || !payload?.success) {
    throw new ApiError(payload?.error ?? `请求失败 (${response.status})`, response.status, payload?.data ?? null);
  }

  return payload.data as T;
}

const CHUNK_SIZE = 4 * 1024 * 1024;

// crypto.randomUUID 需要安全上下文，通过局域网 http 访问时不可用
function randomId(): string {
  const bytes = new Uint8Array(16);
  try {
    crypto.getRandomValues(bytes);
  } catch {
    for (let i = 0; i < bytes.length; i += 1) bytes[i] = Math.floor(Math.random() * 256);
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export interface UploadController {
  done: Promise<void>;
  pause(): void;
  resume(): void;
  cancel(): void;
  isPaused(): boolean;
}

interface StartUploadOptions {
  uploadId: string;
  file: File;
  fileName: string;
  folderIndex: number;
  dir: string;
  token: string;
  onProgress: (loaded: number, total: number) => void;
  onStateChange?: (state: "uploading" | "paused") => void;
}

interface ReserveUploadOptions {
  token: string;
  folderIndex: number;
  dir: string;
  name: string;
}

/** 在服务端占用目标文件名，同名（磁盘已存在 / 正在上传）时抛 UploadConflictError */
export async function reserveUpload(options: ReserveUploadOptions): Promise<string> {
  const uploadId = randomId();
  try {
    const result = await api<{ upload_id: string }>("/api/files/upload/start", {
      token: options.token,
      body: {
        upload_id: uploadId,
        folder_index: options.folderIndex,
        dir: options.dir,
        name: options.name,
      },
    });
    return result.upload_id;
  } catch (error) {
    if (error instanceof ApiError && error.status === 409) {
      const reason = (error.data as { reason?: string } | null)?.reason;
      throw new UploadConflictError(error.message, reason === "busy" ? "busy" : "exists");
    }
    throw error;
  }
}

export function startUpload(options: StartUploadOptions): UploadController {
  const uploadId = options.uploadId;
  const fileName = options.fileName;
  let offset = 0;
  let paused = false;
  let cancelled = false;
  let activeRequest: XMLHttpRequest | null = null;
  let wake: (() => void) | null = null;

  const scopedQuery = () =>
    new URLSearchParams({
      upload_id: uploadId,
      folder_index: String(options.folderIndex),
      dir: options.dir,
    });

  function sendChunk(): Promise<void> {
    return new Promise((resolve, reject) => {
      const start = offset;
      const blob = options.file.slice(start, start + CHUNK_SIZE);
      const query = scopedQuery();
      query.set("offset", String(start));

      const request = new XMLHttpRequest();
      activeRequest = request;
      request.open("POST", `/api/files/upload/chunk?${query.toString()}`);
      request.setRequestHeader("Authorization", `Bearer ${options.token}`);
      request.upload.onprogress = (event) => {
        if (event.lengthComputable) options.onProgress(start + event.loaded, options.file.size);
      };
      request.onload = () => {
        activeRequest = null;
        let payload: Envelope<unknown> | null = null;
        try {
          payload = JSON.parse(request.responseText) as Envelope<unknown>;
        } catch {
          payload = null;
        }
        if (request.status >= 200 && request.status < 300 && payload?.success) {
          offset = start + blob.size;
          options.onProgress(offset, options.file.size);
          resolve();
          return;
        }
        reject(new ApiError(payload?.error ?? `分片上传失败 (${request.status})`, request.status));
      };
      request.onerror = () => {
        activeRequest = null;
        reject(new ApiError("网络错误，请检查连接", 0));
      };
      request.onabort = () => {
        activeRequest = null;
        reject(new DOMException("Aborted", "AbortError"));
      };
      request.send(blob);
    });
  }

  async function run() {
    while (offset < options.file.size) {
      if (cancelled) throw new ApiError("已取消", 0);
      if (paused) {
        await new Promise<void>((resolve) => {
          wake = resolve;
        });
        continue;
      }
      try {
        await sendChunk();
      } catch (error) {
        if ((error as DOMException)?.name === "AbortError") {
          if (cancelled) throw new ApiError("已取消", 0);
          continue;
        }
        throw error;
      }
    }

    if (cancelled) throw new ApiError("已取消", 0);
    await api("/api/files/upload/complete", {
      token: options.token,
      body: {
        upload_id: uploadId,
        folder_index: options.folderIndex,
        dir: options.dir,
        name: fileName,
        size: options.file.size,
      },
    });
    options.onProgress(options.file.size, options.file.size);
  }

  const done = run();

  return {
    done,
    isPaused: () => paused,
    pause() {
      if (paused || cancelled) return;
      paused = true;
      options.onStateChange?.("paused");
      activeRequest?.abort();
    },
    resume() {
      if (!paused || cancelled) return;
      paused = false;
      options.onStateChange?.("uploading");
      wake?.();
      wake = null;
    },
    cancel() {
      if (cancelled) return;
      cancelled = true;
      paused = false;
      activeRequest?.abort();
      wake?.();
      wake = null;
      void done
        .catch(() => undefined)
        .finally(() => {
          void api("/api/files/upload/cancel", {
            token: options.token,
            body: {
              upload_id: uploadId,
              folder_index: options.folderIndex,
              dir: options.dir,
            },
          }).catch(() => undefined);
        });
    },
  };
}

export function fileUrl(kind: "preview" | "download", folderIndex: number, path: string, token: string) {
  const params = new URLSearchParams({
    folder_index: String(folderIndex),
    path,
    token,
  });
  return `/api/files/${kind}?${params.toString()}`;
}

export function triggerDownload(folderIndex: number, path: string, token: string) {
  const anchor = document.createElement("a");
  anchor.href = fileUrl("download", folderIndex, path, token);
  anchor.download = path.split("/").pop() ?? "download";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
}
