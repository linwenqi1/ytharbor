import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type UpdatePhase = "idle" | "checking" | "upToDate" | "available" | "downloading" | "ready" | "installing" | "error";
export type UpdateErrorStage = "check" | "download" | "install";

export interface UpdateStatus {
  phase: UpdatePhase;
  currentVersion: string;
  availableVersion: string | null;
  notes: string | null;
  progress: number | null;
  error: string | null;
  errorStage: UpdateErrorStage | null;
}

export interface UpdaterController {
  status: UpdateStatus;
  checkForUpdates: () => Promise<boolean>;
  downloadUpdate: () => Promise<void>;
  installUpdate: () => Promise<void>;
}

const initialStatus: UpdateStatus = {
  phase: "idle",
  currentVersion: "—",
  availableVersion: null,
  notes: null,
  progress: null,
  error: null,
  errorStage: null,
};

export function useUpdater(): UpdaterController {
  const [status, setStatus] = useState(initialStatus);
  const updateRef = useRef<Update | null>(null);
  const busyRef = useRef(false);
  const mountedRef = useRef(true);

  const runCheck = useCallback(async (interactive: boolean) => {
    if (busyRef.current) return false;
    busyRef.current = true;
    setStatus((current) => ({ ...current, phase: "checking", error: null, errorStage: null, progress: null }));

    try {
      const currentVersion = await getVersion();
      const previous = updateRef.current;
      updateRef.current = null;
      if (previous) await previous.close().catch(() => undefined);

      const update = await check({ timeout: 12_000 });
      if (!mountedRef.current) {
        await update?.close().catch(() => undefined);
        return false;
      }

      if (!update) {
        setStatus({ ...initialStatus, phase: "upToDate", currentVersion });
        return false;
      }

      updateRef.current = update;
      setStatus({
        ...initialStatus,
        phase: "available",
        currentVersion: update.currentVersion || currentVersion,
        availableVersion: update.version,
        notes: update.body?.trim() || null,
      });
      return true;
    } catch {
      if (mountedRef.current) {
        if (interactive) {
          setStatus((current) => ({
            ...current,
            phase: "error",
            error: "无法检查更新，请确认网络连接后重试。",
            errorStage: "check",
          }));
        } else {
          setStatus((current) => ({ ...current, phase: "idle", error: null, errorStage: null }));
        }
      }
      return false;
    } finally {
      busyRef.current = false;
    }
  }, []);

  const checkForUpdates = useCallback(() => runCheck(true), [runCheck]);

  const downloadUpdate = useCallback(async () => {
    const update = updateRef.current;
    if (!update || busyRef.current) return;
    busyRef.current = true;
    let downloaded = 0;
    let total: number | undefined;
    setStatus((current) => ({ ...current, phase: "downloading", progress: 0, error: null, errorStage: null }));

    try {
      await update.download((event) => {
        if (!mountedRef.current) return;
        if (event.event === "Started") {
          total = event.data.contentLength;
          setStatus((current) => ({ ...current, progress: total ? 0 : null }));
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          if (total) setStatus((current) => ({ ...current, progress: Math.min(100, Math.round((downloaded / total!) * 100)) }));
        } else if (event.event === "Finished") {
          setStatus((current) => ({ ...current, progress: 100 }));
        }
      }, { timeout: 120_000 });
      if (mountedRef.current) setStatus((current) => ({ ...current, phase: "ready", progress: 100 }));
    } catch {
      if (mountedRef.current) {
        setStatus((current) => ({
          ...current,
          phase: "error",
          error: "更新下载失败，请稍后重试。",
          errorStage: "download",
        }));
      }
    } finally {
      busyRef.current = false;
    }
  }, []);

  const installUpdate = useCallback(async () => {
    const update = updateRef.current;
    if (!update || busyRef.current) return;
    busyRef.current = true;
    setStatus((current) => ({ ...current, phase: "installing", error: null, errorStage: null }));

    try {
      await update.install({ restartAfterInstall: true });
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

    const timer = window.setTimeout(() => { void runCheck(false); }, 3_000);
    return () => {
      mountedRef.current = false;
      window.clearTimeout(timer);
      const update = updateRef.current;
      updateRef.current = null;
      if (update && !busyRef.current) void update.close().catch(() => undefined);
    };
  }, [runCheck]);

  return { status, checkForUpdates, downloadUpdate, installUpdate };
}
