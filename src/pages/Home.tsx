import { useEffect, useState } from "react";
import { friendlyError, parseVideo } from "../lib/tauri";
import type { AppSettings, DownloadRequest, MediaKind, VideoMetadata } from "../types";
import { UrlInput } from "../components/UrlInput";
import { VideoPreview } from "../components/VideoPreview";

export function Home({ settings, onStart, onTaskCreated }: {
  settings: AppSettings;
  onStart: (request: DownloadRequest) => Promise<unknown>;
  onTaskCreated: () => void;
}) {
  const [video, setVideo] = useState<VideoMetadata | null>(null);
  const [loading, setLoading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [kind, setKind] = useState<MediaKind>("video");
  const [formatId, setFormatId] = useState("");
  const [audioFormat, setAudioFormat] = useState<"mp3" | "m4a">("mp3");

  useEffect(() => {
    if (!video) return;
    const matches = video.formats.filter((item) => item.kind === kind);
    const preferred = matches.find((item) => item.label.includes(settings.defaultQuality));
    setFormatId((preferred ?? matches[0])?.id ?? "");
  }, [kind, settings.defaultQuality, video]);

  const parse = async (url: string) => {
    setLoading(true); setError(""); setNotice(""); setVideo(null);
    try {
      setVideo(await parseVideo(url));
      setKind("video");
    } catch (err) {
      setError(friendlyError(err));
    } finally {
      setLoading(false);
    }
  };

  const download = async () => {
    if (!video || !formatId) return;
    setSubmitting(true); setError("");
    try {
      await onStart({
        url: video.url, title: video.title, thumbnail: video.thumbnail,
        formatId, formatLabel: video.formats.find((item) => item.id === formatId)?.label ?? "最佳音质",
        kind, outputDirectory: settings.outputDirectory,
        audioFormat: kind === "audio" ? audioFormat : undefined,
      });
      setNotice("任务已加入下载队列");
      onTaskCreated();
    } catch (err) {
      setError(friendlyError(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="page home-page">
      <header className="page-header hero-header">
        <span className="eyebrow">LOCAL-FIRST DOWNLOADER</span>
        <h1>把喜欢的视频，<br /><em>停靠在本地。</em></h1>
        <p>简单、安静地下载公开视频。文件只在你的电脑上流转。</p>
      </header>
      <UrlInput onParse={parse} loading={loading} />
      {error && <div className="alert error-alert">{error}</div>}
      {notice && <div className="toast" role="status">✓ {notice}</div>}
      {video ? <VideoPreview video={video} kind={kind} onKindChange={setKind} formatId={formatId} onFormatChange={setFormatId} audioFormat={audioFormat} onAudioFormatChange={setAudioFormat} onDownload={download} submitting={submitting} /> : (
        !loading && <section className="empty-harbor"><div className="water-lines"><i /><i /><i /></div><p>粘贴链接，开始一次新的下载</p></section>
      )}
    </div>
  );
}
