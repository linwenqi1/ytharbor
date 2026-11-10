# Contributing to YtHarbor

感谢你愿意帮助改进 YtHarbor。本文档面向准备提交 Issue、代码或文档的贡献者，也包含维护者使用的发布说明。

## 提交 Issue

提交前请先搜索已有 Issue，确认问题尚未被报告。一个可操作的问题报告通常包括：

- YtHarbor 版本和 Windows 版本
- 视频平台及可公开访问的示例链接
- 期望行为与实际行为
- 可以稳定复现问题的步骤
- 完整错误信息或经过脱敏的日志

请勿公开账号凭据、Cookie、私有视频链接、签名私钥或其他敏感信息。平台页面经常变化；如果某个此前可用的公开链接突然无法解析，也欢迎报告。

## 开发环境

需要准备：

- Node.js 20 或更高版本
- Rust stable
- [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)
- Git

安装前端依赖并启动桌面开发环境：

```bash
npm install
npm run tauri dev
```

开发时可以使用系统安装的 yt-dlp、FFmpeg 和 Deno，也可以通过环境变量指定路径：

```text
YTHARBOR_YTDLP=/path/to/yt-dlp
YTHARBOR_FFMPEG_DIR=/path/to/ffmpeg-directory
YTHARBOR_DENO=/path/to/deno
```

Windows 打包所需的捆绑工具可在 PowerShell 中准备：

```powershell
.\scripts\prepare-windows.ps1
```

该脚本会下载构建所需的第三方二进制文件。请勿将生成的 sidecar 或资源文件提交到 Git。

## 项目结构

```text
src/                   React UI、页面、Hooks 和 IPC 封装
src-tauri/src/         Tauri 命令、解析、下载队列和本地存储
src-tauri/binaries/    构建时准备的 yt-dlp sidecar
src-tauri/resources/   构建时准备的 FFmpeg 与 Deno 资源
scripts/               第三方工具准备脚本
.github/workflows/     Windows 发布工作流
```

## 开发原则

- 保持本地优先，不引入不必要的服务端依赖或遥测。
- 对用户输入和文件路径进行验证，不通过 shell 字符串拼接执行下载命令。
- 保持现有界面风格和交互密度，避免为了单一功能大幅改变布局。
- 平台相关逻辑应明确限定适用范围，避免修复一个平台时破坏其他平台。
- 不提交密钥、Cookie、下载样本、构建产物或受版权保护的媒体文件。
- 新增第三方依赖或捆绑资源时，同步检查并更新 `THIRD_PARTY.md`。

## 修改与验证

创建范围清晰的分支，并尽量让每个 Pull Request 只解决一个问题。提交前至少运行：

```bash
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo check --manifest-path src-tauri/Cargo.toml
```

涉及下载流程时，还应手动检查受影响平台的公开单视频链接，并覆盖：

- 视频与音频下载
- 不同清晰度的文件命名
- 文件已存在的识别
- 取消任务和失败状态
- 下载完成后的“打开文件位置”

不要在自动化测试或 Issue 中使用需要登录、包含个人信息或无权分发的内容。

## 提交信息

项目使用简洁的 Conventional Commits 风格，例如：

```text
feat(downloads): reveal completed files
fix(ci): use the official npm registry
refactor(updates): simplify the release indicator
docs: refresh contribution guide
chore: release v0.2.3
```

常用类型包括 `feat`、`fix`、`refactor`、`docs`、`test` 和 `chore`。提交信息使用英文、祈使语气，并说明改动的实际结果。

## Pull Request

Pull Request 应说明：

- 解决的问题和采用的方案
- 用户可见的行为变化
- 已执行的验证步骤
- UI 改动的截图或录屏（如适用）
- 已知限制、兼容性影响或后续工作

维护者可能要求拆分无关改动、补充错误处理或增加针对不同平台的验证。

## 维护者发布流程

发布前同步更新以下版本位置：

- `package.json` 与 `package-lock.json`
- `src-tauri/Cargo.toml` 与 `src-tauri/Cargo.lock`
- `src-tauri/tauri.conf.json`

确认前端构建、Rust 检查和 Windows 安装包测试通过后，创建发布提交和带注释的标签：

```bash
git commit -m "chore: release v0.2.3"
git tag -a v0.2.3 -m "YtHarbor v0.2.3"
git push --atomic origin main v0.2.3
```

`v*` 标签会触发 Windows 发布工作流，生成 NSIS 安装包、更新签名和 `latest.json`。仓库必须配置 `TAURI_SIGNING_PRIVATE_KEY`；更新私钥不得提交到 Git，并应安全备份。公开发布的标签应保持不可变，如需修复请发布新的补丁版本。

正式发布前还应确认第三方工具版本、校验值和许可证材料符合 [THIRD_PARTY.md](./THIRD_PARTY.md)。

## 许可证

向本项目提交贡献即表示你同意在 [MIT License](./LICENSE) 下提供相应内容。你提交的代码和资源必须由你原创，或具有与本项目兼容且允许再分发的许可证。
