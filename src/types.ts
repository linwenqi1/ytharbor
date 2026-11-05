export type Page = "download" | "tasks" | "history" | "settings";
export type MediaKind = "video" | "audio";
export type TaskStatus = "pending" | "downloading" | "processing" | "completed" | "failed" | "cancelled";

export interface FormatOption {
  id: string;
  label: string;
  height: number | null;
  extension: string;
  filesize: number | null;
  kind: MediaKind;
}

export interface VideoMetadata {
  id: string;
  url: string;
  platform: "YouTube" | "Bilibili";
  title: string;
  author: string;
  duration: number | null;
  thumbnail: string | null;
  formats: FormatOption[];
}

export interface DownloadRequest {
  url: string;
  title: string;
  thumbnail: string | null;
  formatId: string;
  formatLabel: string;
  kind: MediaKind;
  outputDirectory: string;
  audioFormat?: "mp3" | "m4a";
}

export interface DownloadTask {
  id: string;
  url: string;
  title: string;
  thumbnail: string | null;
  formatLabel: string;
  kind: MediaKind;
  status: TaskStatus;
  percent: number | null;
  speed: number | null;
  eta: number | null;
  outputPath: string | null;
  error: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface AppSettings {
  outputDirectory: string;
  defaultQuality: string;
  filenameTemplate: string;
}
