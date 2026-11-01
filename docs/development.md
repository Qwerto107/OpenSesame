# 开发指南

[文档中心](README.md) · [配置参考](configuration.md) · [发布流程](releasing.md)

本项目使用 Rust + Slint，目标为 Windows x64、Linux x64 / ARM64、macOS Intel / Apple Silicon，界面、字体、翻译与各平台应用图标随程序构建。

[环境](#开发环境) · [目录](#代码结构) · [运行](#本地运行) · [构建](#完整程序) · [验证](#按需验证) · [约定](#实现约定)

## 开发环境

下述环境和命令适用于 Windows；Linux / macOS 的原生构建见[Unix 开发](#unix-开发)。

| 工具 | 用途 |
| --- | --- |
| Windows x64、PowerShell 7 | 本地开发与打包脚本 |
| Rust `1.99.1-x86_64-pc-windows-gnu` | 与 `rust-toolchain.toml`、CI 保持一致 |
| rustfmt、Clippy | 格式与静态检查 |
| MSYS2 MINGW64 GCC | GNU 链接器、`ar`、`windres`、`objdump` |
| Git | 检出代码，并提供发行包固定时间戳 |

安装 Rust 后，在 PowerShell 中安装项目工具链：

```powershell
rustup toolchain install 1.99.1-x86_64-pc-windows-gnu --profile minimal --component rustfmt --component clippy
$env:RUSTUP_TOOLCHAIN = '1.99.1-x86_64-pc-windows-gnu'
```

在 MSYS2 MINGW64 终端安装 `mingw-w64-x86_64-gcc`，再将其 `mingw64\bin` 加入当前 PowerShell 会话的 PATH。通常位于 `C:\msys64\mingw64\bin`，应以实际安装位置为准：

```powershell
$env:PATH = "C:\msys64\mingw64\bin;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = 'C:\msys64\mingw64\bin\gcc.exe'
$env:CC = 'C:\msys64\mingw64\bin\gcc.exe'
$env:AR = 'C:\msys64\mingw64\bin\ar.exe'
```

构建脚本也兼容项目 `.tools` 中的便携式 Rust / MinGW 工具链；这些缓存不属于源码交付内容。应用运行无需上述开发工具。

## Unix 开发

需要 Rust 1.99.1、Git、Bash 和 Python 3.9 以上。macOS 需 Xcode Command Line Tools，打包使用系统的 `sips`、`iconutil` 和 `codesign`。Linux 安装以下依赖：

```sh
sudo apt install build-essential pkg-config libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev fonts-noto-cjk
bash scripts/build.sh fmt
bash scripts/build.sh lint
bash scripts/build.sh test
bash scripts/build.sh release
./dist/OpenSesame
```

Unix 按当前系统和 CPU 选择一条打包命令，脚本拒绝不匹配的架构：

```sh
bash .github/scripts/package-binary.sh linux-amd64
bash .github/scripts/package-binary.sh linux-arm64
bash .github/scripts/package-binary.sh darwin-amd64
bash .github/scripts/package-binary.sh darwin-arm64
```

Linux 输出 TAR.GZ，macOS 输出包含图标和临时签名的 `.app` ZIP。macOS 本地构建应设置 `MACOSX_DEPLOYMENT_TARGET=12.0`，与 CI 一致。正式 Developer ID 签名、公证需另行配置。Unix 不要求安装 PowerShell。

本地开发需要排查界面时，可手动使用隔离验证和预览入口；这些命令不加入 CI：

```sh
bash scripts/build.sh qa
./target/debug/opensesame --self-test
./target/debug/opensesame --screenshot artifacts/en-main-light.png --scene main-light --language en
```

## 代码结构

| 路径 | 内容 |
| --- | --- |
| `src/model.rs` | 配置模型、字段校验与批量更新 |
| `src/network.rs` | TCP / UDP 敲门、业务探测及同 IP 互斥 |
| `src/controller.rs` | 界面回调、调度、设置与状态管理 |
| `src/storage.rs` | 平台配置目录、读取校验与安全写入 |
| `src/i18n.rs`、`i18n/` | 运行时消息与语言目录 |
| `src/preview.rs` | `qa` 功能下的隔离验证与截图 |
| `ui/` | Slint 主窗口、组件和主题 |
| `assets/`、`build.rs` | 字体、图标、状态 SVG 和构建资源 |
| `scripts/build.ps1`、`scripts/build.sh` | Windows 与 Unix 构建入口，Bash 入口也可转发 Windows 操作 |
| `.github/` | CI 环境、检查、打包和发布脚本 |

## 本地运行

在源码根目录执行：

```powershell
.\scripts\build.ps1 -Action build
.\target\debug\opensesame.exe
```

普通开发程序与发行程序使用同一[平台配置目录](configuration.md#配置位置)。仅验证界面时优先使用下述隔离入口，避免自动敲门或检测真实服务器。

## 完整程序

```powershell
.\scripts\build.ps1 -Action release
```

输出 `dist\OpenSesame.exe`。字体、语言和图标已内嵌，默认使用 FemtoVG，不需要 WebView2、.NET 或旁置资源目录。

生成与 Actions 相同的发行包：

```powershell
.\.github\scripts\package-binary.ps1
```

打包脚本从 Git 提交读取时间，设置 PE 去时间戳链接参数，固定 ZIP 条目顺序和时间，并生成 SHA-256。输出位于 `dist\release`。`-SkipBuild` 可复用已经构建的 EXE；没有 Git 元数据的源码目录需显式提供 `-SourceDateEpoch <Unix秒数>`。相同输入文件与时间可得到一致压缩包，不保证跨编译器或不同依赖版本的 EXE 完全一致。

## 按需验证

日常修改先执行适合当前变更的检查，完整流程见[发布流程](releasing.md)。

```powershell
.\scripts\build.ps1 -Action fmt
.\scripts\build.ps1 -Action lint
.\scripts\build.ps1 -Action test
```

`fmt` 检查格式，`lint` 检查所有目标及功能并将警告视为错误，`test` 运行库测试。界面或调度修改可增加隔离验证：

```powershell
.\scripts\build.ps1 -Action qa
.\target\debug\opensesame.exe --self-test
.\target\debug\opensesame.exe --screenshot artifacts\zh-CN-main-light.png --scene main-light --language zh-CN
.\target\debug\opensesame.exe --screenshot artifacts\en-settings-light.png --scene settings-light --language en
```

自检使用独立配置与本机回环网络，覆盖界面操作、单台与批量开门、周期检测、批量独立更新、主题和语言切换。截图使用软件离屏渲染，不连接演示服务器。可用场景还包括 `main-dark`、`editor-dark`、`editor-error-light`、`batch-settings-light`、`batch-result-dark`、`empty-light` 和 `protocol-light`。

`qa` 未被默认启用，发行程序不包含上述测试命令行入口。它只用于本地开发，CI 不运行界面自检、截图或窗口启动验证。

依赖审计按需执行，与 CI 使用同一工具版本：

```powershell
cargo install cargo-audit --version 0.22.2 --locked --no-default-features
cargo audit --file Cargo.lock
```

审计发现漏洞时阻止发布，停止维护类警告保留在日志中供评估，不自动升级依赖或添加忽略项。

## 实现约定

- TCP 敲门对目标端口发起普通非阻塞连接尝试，端口拒绝或无响应仍可完成 SYN 发送。服务端建议配置 `TCPFlags = syn`。
- UDP 使用空数据报。无回复是“无法确认”，不等于端口关闭，也不等于业务可用。
- 同一解析 IP 的敲门与业务探测互斥；不同 IP 可并行，周期从本轮完成后计算，避免重叠和堆积。
- 异步结果按服务器 ID 和当前任务代次核对，配置编辑或删除后不应用过期结果。
- 字体按平台选择微软雅黑、苹方或 Noto Sans CJK，复选标记使用 SVG；系统主题由 Slint 读取，各平台统一使用 Fluent 标准控件。主题、语言和状态文案复用现有组件与翻译资源。新增语言遵循[语言资源说明](../i18n/README.md)，不要复制业务逻辑。
- 代码注释使用中文，修改范围遵循项目协作规则；验收并获得明确授权后才提交，不擅自推送。

原始[需求说明](requirements.md)及[设计对应与实施说明](implementation.md)作为开发背景保留，当前用户行为以使用指南和配置参考为准。
