//! Native resolver for public Douyin single-video share links.
//!
//! This intentionally uses a fresh, in-memory visitor session for every
//! resolve/download operation. It never reads a browser profile, persists a
//! cookie, or sends a user's account cookie to Douyin.

use crate::models::{FormatOption, MediaKind, VideoMetadata};
use reqwest::{
    blocking::{Client, Response},
    header::{
        ACCEPT, ACCEPT_LANGUAGE, CONTENT_RANGE, CONTENT_TYPE, COOKIE, ORIGIN, RANGE, REFERER,
        SET_COOKIE, USER_AGENT,
    },
    redirect::Policy,
    Method,
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, time::Duration};

const USER_AGENT_VALUE: &str = "Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36";
const DOUYIN_HOME: &str = "https://www.douyin.com/";

#[derive(Clone)]
struct Stream {
    id: String,
    label: String,
    height: Option<u32>,
    filesize: Option<u64>,
    bitrate: u64,
    url: String,
}

struct ResolvedVideo {
    id: String,
    title: String,
    author: String,
    duration: Option<f64>,
    thumbnail: Option<String>,
    source_url: String,
    streams: Vec<Stream>,
}

/// An open direct media response. The caller owns streaming it to disk.
pub struct MediaDownload {
    media_url: String,
    pub total_bytes: Option<u64>,
    pub id: String,
}

/// Resolve metadata and quality choices for a public Douyin video.
pub fn parse(input: &str) -> Result<VideoMetadata, String> {
    let (_, video) = resolve(input)?;
    Ok(as_metadata(video))
}

/// Resolve a fresh signed media URL and open it for a native download.
///
/// Direct Douyin CDN URLs are short lived, so they are deliberately resolved
/// at task execution rather than sent to the frontend or written to history.
pub fn open_download(input: &str, format_id: &str) -> Result<MediaDownload, String> {
    let (mut session, video) = resolve(input)?;
    let stream = video
        .streams
        .iter()
        .find(|stream| stream.id == format_id)
        .or_else(|| video.streams.first())
        .ok_or_else(|| "抖音未返回可下载的视频流".to_owned())?;

    let response = session.get(&stream.url, Some(DOUYIN_HOME))?;
    if !response.status().is_success() {
        return Err(format!(
            "抖音媒体地址不可用（HTTP {}），请重新解析后再试",
            response.status()
        ));
    }
    let total_bytes = response.content_length();
    let media_url = response.url().to_string();
    Ok(MediaDownload {
        media_url,
        total_bytes,
        id: video.id,
    })
}

/// Streams a freshly resolved CDN URL. The async client uses a per-read
/// timeout, so a stalled connection is released without imposing a short total
/// timeout on large videos.
pub fn stream_download<F>(download: &MediaDownload, mut on_chunk: F) -> Result<(), String>
where
    F: FnMut(&[u8]) -> Result<(), String>,
{
    let media_url = download.media_url.clone();
    tauri::async_runtime::block_on(async move {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(24 * 60 * 60))
            .redirect(Policy::limited(8))
            .build()
            .map_err(|error| format!("无法初始化抖音媒体连接：{error}"))?;
        let mut response = client
            .get(media_url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.7")
            .header(REFERER, DOUYIN_HOME)
            .send()
            .await
            .map_err(|error| format!("抖音媒体请求失败：{error}"))?;
        if !response.status().is_success() {
            return Err(format!("抖音媒体地址不可用（HTTP {}）", response.status()));
        }
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| format!("抖音媒体下载中断：{error}"))?
        {
            on_chunk(&chunk)?;
        }
        Ok(())
    })
}

