import { useEffect } from "react";
import { Icon } from "./Icon";

export interface Notice {
  id: number;
  title: string;
  message: string;
  tone?: "success" | "error";
  actionLabel?: string;
  action?: () => void | Promise<void>;
}

function NoticeCard({ notice, onDismiss }: { notice: Notice; onDismiss: (id: number) => void }) {
  useEffect(() => {
    const timer = window.setTimeout(() => onDismiss(notice.id), 7000);
    return () => window.clearTimeout(timer);
  }, [notice.id, onDismiss]);

  const runAction = () => {
    onDismiss(notice.id);
    void notice.action?.();
  };

  return (
    <aside className={`notice-toast ${notice.tone === "error" ? "error" : ""}`} role="status">
      <span className="notice-mark"><Icon name={notice.tone === "error" ? "close" : "check"} size={15} /></span>
      <div className="notice-copy"><strong>{notice.title}</strong><p title={notice.message}>{notice.message}</p></div>
      {notice.action && notice.actionLabel && <button className="notice-action" onClick={runAction}><Icon name="folder" size={14} />{notice.actionLabel}</button>}
      <button className="notice-close" aria-label="关闭提示" onClick={() => onDismiss(notice.id)}><Icon name="close" size={14} /></button>
    </aside>
  );
}

export function NoticeViewport({ notices, onDismiss }: { notices: Notice[]; onDismiss: (id: number) => void }) {
  if (notices.length === 0) return null;
  return <div className="notice-stack" aria-live="polite">{notices.map((notice) => <NoticeCard key={notice.id} notice={notice} onDismiss={onDismiss} />)}</div>;
}
