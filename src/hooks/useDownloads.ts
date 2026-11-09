import { useCallback, useEffect, useRef, useState } from "react";
import type { DownloadRequest, DownloadTask } from "../types";
import * as api from "../lib/tauri";

export function useDownloads(onCompleted?: (task: DownloadTask) => void) {
  const [tasks, setTasks] = useState<DownloadTask[]>([]);
  const statuses = useRef(new Map<string, DownloadTask["status"]>());
  const onCompletedRef = useRef(onCompleted);

  useEffect(() => { onCompletedRef.current = onCompleted; }, [onCompleted]);

  const upsert = useCallback((next: DownloadTask) => {
    setTasks((current) => {
      const found = current.some((task) => task.id === next.id);
      return found ? current.map((task) => (task.id === next.id ? next : task)) : [next, ...current];
    });
  }, []);

  const handleUpdate = useCallback((next: DownloadTask) => {
    const previous = statuses.current.get(next.id);
    statuses.current.set(next.id, next.status);
    upsert(next);
    if (next.status === "completed" && previous != null && previous !== "completed") {
      onCompletedRef.current?.(next);
    }
  }, [upsert]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    api.getTasks().then((items) => {
      if (disposed) return;
      statuses.current = new Map(items.map((task) => [task.id, task.status]));
      setTasks(items);
    }).catch(() => undefined);
    api.onTaskUpdated(handleUpdate).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [handleUpdate]);

  const start = useCallback(async (request: DownloadRequest) => {
    const task = await api.startDownload(request);
    if (!statuses.current.has(task.id)) statuses.current.set(task.id, task.status);
    upsert(task);
    return task;
  }, [upsert]);

  const cancel = useCallback(async (taskId: string) => {
    await api.cancelDownload(taskId);
  }, []);

  return { tasks, start, cancel };
}