fn resolve(input: &str) -> Result<(AnonymousSession, ResolvedVideo), String> {
    let mut session = AnonymousSession::new()?;
    session.register_visitor()?;

    let mut final_url = input.trim().to_owned();
    if !is_aweme_id(&final_url) {
        let response = session.get(&final_url, Some(DOUYIN_HOME))?;
        if !response.status().is_success() {
            return Err(format!(
                "抖音分享链接不可访问（HTTP {}）",
                response.status()
            ));
        }
        final_url = response.url().to_string();
    }

    let aweme_id = extract_aweme_id(&final_url)
        .or_else(|| extract_aweme_id(input))
        .ok_or_else(|| "无法从抖音分享链接中识别视频 ID".to_owned())?;

    let pages = [
        format!("https://www.iesdouyin.com/share/video/{aweme_id}/"),
        format!("https://www.douyin.com/video/{aweme_id}"),
    ];
    for page_url in pages {
        let response = session.get(&page_url, Some(DOUYIN_HOME))?;
        if !response.status().is_success() {
            continue;
        }
        let source_url = response.url().to_string();
        let html = response
            .text()
            .map_err(|error| format!("无法读取抖音分享页：{error}"))?;
        for payload in page_payloads(&html) {
            if let Some(aweme) = find_aweme(&payload) {
                if let Some(video) = resolved_video(aweme, &source_url) {
                    let mut video = video;
                    session.fill_missing_sizes(&mut video);
                    return Ok((session, video));
                }
            }
        }
    }

    Err("抖音分享页未提供可播放的视频流；该视频可能需要签名接口或浏览器兜底".to_owned())
}

fn as_metadata(video: ResolvedVideo) -> VideoMetadata {
    let formats = video
        .streams
        .iter()
        .map(|stream| FormatOption {
            id: stream.id.clone(),
            label: stream.label.clone(),
            height: stream.height,
            extension: "mp4".to_owned(),
            filesize: stream.filesize,
            kind: MediaKind::Video,
        })
        .collect();
    VideoMetadata {
        id: video.id,
        url: video.source_url,
        platform: "Douyin".to_owned(),
        title: video.title,
        author: video.author,
        duration: video.duration,
        thumbnail: video.thumbnail,
        formats,
    }
}

fn resolved_video(aweme: &Value, source_url: &str) -> Option<ResolvedVideo> {
    let video = aweme.get("video")?;
    let streams = streams_from_video(video);
    if streams.is_empty() {
        return None;
    }
    let raw_duration = number(video.get("duration")).or_else(|| number(aweme.get("duration")));
    Some(ResolvedVideo {
        id: string(aweme.get("aweme_id"))
            .or_else(|| string(aweme.get("awemeId")))
            .unwrap_or_default(),
        title: string(aweme.get("desc"))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "未命名抖音视频".to_owned()),
        author: aweme
            .get("author")
            .and_then(|author| {
                string(author.get("nickname")).or_else(|| string(author.get("unique_id")))
            })
            .unwrap_or_default(),
        duration: raw_duration.map(|duration| {
            if duration > 1_000.0 {
                duration / 1_000.0
            } else {
                duration
            }
        }),
        thumbnail: address_url(video.get("cover"))
            .or_else(|| address_url(video.get("origin_cover"))),
        source_url: source_url.to_owned(),
        streams,
    })
}

fn streams_from_video(video: &Value) -> Vec<Stream> {
    let fallback_height = number(video.get("height")).map(|value| value as u32);
    let mut by_id = BTreeMap::<String, Stream>::new();

    if let Some(rates) = video
        .get("bit_rate")
        .or_else(|| video.get("bitRate"))
        .and_then(Value::as_array)
    {
        for rate in rates {
            let address = rate.get("play_addr").or_else(|| rate.get("playAddr"));
            let Some(url) = address_url(address) else {
                continue;
            };
            let height = number(rate.get("height"))
                .or_else(|| address.and_then(|value| number(value.get("height"))))
                .map(|value| value as u32)
                .or(fallback_height);
            let bitrate = number(rate.get("bit_rate"))
                .or_else(|| number(rate.get("bitRate")))
                .unwrap_or(0.0) as u64;
            let id = height
                .map(|value| format!("douyin:{value}p"))
                .unwrap_or_else(|| "douyin:best".to_owned());
            let stream = Stream {
                label: height
                    .map(|value| format!("{value}p"))
                    .unwrap_or_else(|| "最佳画质".to_owned()),
                id: id.clone(),
                height,
                filesize: address
                    .and_then(|value| {
                        number(value.get("data_size")).or_else(|| number(value.get("dataSize")))
                    })
                    .map(|value| value as u64),
                bitrate,
                url: playback_url(url),
            };
            let should_replace = by_id
                .get(&id)
                .is_none_or(|existing| existing.bitrate < stream.bitrate);
            if should_replace {
                by_id.insert(id, stream);
            }
        }
    }

    if by_id.is_empty() {
        if let Some(url) = address_url(video.get("play_addr").or_else(|| video.get("playAddr"))) {
            let id = fallback_height
                .map(|value| format!("douyin:{value}p"))
                .unwrap_or_else(|| "douyin:best".to_owned());
            by_id.insert(
                id.clone(),
                Stream {
                    label: fallback_height
                        .map(|value| format!("{value}p"))
                        .unwrap_or_else(|| "最佳画质".to_owned()),
                    id,
                    height: fallback_height,
                    filesize: None,
                    bitrate: 0,
                    url: playback_url(url),
                },
            );
        }
    }

    let mut streams: Vec<_> = by_id.into_values().collect();
    streams.sort_by(|left, right| {
        right
            .height
            .cmp(&left.height)
            .then_with(|| right.bitrate.cmp(&left.bitrate))
    });
    streams
}

