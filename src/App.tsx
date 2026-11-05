import { useEffect, useMemo, useState } from "react";
import { Sidebar } from "./components/Sidebar";
import { useDownloads } from "./hooks/useDownloads";
import { getSettings } from "./lib/tauri";
import { Home } from "./pages/Home";
import { Downloads } from "./pages/Downloads";
import { History } from "./pages/History";
import { Settings } from "./pages/Settings";
import type { AppSettings, Page } from "./types";

const fallback: AppSettings = { outputDirectory: "", defaultQuality: "1080p", filenameTemplate: "%(title)s [%(id)s].%(ext)s" };

export default function App() {
  const [page, setPage] = useState<Page>("download");
  const [settings, setSettings] = useState(fallback);
  const [historyKey, setHistoryKey] = useState(0);
  const { tasks, start, cancel } = useDownloads();
  const activeCount = useMemo(() => tasks.filter((task) => ["pending", "downloading", "processing"].includes(task.status)).length, [tasks]);
  const terminalCount = useMemo(() => tasks.filter((task) => ["completed", "failed", "cancelled"].includes(task.status)).length, [tasks]);
  useEffect(() => { getSettings().then(setSettings).catch(() => undefined); }, []);
  useEffect(() => { if (terminalCount > 0) setHistoryKey((value) => value + 1); }, [terminalCount]);

  return (
    <div className="app-shell">
      <Sidebar page={page} onChange={setPage} activeCount={activeCount} />
      <main>
        {page === "download" && <Home settings={settings} onStart={start} onTaskCreated={() => setPage("tasks")} />}
        {page === "tasks" && <Downloads tasks={tasks} onCancel={cancel} />}
        {page === "history" && <History refreshKey={historyKey} />}
        {page === "settings" && <Settings settings={settings} onSaved={setSettings} />}
      </main>
    </div>
  );
}
