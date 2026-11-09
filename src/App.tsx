import { useCallback, useEffect, useMemo, useState } from "react";
import { NoticeViewport, type Notice } from "./components/NoticeToast";
import { Sidebar } from "./components/Sidebar";
import { useDownloads } from "./hooks/useDownloads";
import { getSettings, revealDownload } from "./lib/tauri";
import { Home } from "./pages/Home";
import { Downloads } from "./pages/Downloads";
import { History } from "./pages/History";
import { Settings } from "./pages/Settings";
import type { AppSettings, DownloadTask, Page } from "./types";

const fallback: AppSettings = { outputDirectory: "", defaultQuality: "1080p", filenameTemplate: "%(title)s [%(id)s].%(ext)s" };
let nextNoticeId = 0;

export default function App() {
  const [page, setPage] = useState<Page>("download");
  const [settings, setSettings] = useState(fallback);
  const [historyKey, setHistoryKey] = useState(0);
  const [notices, setNotices] = useState<Notice[]>([]);
  const dismissNotice = useCallback((id: number) => setNotices((items) => items.filter((item) => item.id !== id)), []);
  const pushNotice = useCallback((notice: Omit<Notice, "id">) => {
    setNotices((items) => [...items, { ...notice, id: ++nextNoticeId }].slice(-3));
  }, []);
  const revealTask = useCallback(async (task: DownloadTask) => {
    if (!task.outputPath) return;
    try {
      await revealDownload(task.outputPath);
    } catch {
      pushNotice({ title: "无法打开文件位置", message: "文件可能已被移动或删除。", tone: "error" });
    }
  }, [pushNotice]);
  const handleCompleted = useCallback((task: DownloadTask) => {
    pushNotice({ title: "下载完成", message: task.title, actionLabel: "查看文件", action: () => revealTask(task) });
  }, [pushNotice, revealTask]);
  const { tasks, start, cancel } = useDownloads(handleCompleted);
  const activeCount = useMemo(() => tasks.filter((task) => ["pending", "downloading", "processing"].includes(task.status)).length, [tasks]);
  const terminalCount = useMemo(() => tasks.filter((task) => ["completed", "existing", "failed", "cancelled"].includes(task.status)).length, [tasks]);
  useEffect(() => { getSettings().then(setSettings).catch(() => undefined); }, []);
  useEffect(() => { if (terminalCount > 0) setHistoryKey((value) => value + 1); }, [terminalCount]);

  return (
    <div className="app-shell">
      <Sidebar page={page} onChange={setPage} activeCount={activeCount} />
      <main>
        {page === "download" && <Home settings={settings} onStart={start} onTaskCreated={() => setPage("tasks")} />}
        {page === "tasks" && <Downloads tasks={tasks} onCancel={cancel} onReveal={revealTask} />}
        {page === "history" && <History refreshKey={historyKey} onReveal={revealTask} />}
        {page === "settings" && <Settings settings={settings} onSaved={setSettings} />}
      </main>
      <NoticeViewport notices={notices} onDismiss={dismissNotice} />
    </div>
  );
}