fn playback_url(url: String) -> String {
    // Share pages sometimes expose the watermarked `playwm` URL even in
    // `play_addr`. The signed sibling `play` endpoint is the normal playback
    // stream, so retain the complete query string and alter only the path.
    url.replacen("/playwm/", "/play/", 1)
}

fn address_url(value: Option<&Value>) -> Option<String> {
    let address = value?;
    let list = address
        .get("url_list")
        .or_else(|| address.get("urlList"))?
        .as_array()?;
    list.iter().find_map(|value| string(Some(value)))
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
    })
}

fn string(value: Option<&Value>) -> Option<String> {
    value?.as_str().map(ToOwned::to_owned)
}

fn find_aweme(value: &Value) -> Option<&Value> {
    if let Some(object) = value.as_object() {
        if object.get("video").is_some_and(Value::is_object)
            && (object.contains_key("aweme_id")
                || object.contains_key("awemeId")
                || object.contains_key("desc"))
        {
            return Some(value);
        }
        for key in ["aweme_detail", "awemeDetail"] {
            if let Some(candidate) = object
                .get(key)
                .filter(|candidate| candidate.get("video").is_some())
            {
                return Some(candidate);
            }
        }
        for child in object.values() {
            if let Some(found) = find_aweme(child) {
                return Some(found);
            }
        }
    }
    if let Some(values) = value.as_array() {
        for child in values {
            if let Some(found) = find_aweme(child) {
                return Some(found);
            }
        }
    }
    None
}

fn page_payloads(html: &str) -> Vec<Value> {
    let mut payloads = Vec::new();
    for marker in [
        "_ROUTER_DATA",
        "__INITIAL_STATE__",
        "__UNIVERSAL_DATA_FOR_REHYDRATION__",
        "SIGI_STATE",
    ] {
        let mut offset = 0;
        while let Some(relative) = html[offset..].find(marker) {
            let marker_end = offset + relative + marker.len();
            if let Some(equals) = html[marker_end..].find('=') {
                if equals < 128 {
                    if let Some(value) = json_at(html, marker_end + equals + 1) {
                        payloads.push(value);
                    }
                }
            }
            offset = marker_end;
        }
    }
    if let Some(value) = render_data(html) {
        payloads.push(value);
    }
    payloads
}

fn json_at(input: &str, from: usize) -> Option<Value> {
    let bytes = input.as_bytes();
    let start = (from..bytes.len()).find(|index| matches!(bytes[*index], b'{' | b'['))?;
    let mut expected = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    for index in start..bytes.len() {
        let byte = bytes[index];
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' => expected.push(b'}'),
            b'[' => expected.push(b']'),
            b'}' | b']' if expected.last() == Some(&byte) => {
                expected.pop();
                if expected.is_empty() {
                    return serde_json::from_slice(&bytes[start..=index]).ok();
                }
            }
            b'}' | b']' => return None,
            _ => {}
        }
    }
    None
}

