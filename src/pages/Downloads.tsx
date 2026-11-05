import { DownloadTask } from "../components/DownloadTask";
import type { DownloadTask as Task } from "../types";

export function Downloads({ tasks, onCancel }: { tasks: Task[]; onCancel: (id: string) => void }) {
  const active = tasks.filter((task) => ["pending", "downloading", "processing"].includes(task.status));
  const recent = tasks.filter((task) => !active.includes(task));
  return (
    <div className="page narrow-page">
      <header className="page-header"><span className="eyebrow">DOWNLOAD QUEUE</span><h1>下载任务</h1><p>{active.length ? `${active.length} 个任务正在进行` : "港湾现在很安静"}</p></header>
      {tasks.length === 0 ? <div className="empty-state"><span>↓</span><h3>还没有下载任务</h3><p>从「下载」页面解析一个视频吧。</p></div> : <>
        {active.length > 0 && <section className="task-section"><div className="section-heading"><h2>进行中</h2><span>{active.length}</span></div>{active.map((task) => <DownloadTask key={task.id} task={task} onCancel={onCancel} />)}</section>}
        {recent.length > 0 && <section className="task-section"><div className="section-heading"><h2>最近任务</h2></div>{recent.slice(0, 8).map((task) => <DownloadTask key={task.id} task={task} />)}</section>}
      </>}
    </div>
  );
}
