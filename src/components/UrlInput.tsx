import { useState } from "react";
import { Icon } from "./Icon";

export function UrlInput({ onParse, loading }: { onParse: (url: string) => Promise<void>; loading: boolean }) {
  const [url, setUrl] = useState("");
  const [error, setError] = useState("");

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const value = url.trim();
    if (!value) return setError("请先粘贴视频链接");
    if (!/^https?:\/\//i.test(value)) return setError("链接需要以 http:// 或 https:// 开头");
    setError("");
    await onParse(value);
  };

  return (
    <form className="url-form" onSubmit={submit}>
      <div className={error ? "url-box invalid" : "url-box"}>
        <Icon name="search" size={21} />
        <input value={url} onChange={(e) => { setUrl(e.target.value); setError(""); }} placeholder="粘贴 Bilibili 或 YouTube 视频链接" aria-label="视频链接" />
        {url && !loading && <button type="button" className="icon-button" onClick={() => setUrl("")} aria-label="清空"><Icon name="close" size={17} /></button>}
        <button className="primary-button parse-button" disabled={loading}>{loading ? <><span className="spinner" />解析中</> : "解析视频"}</button>
      </div>
      {error && <p className="field-error">{error}</p>}
      <div className="platform-hints"><span className="youtube-dot" />YouTube <span className="bilibili-dot" />Bilibili <i>仅支持单个视频，暂不解析播放列表</i></div>
    </form>
  );
}