fn render_data(html: &str) -> Option<Value> {
    let marker = "RENDER_DATA";
    let index = html.find(marker)?;
    let content_start = index + html[index..].find('>')? + 1;
    let content_end = content_start + html[content_start..].find("</script>")?;
    let decoded = percent_decode(html[content_start..content_end].trim())?;
    serde_json::from_str(&decoded).ok()
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16)?;
            let low = (bytes[index + 2] as char).to_digit(16)?;
            output.push((high * 16 + low) as u8);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).ok()
}

fn is_aweme_id(value: &str) -> bool {
    value.len() >= 15 && value.len() <= 25 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn extract_aweme_id(value: &str) -> Option<String> {
    if is_aweme_id(value.trim()) {
        return Some(value.trim().to_owned());
    }
    for marker in ["/video/", "/note/", "/share/video/"] {
        if let Some(start) = value.find(marker) {
            if let Some(id) = leading_id(&value[start + marker.len()..]) {
                return Some(id);
            }
        }
    }
    for marker in ["modal_id=", "aweme_id=", "item_id="] {
        if let Some(start) = value.find(marker) {
            if let Some(id) = leading_id(&value[start + marker.len()..]) {
                return Some(id);
            }
        }
    }
    None
}

fn leading_id(value: &str) -> Option<String> {
    let id: String = value
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    is_aweme_id(&id).then_some(id)
}

struct AnonymousSession {
    client: Client,
    cookies: BTreeMap<String, String>,
}

impl AnonymousSession {
    fn new() -> Result<Self, String> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(30))
            .redirect(Policy::limited(8))
            .build()
            .map_err(|error| format!("无法初始化抖音网络会话：{error}"))?;
        Ok(Self {
            client,
            cookies: BTreeMap::new(),
        })
    }

    fn register_visitor(&mut self) -> Result<(), String> {
        let response = self.client.post("https://ttwid.bytedance.com/ttwid/union/register/")
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT, "application/json, text/plain, */*")
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.7")
            .header(CONTENT_TYPE, "application/json")
            .header(ORIGIN, "https://www.douyin.com")
            .header(REFERER, DOUYIN_HOME)
            .json(&json!({ "region": "cn", "aid": 6383, "need_t": true, "service": "www.douyin.com", "domain": ".douyin.com" }))
            .send()
            .map_err(|error| format!("无法建立抖音匿名访客会话：{error}"))?;
        self.absorb_cookies(&response);
        if !response.status().is_success() || !self.cookies.contains_key("ttwid") {
            return Err(format!(
                "无法建立抖音匿名访客会话（HTTP {}）",
                response.status()
            ));
        }
        Ok(())
    }

    fn get(&mut self, url: &str, referer: Option<&str>) -> Result<Response, String> {
        self.get_with_range(url, referer, None)
    }

    fn get_with_range(&mut self, url: &str, referer: Option<&str>, range: Option<&str>) -> Result<Response, String> {
        let mut request = self
            .client
            .request(Method::GET, url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(ACCEPT_LANGUAGE, "zh-CN,zh;q=0.9,en;q=0.7");
        if let Some(referer) = referer {
            request = request.header(REFERER, referer);
        }
        if let Some(range) = range {
            request = request.header(RANGE, range);
        }
        // `ttwid` is an anonymous Douyin visitor cookie, but it still must not
        // be attached to third-party/CDN requests. Reqwest also strips Cookie
        // on cross-host redirects as a second line of defence.
        if !self.cookies.is_empty() && is_douyin_session_host(url) {
            request = request.header(COOKIE, self.cookie_header());
        }
        let response = request
            .send()
            .map_err(|error| format!("抖音请求失败：{error}"))?;
        self.absorb_cookies(&response);
        Ok(response)
    }

    fn fill_missing_sizes(&mut self, video: &mut ResolvedVideo) {
        for stream in &mut video.streams {
            if stream.filesize.is_none() {
                stream.filesize = self.probe_media_size(&stream.url);
            }
        }
    }

    fn probe_media_size(&mut self, url: &str) -> Option<u64> {
        let response = self.get_with_range(url, Some(DOUYIN_HOME), Some("bytes=0-0")).ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.headers().get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit('/').next())
            .and_then(|value| value.parse::<u64>().ok())
            .or_else(|| response.content_length())
    }

    fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn absorb_cookies(&mut self, response: &Response) {
        for raw in response.headers().get_all(SET_COOKIE).iter() {
            let Ok(raw) = raw.to_str() else { continue };
            let Some(pair) = raw.split(';').next() else {
                continue;
            };
            let Some((name, value)) = pair.split_once('=') else {
                continue;
            };
            if !name.trim().is_empty() {
                self.cookies
                    .insert(name.trim().to_owned(), value.trim().to_owned());
            }
        }
    }
}

