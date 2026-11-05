import { useCallback, useEffect, useState } from "react";
import type { DownloadRequest, DownloadTask } from "../types";
import * as api from "../lib/tauri";

export function useDownloads() {
  const [tasks, setTasks] = useState<DownloadTask[]>([]);

  const upsert = useCallback((next: DownloadTask) => {
    setTasks((current) => {
      const found = current.some((task) => task.id === next.id);
      return found ? current.map((task) => (task.id === next.id ? next : task)) : [next, ...current];
    });
  }, []);

  useEffect(() => {
    api.getTasks().then(setTasks).catch(() => undefined);
    let disposed = false;
    let unlisten: (() => void) | undefined;
    api.onTaskUpdated(upsert).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [upsert]);

  const start = useCallback(async (request: DownloadRequest) => {
    const task = await api.startDownload(request);
    upsert(task);
    return task;
  }, [upsert]);

  const cancel = useCallback(async (taskId: string) => {
    await api.cancelDownload(taskId);
  }, []);

  return { tasks, start, cancel };
}
