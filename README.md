<div align="center">
  <img src="./public/harbor-mark.svg" width="88" alt="YtHarbor logo" />
  <h1>YtHarbor</h1>
  <p>轻量、本地优先的桌面视频下载器。</p>
  <p>
    <a href="https://github.com/linwenqi1/ytharbor/releases/latest">下载最新版本</a>
    · <a href="#支持范围">支持范围</a>
    · <a href="./CONTRIBUTING.md">参与贡献</a>
    · <a href="./LICENSE">MIT License</a>
  </p>
</div>

YtHarbor 将视频解析、下载和后处理保留在本地完成。无需账号，也无需单独安装 yt-dlp、FFmpeg 或 Deno；当前官方版本提供 Windows x64 安装包。

## 功能亮点

- 支持 YouTube、Bilibili、抖音和 TikTok 的公开单视频链接
- 展示标题、作者、封面、时长及可用清晰度
- 下载视频并自动合并音视频，或提取 MP3 / M4A 音频
- 不同清晰度使用独立文件名，可同时保留同一视频的多个版本
- 显示下载进度、速度、预计剩余时间和后处理状态，支持取消任务
- 区分“下载完成”与“文件已存在”，并可直接在资源管理器中定位文件
- 在本地保存设置和最近 200 条下载记录
- 设置修改后立即生效
- 在设置页静默检查新版本，由用户确认后下载并安装更新

## 安装

1. 打开 [Releases](https://github.com/linwenqi1/ytharbor/releases/latest)。
2. 下载名称以 `x64-setup.exe` 结尾的安装程序。
3. 运行安装程序并按提示完成安装。

普通用户不需要单独下载 `.sig` 文件；它由应用自动更新功能用于校验安装包。项目目前没有购买 Windows 代码签名证书，Windows 可能显示 SmartScreen 提示，请确认安装包来自本仓库的 Release 页面。

## 使用方法

1. 粘贴受支持平台的单视频链接并解析。
2. 选择视频清晰度，或切换到音频并选择输出格式。
3. 开始下载；任务页会显示实时进度和处理状态。
4. 完成后通过提示、任务记录或历史记录打开文件所在位置。

下载目录和默认画质可以在设置页修改，变更会立即保存并用于后续任务。

## 支持范围

| 平台 | 当前支持 | 说明 |
| --- | --- | --- |
| YouTube | 公开单视频 | 由 yt-dlp、Deno 和 FFmpeg 处理 |
| Bilibili | 公开单视频 | 由 yt-dlp 和 FFmpeg 处理 |
| 抖音 | 公开单视频 | 使用当前应用会话解析，不读取浏览器 Cookie |
| TikTok | 公开单视频 | 由 yt-dlp 和 FFmpeg 处理 |

当前暂不支持播放列表、登录账号内容、浏览器 Cookie、字幕下载、自定义代理和下载核心在线更新。平台规则和页面结构可能随时变化；如果解析突然失效，欢迎提交 Issue。

## 本地数据与隐私

- 下载文件只会写入用户选择的目录。
- 设置和历史记录以 JSON 文件保存在 Tauri 应用数据目录中。
- YtHarbor 不提供账号系统，也不会将链接、Cookie 或下载内容上传到 YtHarbor 自有服务器。
- 解析和更新检查仍会按功能需要访问对应视频平台、GitHub 及其内容分发服务。

## 开发与贡献

项目基于 Tauri 2、React、TypeScript、Rust、yt-dlp 和 FFmpeg。开发环境搭建、代码检查、提交规范及维护者发布流程请参阅 [CONTRIBUTING.md](./CONTRIBUTING.md)。

提交问题时，请附上应用版本、Windows 版本、视频平台、可公开访问的示例链接和完整错误信息，并在发布前移除个人信息。

## 使用说明

请仅下载你有权保存和使用的内容，并遵守所在地区法律、平台条款及内容许可。YtHarbor 不绕过付费、登录或 DRM 访问控制。

## License

YtHarbor 源代码采用 [MIT License](./LICENSE)。安装包内包含的第三方组件遵循各自的许可证，详情见 [THIRD_PARTY.md](./THIRD_PARTY.md)。