fn is_douyin_session_host(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    host == "douyin.com"
        || host.ends_with(".douyin.com")
        || host == "iesdouyin.com"
        || host.ends_with(".iesdouyin.com")
}

#[cfg(test)]
mod tests {
    use super::{extract_aweme_id, is_douyin_session_host, playback_url};

    #[test]
    fn extracts_ids_from_supported_share_forms() {
        assert_eq!(
            extract_aweme_id("https://www.douyin.com/video/7699483289760959786"),
            Some("7699483289760959786".to_owned())
        );
        assert_eq!(
            extract_aweme_id("https://www.iesdouyin.com/share/video/7699483289760959786/?x=1"),
            Some("7699483289760959786".to_owned())
        );
        assert_eq!(
            extract_aweme_id("https://www.douyin.com/?modal_id=7699483289760959786"),
            Some("7699483289760959786".to_owned())
        );
    }

    #[test]
    fn prefers_the_non_watermarked_playback_path() {
        assert_eq!(
            playback_url("https://aweme.snssdk.com/aweme/v1/playwm/?video_id=test".to_owned()),
            "https://aweme.snssdk.com/aweme/v1/play/?video_id=test",
        );
    }

    #[test]
    fn only_sends_the_visitor_cookie_to_douyin_hosts() {
        assert!(is_douyin_session_host("https://www.douyin.com/video/123"));
        assert!(is_douyin_session_host("https://www.iesdouyin.com/share/video/123/"));
        assert!(!is_douyin_session_host("https://aweme.snssdk.com/aweme/v1/play/"));
        assert!(!is_douyin_session_host("https://v1-cold.douyinvod.com/video.mp4"));
    }

    #[test]
    #[ignore = "requires a live public Douyin video supplied through YTHARBOR_DOUYIN_TEST_URL"]
    fn resolves_a_live_public_video() {
        let url = std::env::var("YTHARBOR_DOUYIN_TEST_URL")
            .expect("set YTHARBOR_DOUYIN_TEST_URL before running this ignored test");
        let metadata = super::parse(&url).expect("the public share page should resolve");
        assert_eq!(metadata.platform, "Douyin");
        assert!(!metadata.formats.is_empty());
        assert!(metadata
            .formats
            .iter()
            .all(|format| format.kind == crate::models::MediaKind::Video));
        assert!(metadata
            .formats
            .iter()
            .any(|format| format.filesize.is_some_and(|size| size > 0)));
    }

    #[test]
    #[ignore = "requires a live public Douyin video supplied through YTHARBOR_DOUYIN_TEST_URL"]
    fn opens_a_live_playback_stream() {
        let url = std::env::var("YTHARBOR_DOUYIN_TEST_URL")
            .expect("set YTHARBOR_DOUYIN_TEST_URL before running this ignored test");
        let metadata = super::parse(&url).expect("the public share page should resolve");
        let download = super::open_download(&url, &metadata.formats[0].id)
            .expect("the selected playback stream should open");
        let mut header = Vec::new();
        let result = super::stream_download(&download, |chunk| {
            let remaining = 12_usize.saturating_sub(header.len());
            header.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            if header.len() == 12 {
                Err("test-stop-after-header".to_owned())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.unwrap_err(), "test-stop-after-header");
        assert_eq!(&header[4..8], b"ftyp");
    }
}
