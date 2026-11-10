import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Icon } from "../components/Icon";
import type { UpdaterController } from "../hooks/useUpdater";
import { friendlyError, saveSettings } from "../lib/tauri";
import type { AppSettings } from "../types";

const fixedFilenameTemplate = "%(title)s [%(id)s] [%(quality)s].%(ext)s";
const withFixedFilenameTemplate = (settings: AppSettings): AppSettings => ({ ...settings, filenameTemplate: fixedFilenameTemplate });

interface SettingsProps {
  settings: AppSettings;
  onSaved: (value: AppSettings) => void;
  activeDownloadCount: number;
  updater: UpdaterController;
}

export function Settings({ settings, onSaved, activeDownloadCount, updater }: SettingsProps) {
  const [form, setForm] = useState(() => withFixedFilenameTemplate(settings));
  const [saveError, setSaveError] = useState("");
  const [saving, setSaving] = useState(false);
  const [showUpdateDetails, setShowUpdateDetails] = useState(false);
  useEffect(() => setForm(withFixedFilenameTemplate(settings)), [settings]);
  const { status } = updater;
  const hasUpdate = status.availableVersion != null;
  const updateBadgeText = status.phase === "downloading"
    ? `下载中${status.progress == null ? "" : ` ${status.progress}%`}`
    : status.phase === "ready" ? "等待安装"
    : status.phase === "installing" ? "正在安装"
    : status.phase === "error" ? "更新失败"
    : `新版本 v${status.availableVersion}`;
  const choose = async () => {
    const value = await open({ directory: true, multiple: false, defaultPath: form.outputDirectory });
    if (typeof value === "string") await applySettings({ ...form, outputDirectory: value });
  };
  const applySettings = async (next: AppSettings) => {
    const previous = withFixedFilenameTemplate(settings);
    const value = withFixedFilenameTemplate(next);
    setForm(value);
    onSaved(value);
    setSaveError("");
    setSaving(true);
    try {
      const saved = withFixedFilenameTemplate(await saveSettings(value));
      setForm(saved);
      onSaved(saved);
    } catch (error) {
      setForm(previous);
      onSaved(previous);
      setSaveError(friendlyError(error));
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="page narrow-page settings-page">
      <header className="page-header"><span className="eyebrow">PREFERENCES</span><h1>设置</h1><p>为之后的下载设定默认选项</p></header>
      <section className="settings-card">
        <div className="setting-row"><div><label>下载位置</label><p>视频与音频保存到这里</p></div><div className="path-picker"><span title={form.outputDirectory}>{form.outputDirectory || "未选择"}</span><button className="secondary-button" onClick={choose} disabled={saving}><Icon name="folder" size={17} />选择</button></div></div>
        <div className="setting-row"><div><label>默认画质</label><p>解析后优先选择，可随时更改</p></div><select value={form.defaultQuality} disabled={saving} onChange={(e) => void applySettings({ ...form, defaultQuality: e.target.value })}><option>1080p</option><option>720p</option><option>480p</option><option>360p</option></select></div>
        <div className="setting-row"><div><label>文件名模板</label><p>支持 %(title)s、%(id)s、%(quality)s、%(ext)s</p></div><input className="setting-input" value={form.filenameTemplate} disabled title="暂未开放自定义文件名模板" /></div>
        {saveError && <div className="settings-inline-error" role="alert">{saveError}</div>}
      </section>
      <section className="settings-card info-card">
        <div className="setting-row"><div><label>下载核心</label><p>yt-dlp + FFmpeg · 随应用在本地运行</p></div><span className="local-badge"><i />本地模式</span></div>
        <div className="setting-row version-row">
          <div>
            <label>应用版本</label>
            <p>当前版本 v{status.currentVersion}</p>
          </div>
          {hasUpdate && <button className={`update-badge ${status.phase}`} onClick={() => setShowUpdateDetails((value) => !value)} disabled={status.phase === "downloading" || status.phase === "installing"}><i />{updateBadgeText}</button>}
        </div>
        {showUpdateDetails && hasUpdate && (
          <div className="update-panel">
            <div className="update-panel-heading"><div><span>NEW RELEASE</span><h3>YtHarbor v{status.availableVersion}</h3></div>{status.phase === "ready" && <span className="ready-badge"><Icon name="check" size={13} />已下载</span>}</div>
            {status.phase === "available" && <>
              <p className="update-notes">{status.notes || "此版本包含功能改进和问题修复。"}</p>
              <div className="update-panel-actions"><button className="text-button" onClick={() => setShowUpdateDetails(false)}>稍后</button><button className="primary-button update-button" onClick={() => void updater.downloadUpdate()}><Icon name="download" size={16} />下载更新</button></div>
            </>}
            {status.phase === "downloading" && <div className="update-download"><div><span>正在下载更新…</span><b>{status.progress == null ? "—" : `${status.progress}%`}</b></div><div className="progress"><i className={status.progress == null ? "indeterminate" : ""} style={status.progress == null ? undefined : { width: `${status.progress}%` }} /></div><p>下载期间可以继续使用应用，请不要关闭窗口。</p></div>}
            {status.phase === "ready" && <>
              {activeDownloadCount > 0 ? <div className="update-blocked">还有 {activeDownloadCount} 个下载任务正在进行。为避免任务中断，请等待全部完成后再安装。</div> : <p className="update-confirm">安装时应用会短暂关闭，并在完成后自动重新打开。</p>}
              <div className="update-panel-actions"><button className="text-button" onClick={() => setShowUpdateDetails(false)}>稍后安装</button><button className="primary-button update-button" disabled={activeDownloadCount > 0} onClick={() => void updater.installUpdate()}><Icon name="refresh" size={16} />安装并重启</button></div>
            </>}
            {status.phase === "installing" && <div className="update-installing"><span className="inline-spinner" />正在启动安装程序，应用即将重新打开…</div>}
            {status.phase === "error" && <><div className="update-blocked error">{status.error}</div><div className="update-panel-actions"><button className="text-button" onClick={() => setShowUpdateDetails(false)}>稍后</button><button className="secondary-button compact-button" onClick={() => void (status.errorStage === "install" ? updater.installUpdate() : updater.downloadUpdate())}>重试</button></div></>}
          </div>
        )}
        <div className="supported"><span>YouTube</span><span>Bilibili</span><span>抖音</span><span>TikTok</span><small>本地处理 · 不上传下载内容</small></div>
      </section>
    </div>
  );
}
