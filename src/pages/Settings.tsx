import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Icon } from "../components/Icon";
import { friendlyError, saveSettings } from "../lib/tauri";
import type { AppSettings } from "../types";

export function Settings({ settings, onSaved }: { settings: AppSettings; onSaved: (value: AppSettings) => void }) {
  const [form, setForm] = useState(settings);
  const [message, setMessage] = useState("");
  useEffect(() => setForm(settings), [settings]);
  const choose = async () => {
    const value = await open({ directory: true, multiple: false, defaultPath: form.outputDirectory });
    if (typeof value === "string") setForm({ ...form, outputDirectory: value });
  };
  const save = async () => {
    try { const value = await saveSettings(form); onSaved(value); setMessage("设置已保存"); }
    catch (error) { setMessage(friendlyError(error)); }
  };
  return (
    <div className="page narrow-page settings-page">
      <header className="page-header"><span className="eyebrow">PREFERENCES</span><h1>设置</h1><p>为之后的下载设定默认选项</p></header>
      <section className="settings-card">
        <div className="setting-row"><div><label>下载位置</label><p>视频与音频保存到这里</p></div><div className="path-picker"><span title={form.outputDirectory}>{form.outputDirectory || "未选择"}</span><button className="secondary-button" onClick={choose}><Icon name="folder" size={17} />选择</button></div></div>
        <div className="setting-row"><div><label>默认画质</label><p>解析后优先选择，可随时更改</p></div><select value={form.defaultQuality} onChange={(e) => setForm({ ...form, defaultQuality: e.target.value })}><option>1080p</option><option>720p</option><option>480p</option><option>360p</option></select></div>
        <div className="setting-row"><div><label>文件名模板</label><p>支持 %(title)s、%(id)s、%(ext)s</p></div><input className="setting-input" value={form.filenameTemplate} onChange={(e) => setForm({ ...form, filenameTemplate: e.target.value })} /></div>
      </section>
      <section className="settings-card info-card"><div className="setting-row"><div><label>下载核心</label><p>yt-dlp + FFmpeg · 随应用在本地运行</p></div><span className="local-badge"><i />本地模式</span></div><div className="supported"><span>YouTube</span><span>Bilibili</span><small>v0.1.0</small></div></section>
      <div className="save-bar">{message && <span>{message}</span>}<button className="primary-button" onClick={save}>保存设置</button></div>
    </div>
  );
}
