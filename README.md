# YtHarbor

轻量、本地优先的桌面视频下载器。基于 Tauri 2、React、Rust、yt-dlp 与 FFmpeg，当前支持 YouTube、Bilibili、抖音和 TikTok 单视频链接。

## 已实现

- 解析标题、作者、封面、时长和可用清晰度
- 下载视频并自动合并音视频
- 提取 MP3 / M4A 音频
- 单任务串行队列、实时进度、速度、ETA、后处理状态和取消
- 本地设置与最近 200 条历史记录（JSON，无数据库）
- URL 平台白名单、参数数组调用、文件名模板路径校验
- YouTube、Bilibili 与 TikTok 单视频解析与下载（yt-dlp）
- 抖音公开单视频原生解析与无水印播放流下载；仅使用应用本次运行的匿名访客会话，不读取浏览器 Cookie
- Windows sidecar / FFmpeg / Deno 资源打包配置（完整 YouTube 提取链路）
- 设置页静默检查、确认下载和安装应用更新

播放列表、Cookie、代理、字幕和核心在线更新有意留到后续版本，当前范围保持简单。

## 开发环境

- Node.js 20+
- Rust stable 与 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)
- 开发时可安装系统 `yt-dlp` 与 `ffmpeg`，或通过环境变量指定：
  - `YTHARBOR_YTDLP=/path/to/yt-dlp`
  - `YTHARBOR_FFMPEG_DIR=/path/to/ffmpeg-directory`
  - `YTHARBOR_DENO=/path/to/deno`

```bash
npm install
npm run tauri dev
```

只检查前端：

```bash
npm run build
```

## Windows 打包

在 PowerShell 中运行：

```powershell
npm install
.\scripts\prepare-windows.ps1
npm run tauri build
```

准备脚本会把 `yt-dlp` 按 Tauri target triple 命名，并把 `ffmpeg.exe` / `ffprobe.exe` 与 Deno 放入资源目录。Deno 用于 yt-dlp 的 YouTube JavaScript 挑战处理；最终用户无需另外安装这些工具。

正式分发前请固定下载核心版本、校验 SHA-256，并按 [THIRD_PARTY.md](./THIRD_PARTY.md) 补齐许可证材料。YouTube 的提取能力会随站点变化，发布测试应持续覆盖 yt-dlp + EJS + Deno 组合。

## 自动发布 Windows 安装包

推送 `v*` Git Tag 会触发 `.github/workflows/release.yml`：GitHub Actions 会在 Windows runner 上准备下载核心、构建签名的 NSIS `-setup.exe` 更新包，并将安装器、更新签名和 `latest.json` 上传到同名 GitHub Release。

自动更新使用免费的 Tauri 更新签名，不依赖付费的 Windows 代码签名证书。首次发布前，将本地生成且已被 Git 忽略的 `.local/ytharbor-updater.key` 内容保存到仓库的 GitHub Actions Secret `TAURI_SIGNING_PRIVATE_KEY`，并在安全位置备份该私钥。私钥丢失后，现有安装无法验证后续更新。

发布前请先把项目版本更新为要发布的版本（`package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json`），然后推送 Tag，例如：

```bash
git tag v0.1.0
git push origin v0.1.0
```

正式构建的应用使用 Windows GUI 子系统，因此从安装包或快捷方式启动时不会额外弹出黑色终端窗口；`npm run tauri dev` 仍保留开发控制台。

## 结构

```text
src/                   React UI、页面、任务 Hook 和 IPC 封装
src-tauri/src/         命令、元数据、下载队列、进度与存储
src-tauri/binaries/    yt-dlp sidecar（构建前生成，不提交）
src-tauri/resources/   FFmpeg 资源（构建前生成，不提交）
scripts/               各平台依赖准备脚本
```

## 数据位置

配置与历史保存在 Tauri 的应用数据目录中，下载文件直接写入用户选择的文件夹。YtHarbor 不需要账号，也不会把链接、Cookie 或下载内容上传到自有服务器。
