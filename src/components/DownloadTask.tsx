import type { DownloadTask as Task } from "../types";
import { Icon } from "./Icon";

const labels: Record<Task["status"], string> = {
  pending: "等待中", downloading: "下载中", processing: "正在处理", completed: "已完成", existing: "已存在", failed: "失败", cancelled: "已取消",
};

function speed(bytes: number | null) {
  if (!bytes) return null;
  return bytes > 1024 ** 2 ? `${(bytes / 1024 ** 2).toFixed(1)} MB/s` : `${(bytes / 1024).toFixed(0)} KB/s`;
}

export function DownloadTask({ task, onCancel, onReveal }: {
  task: Task;
  onCancel?: (id: string) => void;
  onReveal?: (task: Task) => void;
}) {
  const active = ["pending", "downloading", "processing"].includes(task.status);
  const revealable = ["completed", "existing"].includes(task.status) && Boolean(task.outputPath) && Boolean(onReveal);
  const detail = task.status === "downloading"
    ? [speed(task.speed), task.eta != null ? `剩余 ${task.eta} 秒` : null].filter(Boolean).join(" · ")
    : task.status === "processing" ? "正在合并或转换媒体文件…"
    : task.status === "completed" ? `${task.formatLabel} · 已保存到下载目录`
    : task.status === "existing" ? `${task.formatLabel} · 文件已在下载目录中`
    : task.error || task.formatLabel;
  return (
    <article className="task-card">
      <div className="task-thumb">
        {task.thumbnail ? <img src={task.thumbnail.replace(/^http:/, "https:")} referrerPolicy="no-referrer" alt="" /> : <Icon name={task.kind === "video" ? "video" : "music"} size={23} />}
      </div>
      <div className="task-main">
        <div className="task-title"><strong title={task.title}>{task.title}</strong><span className={`task-status ${task.status}`}>{task.status === "completed" && <Icon name="check" size={13} />}{labels[task.status]}</span></div>
        <p title={detail}>{detail || "准备下载…"}</p>
        {active && <div className="progress-row"><div className="progress"><i style={{ width: `${task.percent ?? (task.status === "processing" ? 100 : 2)}%` }} className={task.status === "processing" ? "indeterminate" : ""} /></div><b>{task.percent == null ? "—" : `${Math.round(task.percent)}%`}</b></div>}
      </div>
      <div className="task-actions">
        {revealable && <button className="task-file-button" onClick={() => onReveal?.(task)}><Icon name="folder" size={15} />打开所在位置</button>}
        {active && onCancel && <button className="text-button danger" onClick={() => onCancel(task.id)}>取消</button>}
      </div>
    </article>
  );
}
