import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppSettings, DownloadRequest, DownloadTask, VideoMetadata } from "../types";

export const parseVideo = (url: string) => invoke<VideoMetadata>("parse_video", { url });
export const startDownload = (request: DownloadRequest) =>
  invoke<DownloadTask>("start_download", { request });
export const cancelDownload = (taskId: string) => invoke<void>("cancel_download", { taskId });
export const getTasks = () => invoke<DownloadTask[]>("get_tasks");
export const getHistory = () => invoke<DownloadTask[]>("get_history");
export const clearHistory = () => invoke<void>("clear_history");
export const getSettings = () => invoke<AppSettings>("get_settings");
export const saveSettings = (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings });

export const onTaskUpdated = (handler: (task: DownloadTask) => void): Promise<UnlistenFn> =>
  listen<DownloadTask>("download://updated", (event) => handler(event.payload));

export function friendlyError(error: unknown): string {
  const message = typeof error === "string" ? error : error instanceof Error ? error.message : "发生未知错误";
  if (message.includes("yt-dlp")) return `${message}。请确认应用已正确打包下载核心。`;
  return message;
}
