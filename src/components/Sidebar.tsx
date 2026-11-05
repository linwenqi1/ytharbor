import type { Page } from "../types";
import { Icon } from "./Icon";

const items: { id: Page; label: string; icon: Parameters<typeof Icon>[0]["name"] }[] = [
  { id: "download", label: "下载", icon: "download" },
  { id: "tasks", label: "任务", icon: "queue" },
  { id: "history", label: "历史", icon: "history" },
  { id: "settings", label: "设置", icon: "settings" },
];

export function Sidebar({ page, onChange, activeCount }: { page: Page; onChange: (page: Page) => void; activeCount: number }) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark"><Icon name="anchor" size={23} /></span>
        <span><strong>YtHarbor</strong><small>本地视频港湾</small></span>
      </div>
      <nav>
        {items.map((item) => (
          <button key={item.id} className={page === item.id ? "nav-item active" : "nav-item"} onClick={() => onChange(item.id)}>
            <Icon name={item.icon} size={19} />
            <span>{item.label}</span>
            {item.id === "tasks" && activeCount > 0 && <em>{activeCount}</em>}
          </button>
        ))}
      </nav>
      <div className="sidebar-foot"><span className="status-dot" />本地运行 · 隐私优先</div>
    </aside>
  );
}
