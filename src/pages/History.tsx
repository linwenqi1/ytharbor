import { useEffect, useState } from "react";
import { DownloadTask } from "../components/DownloadTask";
import { Icon } from "../components/Icon";
import * as api from "../lib/tauri";
import type { DownloadTask as Task } from "../types";

export function History({ refreshKey }: { refreshKey: number }) {
  const [items, setItems] = useState<Task[]>([]);
  const [loading, setLoading] = useState(true);
  useEffect(() => { setLoading(true); api.getHistory().then(setItems).finally(() => setLoading(false)); }, [refreshKey]);
  const clear = async () => { await api.clearHistory(); setItems([]); };
  return (
    <div className="page narrow-page">
      <header className="page-header split-header"><div><span className="eyebrow">LOCAL HISTORY</span><h1>下载历史</h1><p>仅保存在这台电脑上</p></div>{items.length > 0 && <button className="secondary-button" onClick={clear}><Icon name="trash" size={17} />清空记录</button>}</header>
      {!loading && items.length === 0 ? <div className="empty-state"><span>◷</span><h3>没有历史记录</h3><p>完成或失败的任务会出现在这里。</p></div> : <section className="task-section">{items.map((task) => <DownloadTask key={task.id} task={task} />)}</section>}
    </div>
  );
}
