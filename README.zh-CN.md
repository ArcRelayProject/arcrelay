<p align="center">
  <img src="icons/icon.png" alt="ArcRelay 标志" width="128">
</p>

<h1 align="center">ArcRelay</h1>

<p align="center"><strong>让你的设备协同工作。</strong></p>

<p align="center">一个本地优先的桌面协同工作区，在设备之间传递内容、输入、文件、打印任务和自动化动作。</p>

<p align="center">
  <a href="https://github.com/ArcRelayProject/arcrelay/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/ArcRelayProject/arcrelay/ci.yml?branch=main&style=flat-square&label=build" alt="构建状态"></a>
  <a href="https://github.com/ArcRelayProject/arcrelay/releases"><img src="https://img.shields.io/github/v/release/ArcRelayProject/arcrelay?include_prereleases&style=flat-square&label=test%20release&color=7c5cff" alt="最新测试版"></a>
  <a href="https://github.com/ArcRelayProject/arcrelay/releases"><img src="https://img.shields.io/github/downloads/ArcRelayProject/arcrelay/total?style=flat-square&color=0ea5e9" alt="下载量"></a>
  <a href="https://github.com/ArcRelayProject/arcrelay/stargazers"><img src="https://img.shields.io/github/stars/ArcRelayProject/arcrelay?style=flat-square&color=f59e0b" alt="Stars"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/ArcRelayProject/arcrelay?style=flat-square&color=16a34a" alt="AGPL-3.0-only"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/macOS-Apple%20Silicon-111111?style=flat-square&logo=apple&logoColor=white" alt="macOS Apple Silicon">
  <img src="https://img.shields.io/badge/Windows-x86__64-0078D6?style=flat-square&logo=windows11&logoColor=white" alt="Windows x86-64">
</p>

<p align="center">
  <a href="README.md">English</a> · 简体中文 · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.de.md">Deutsch</a> · <a href="README.fr.md">Français</a> · <a href="README.es.md">Español</a> · <a href="README.pt-BR.md">Português</a>
</p>

<p align="center"><img src="assets/screenshots/quick-actions.png" alt="ArcRelay 快捷动作" width="920"></p>

> 截图来自当前 ArcRelay 桌面前端，设备和活动内容为开发模式中的示例数据。

## ArcRelay 能做什么

ArcRelay 让日常设备协同贴近操作系统，也让数据尽量留在你自己的网络中。设备通过局域网发现彼此，建立经过认证的加密连接，并且只开放你明确授权的能力。桌面协同不要求注册云端账号。

| | 功能 | 用途 |
| --- | --- | --- |
| ⚡ | 快捷动作 | 在统一搜索界面中启动应用、打开目录、执行脚本并复用常用任务。 |
| 🔁 | 本地自动化 | 组合触发器、条件、确认和有序步骤，并保留可靠的活动记录。 |
| 📋 | 剪贴板工作区 | 搜索和复用文本、结构化内容及图片预览。 |
| 📦 | 附近传输 | 向已授权设备直接发送文件，提供进度、完整性校验和接收策略。 |
| 📁 | 远程文件 | 浏览获准共享的目录，上传、下载、重命名或预览远程内容。 |
| ⌨️ | 跨屏输入 | 按真实位置排列屏幕，让鼠标和键盘自然穿越多台电脑。 |
| 🖨️ | 打印机共享 | 发布本地打印机，并在桌面端追踪远程打印任务。 |
| 🛡️ | 投屏隐私 | 演示或镜像屏幕时遮罩选定应用窗口。 |
| 🤖 | 本地 Agent 接入 | 通过明确的 MCP 权限模型，让本地工具调用获准的 ArcRelay 动作。 |

“本地优先”不等于完全离线。跨设备功能仍需要可互通的局域网、路由私网或 VPN。

## 产品截图

<table>
  <tr>
    <td width="50%"><img src="assets/screenshots/nearby-transfer.png" alt="附近文件传输"></td>
    <td width="50%"><img src="assets/screenshots/input-workspace.png" alt="跨屏输入工作区"></td>
  </tr>
  <tr>
    <td align="center"><strong>附近传输</strong><br>直接、加密的文件发送和清晰的接收控制。</td>
    <td align="center"><strong>跨屏输入</strong><br>可视化管理屏幕、边缘、连接和诊断。</td>
  </tr>
