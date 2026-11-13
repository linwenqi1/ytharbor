import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { Channel, invoke } from "@tauri-apps/api/core";

export type UpdatePhase = "idle" | "checking" | "upToDate" | "available" | "downloading" | "ready" | "installing" | "error";
export type UpdateErrorStage = "download" | "install";

export interface UpdateStatus {
  phase: UpdatePhase;
  currentVersion: string;
  availableVersion: string | null;
  notes: string | null;
  progress: number | null;
  reconnecting: boolean;
  error: string | null;
  errorStage: UpdateErrorStage | null;
}

export interface UpdaterController {
  status: UpdateStatus;
  downloadUpdate: () => Promise<void>;
  installUpdate: () => Promise<void>;
}

const initialStatus: UpdateStatus = {
  phase: "idle",
  currentVersion: "—",
  availableVersion: null,
  notes: null,
  progress: null,
  reconnecting: false,
  error: null,
  errorStage: null,
};

export function useUpdater(): UpdaterController {
  const [status, setStatus] = useState(initialStatus);
  const hasUpdateRef = useRef(false);
  const busyRef = useRef(false);
  const mountedRef = useRef(true);

  const runCheck = useCallback(async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    setStatus((current) => ({ ...current, phase: "checking", error: null, errorStage: null, progress: null, reconnecting: false }));

    try {
      const currentVersion = await getVersion();
      const update = await invoke<{ currentVersion: string; version: string; body?: string } | null>("check_update");
      if (!mountedRef.current) return;

      if (!update) {
        hasUpdateRef.current = false;
        setStatus({ ...initialStatus, phase: "upToDate", currentVersion });
        return;
      }

      hasUpdateRef.current = true;
      setStatus({
        ...initialStatus,
        phase: "available",
        currentVersion: update.currentVersion || currentVersion,
        availableVersion: update.version,
        notes: update.body?.trim() || null,
      });
    } catch {
      if (mountedRef.current) {
        setStatus((current) => ({ ...current, phase: "idle", error: null, errorStage: null }));
      }
    } finally {
      busyRef.current = false;
    }
  }, []);

  const downloadUpdate = useCallback(async () => {
    if (!hasUpdateRef.current || busyRef.current) return;
    busyRef.current = true;
    setStatus((current) => ({ ...current, phase: "downloading", error: null, errorStage: null, reconnecting: false }));

    try {
      const onEvent = new Channel<
        | { event: "progress"; data: { downloaded: number; total: number | null } }
        | { event: "reconnecting"; data: { attempt: number; delaySeconds: number } }
        | { event: "finished" }
      >();
      onEvent.onmessage = (event) => {
        if (!mountedRef.current) return;
        if (event.event === "progress") {
          const { downloaded, total } = event.data;
          setStatus((current) => ({ ...current, reconnecting: false, progress: total ? Math.min(100, Math.round((downloaded / total) * 100)) : null }));
        } else if (event.event === "reconnecting") {
          setStatus((current) => ({ ...current, reconnecting: true }));
        } else if (event.event === "finished") {
          setStatus((current) => ({ ...current, reconnecting: false, progress: 100 }));
        }
      };
      await invoke("download_update", { onEvent });
      if (mountedRef.current) setStatus((current) => ({ ...current, phase: "ready", reconnecting: false, progress: 100 }));
    } catch (error) {
      if (mountedRef.current) {
        setStatus((current) => ({
          ...current,
          phase: "error",
          reconnecting: false,
          error: String(error) || "更新下载失败，请稍后重试。",
          errorStage: "download",
        }));
      }
    } finally {
      busyRef.current = false;
    }
  }, []);

  const installUpdate = useCallback(async () => {
    if (!hasUpdateRef.current || busyRef.current) return;
    busyRef.current = true;
    setStatus((current) => ({ ...current, phase: "installing", error: null, errorStage: null }));

    try {
      await invoke("install_update");
    } catch {
      busyRef.current = false;
      if (mountedRef.current) {
        setStatus((current) => ({
          ...current,
          phase: "error",
          error: "无法启动更新安装，请稍后重试。",
          errorStage: "install",
        }));
      }
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    void getVersion().then((version) => {
      if (mountedRef.current) setStatus((current) => ({ ...current, currentVersion: version }));
    }).catch(() => undefined);

    const timer = window.setTimeout(() => { void runCheck(); }, 3_000);
    return () => {
      mountedRef.current = false;
      window.clearTimeout(timer);
    };
  }, [runCheck]);

  return { status, downloadUpdate, installUpdate };
}
