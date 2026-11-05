import type { FormatOption, MediaKind, VideoMetadata } from "../types";
import { Icon } from "./Icon";

function duration(value: number | null) {
  if (!value) return "时长未知";
  const hours = Math.floor(value / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  const seconds = Math.floor(value % 60);
  return [hours, minutes, seconds].filter((_, index) => index > 0 || hours > 0).map((part) => String(part).padStart(2, "0")).join(":");
}

function size(value: number | null) {
  if (!value) return "大小未知";
  return value > 1024 ** 3 ? `${(value / 1024 ** 3).toFixed(1)} GB` : `${(value / 1024 ** 2).toFixed(0)} MB`;
}

interface Props {
  video: VideoMetadata;
  kind: MediaKind;
  onKindChange: (kind: MediaKind) => void;
  formatId: string;
  onFormatChange: (id: string) => void;
  audioFormat: "mp3" | "m4a";
  onAudioFormatChange: (format: "mp3" | "m4a") => void;
  onDownload: () => void;
  submitting: boolean;
}

export function VideoPreview(props: Props) {
  const formats = props.video.formats.filter((format) => format.kind === props.kind);
  const selected = formats.find((format) => format.id === props.formatId) ?? formats[0];
  return (
    <section className="preview-card fade-in">
      <div className="video-summary">
        <div className="thumbnail">
          {props.video.thumbnail ? <img src={props.video.thumbnail.replace(/^http:/, "https:")} referrerPolicy="no-referrer" alt="" /> : <div className="thumb-placeholder"><Icon name="video" size={30} /></div>}
          <span>{duration(props.video.duration)}</span>
        </div>
        <div className="video-copy">
          <span className={`platform-tag ${props.video.platform.toLowerCase()}`}>{props.video.platform}</span>
          <h2>{props.video.title}</h2>
          <p>{props.video.author || "未知作者"}</p>
        </div>
      </div>

      <div className="divider" />
      <div className="choice-grid">
        <div>
          <label className="section-label">下载类型</label>
          <div className="segmented">
            <button className={props.kind === "video" ? "selected" : ""} onClick={() => props.onKindChange("video")}><Icon name="video" size={17} />视频</button>
            <button className={props.kind === "audio" ? "selected" : ""} onClick={() => props.onKindChange("audio")}><Icon name="music" size={17} />仅音频</button>
          </div>
        </div>
        <div>
          <label className="section-label">{props.kind === "video" ? "清晰度" : "音频格式"}</label>
          {props.kind === "video" ? (
            <div className="format-options">
              {formats.map((format) => <button key={format.id} className={format.id === selected?.id ? "format-pill selected" : "format-pill"} onClick={() => props.onFormatChange(format.id)}>{format.label}</button>)}
            </div>
          ) : (
            <div className="format-options">
              {(["mp3", "m4a"] as const).map((format) => <button key={format} className={props.audioFormat === format ? "format-pill selected" : "format-pill"} onClick={() => props.onAudioFormatChange(format)}>{format.toUpperCase()}</button>)}
            </div>
          )}
        </div>
      </div>
      <div className="download-bar">
        <div><small>预计文件大小</small><strong>{size(selected?.filesize ?? null)}</strong></div>
        <button className="primary-button" onClick={props.onDownload} disabled={props.submitting || !selected}><Icon name="download" size={18} />{props.submitting ? "正在加入…" : "加入下载"}</button>
      </div>
    </section>
  );
}
