export type PreviewKind = "image" | "video" | "audio" | "pdf" | "text" | "none";

const IMAGE_EXT = ["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "avif"];
const VIDEO_EXT = ["mp4", "webm", "ogg", "mov", "m4v"];
const AUDIO_EXT = ["mp3", "wav", "flac", "aac", "m4a"];
const TEXT_EXT = ["txt", "csv", "json", "xml", "html", "css", "js", "ts", "md", "log", "ini", "yml", "yaml", "toml", "rs", "py", "sh"];

export function extensionOf(name: string): string {
  const index = name.lastIndexOf(".");
  return index < 0 ? "" : name.slice(index + 1).toLowerCase();
}

export function previewKind(name: string): PreviewKind {
  const ext = extensionOf(name);
  if (IMAGE_EXT.includes(ext)) return "image";
  if (VIDEO_EXT.includes(ext)) return "video";
  if (AUDIO_EXT.includes(ext)) return "audio";
  if (ext === "pdf") return "pdf";
  if (TEXT_EXT.includes(ext)) return "text";
  return "none";
}

export function formatSize(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, index);
  return `${value >= 100 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

export function formatTime(value: string): string {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

export function displayPath(path: string): string {
  return `${path.slice(0, 1).toUpperCase()}${path.slice(1)}`;
}