</table>

## 隐私与安全

- 设备连接使用 QUIC 和 TLS 1.3 保护传输内容。
- 配对过程记录设备身份和逐项能力授权，不会一次开放全部功能。
- 共享文件夹、远程输入、打印和 Agent 接入分别控制。
- 更新包发布前使用 Tauri updater 私钥签名，客户端安装前验证签名。
- 官方 macOS 安装包使用 Developer ID Application 签名并经过 Apple 公证。
- 安全问题可按照 [SECURITY.md](SECURITY.md) 私密报告。

## 下载与更新线路

| 线路 | 适合人群 | 发布方式 |
| --- | --- | --- |
| **稳定版** | 日常使用 | 维护者创建 `v主版本.次版本.修订版本` 标签后发布，并成为 GitHub 最新正式版。 |
| **测试版** | 提前体验和反馈 | `main` 每次成功通过 CI 后自动发布，可能包含尚未完全稳定的功能。 |

请从 **[GitHub Releases](https://github.com/ArcRelayProject/arcrelay/releases)** 下载。当前官方目标为 macOS Apple Silicon（`.dmg`）和 Windows x86-64（`-setup.exe`）。Linux 将在原生进程与系统监控服务完成后提供。

打开 **设置 → 通用 → 更新通道** 即可切换稳定版和测试版。启用自动检查后，ArcRelay 会在启动后及每六小时检查所选线路，也可随时手动检查并安装。

两个线路都使用 HTTPS 清单和应用内置的同一把公钥。测试版发布不会改变稳定版线路。ArcRelay 为测试包保留了各平台均可接受的数字修订号区间，并在切回稳定版时识别该区间。

<p align="center"><img src="assets/screenshots/settings-update-channel.png" alt="稳定版与测试版更新线路设置" width="920"></p>

ArcRelay 官方移动端单独分发，其源代码不包含在本仓库中。

## 快速开始

1. 在需要连接的电脑上安装并打开 ArcRelay。
2. 进入 **设置 → 连接**，为每台设备设置容易识别的名称。
3. 添加附近设备，核对配对码，并只批准实际需要的能力。
4. 从侧边栏进入附近传输、远程文件、打印机共享或跨屏输入。

系统权限仅在对应功能需要时请求。macOS 可能根据启用的功能请求辅助功能、屏幕录制或完全磁盘访问权限。

## 从源码构建

需要 Rust stable、Node.js 22，以及 Tauri 2 对应平台的开发依赖。

```bash
git clone https://github.com/ArcRelayProject/arcrelay.git
cd arcrelay
npm ci
npm run tauri -- dev
```

提交代码前运行：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
npm run check
npm test
npm run version:check
```

官方构建可包含专有的截图/OCR sidecar，社区构建不依赖该组件也能运行。维护者通过 `SNIPTRA_ARTIFACT_DIR` 提供官方 sidecar，并使用 `npm run bundle:official` 构建。

## 项目结构与贡献

本仓库包含 Tauri 主机、Svelte 界面、桌面原生集成和发布自动化。共享协议和领域 crate 位于 [ArcRelayProject](https://github.com/ArcRelayProject) 组织下，并固定到经过审核的提交。公开集成清单位于 [`arcrelay-workspace`](https://github.com/ArcRelayProject/arcrelay-workspace)。

欢迎提交 Issue 和范围清晰的 Pull Request。贡献前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)；代码贡献需要签署个人或企业 CLA。

## 协议与商标

版权所有 © 2026 深圳市长柠科技有限公司（Shenzhen Changning Technology Co., Ltd.）。

源代码采用 [GNU AGPL v3.0 only](LICENSE) 许可。ArcRelay 名称、标志和其他品牌资产单独管理；以 ArcRelay 品牌分发修改版前请阅读 [TRADEMARKS.md](TRADEMARKS.md)。
