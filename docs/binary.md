# 二进制说明

[文档中心](https://github.com/Qwerto107/OpenSesame/blob/main/docs/README.md) · [部署指南](https://github.com/Qwerto107/OpenSesame/blob/main/docs/deployment.md) · [下载版本](https://github.com/Qwerto107/OpenSesame/releases)

发行包包含原生应用、使用说明和字体授权文件，无需安装 WebView2、.NET 或 Rust。选择与系统、CPU 架构一致的附件。

[下载](#下载与校验) · [启动](#启动程序) · [配置](#配置与数据) · [升级](#升级与回退) · [源码](#源码构建)

## 下载与校验

从 [Releases](https://github.com/Qwerto107/OpenSesame/releases) 选择版本。正式版与预发布均提供对应平台附件，是否已有附件以发布页面为准。

| 平台 | 压缩包 |
| --- | --- |
| Windows 10 / 11 x64 | `opensesame-windows-amd64.zip` |
| Linux x64 | `opensesame-linux-amd64.tar.gz` |
| Linux ARM64 | `opensesame-linux-arm64.tar.gz` |
| macOS Intel | `opensesame-darwin-amd64.zip` |
| macOS Apple Silicon | `opensesame-darwin-arm64.zip` |
| Web 服务端 · Linux x64 | `opensesame-web-linux-amd64.tar.gz` |

每份压缩包都有同名 `.sha256` 文件。Windows 在 PowerShell 中校验：

```powershell
$expected = ((Get-Content .\opensesame-windows-amd64.zip.sha256 -Raw).Trim() -split '\s+')[0]
$actual = (Get-FileHash .\opensesame-windows-amd64.zip -Algorithm SHA256).Hash
if ($actual -ine $expected) { throw 'SHA-256 不一致，请重新下载' }
```

Linux 和 macOS 在终端中校验，对应替换平台文件名：

```sh
# Linux 示例
sha256sum -c opensesame-linux-amd64.tar.gz.sha256
# macOS 示例
shasum -a 256 -c opensesame-darwin-arm64.zip.sha256
```

校验文件用于检查下载内容是否一致，不等同于发布者身份认证。Windows EXE 尚未签名，macOS app 使用临时签名，尚未进行 Developer ID 签名和公证。

Web 包运行 `opensesame-web`，使用 SQLite，默认监听 `127.0.0.1:18010`，无需图形库。登录和反向代理配置见 [Web 版本](web.md)。以下启动与配置说明针对桌面应用。

## 启动程序

各压缩包包含 `opensesame-{平台}/` 目录，内有应用、`README.md`、`licenses/fonts.txt`。字体与界面资源已内嵌，不要求从解压目录作为工作目录运行。

### 自定义 TCP Flags

桌面版可在新增／编辑服务器时选择 `SYN / ACK / FIN / RST / PSH / URG`，支持 `SYN`、`SYN+ACK`、`FIN` 和 `XMAS`（FIN+PSH+URG）快捷组合。全部取消表示 NONE。标志位由同一服务器的 TCP 步骤共用，不影响 UDP 或业务端口检测。批量设置替换序列时同时替换 Flags，保留各服务器原有的步骤间隔和序列时间上限。

Windows x64 仍只需一个 EXE。默认 SYN 不加载驱动；发送其他组合时需以管理员身份运行。程序内嵌 WinDivert 2.2.2-A，在首次使用时将官方 DLL、签名驱动和许可证释放到 `%LOCALAPPDATA%\OpenSesame\WinDivert\2.2.2-A`，之后核对缓存内容再加载。WinDivert 使用仅发送模式，不捕获或拦截其他网络流量。权限不足时直接报错，不发送部分序列，也不进入后续业务检测。

WinDivert 按上游 LGPL-3.0 许可动态加载，对应源码与完整许可证已内嵌。可在 PowerShell 中提取：

```powershell
Start-Process -FilePath .\OpenSesame.exe -ArgumentList '--export-windivert-source', '.\WinDivert-source' -Wait -WindowStyle Hidden
```

使用 `OpenSesame.exe --windivert-dir "C:\path\to\compatible-windivert"` 可显式加载兼容的替代 DLL 和驱动。默认缓存校验不会限制该替代方式，允许为调试 WinDivert 修改而进行逆向工程。

Linux 使用原始套接字，无需额外驱动。默认 SYN 不需要特殊权限；其他组合需要给程序授予 `CAP_NET_RAW`：

```sh
sudo setcap cap_net_raw=ep /path/to/OpenSesame
```

替换程序后需重新授权。macOS 当前仍使用默认 SYN，其他组合会明确提示不支持。

| 系统 | 应用 | 启动方式 |
| --- | --- | --- |
| Windows | `OpenSesame.exe` | 解压后双击运行 |
| Linux | `OpenSesame` | 解压后在终端执行 `./OpenSesame`，执行权限已保留 |
| macOS | `OpenSesame.app` | 解压后打开，可复制到“应用程序” |

Linux x64 示例：

```sh
tar -xzf opensesame-linux-amd64.tar.gz
cd opensesame-linux-amd64
./OpenSesame
```

Linux 以 Ubuntu 22.04 构建，目标基线为 glibc 2.35，依赖桌面环境中的 X11 / Wayland、字体与图形库，不是完全静态的单文件程序。Ubuntu / Debian 可按需安装：

```sh
sudo apt install libfontconfig1 libxkbcommon0 libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxrandr2 libxi6 libwayland-client0 libwayland-cursor0 libwayland-egl1 libgl1 fonts-noto-cjk
```

默认使用 FemtoVG，需要可用的 OpenGL 驱动。图形驱动异常时，可用 `SLINT_BACKEND=winit-software ./OpenSesame` 验证软件渲染。当前不提供 AppImage、DEB、RPM 或 musl 包。

macOS 构建最低目标为 12.0，实际兼容性需以对应机器验证。测试包仅临时签名，系统可能提示无法验证开发者；核对来源和校验值后，按系统提示在“隐私与安全性”中允许打开。尚未提供 DMG、Developer ID 签名或公证。

首次打开为空清单，点击“新增”填写序列和业务端口。默认每 30 秒检测一次，启动自动开门默认关闭。完整操作见[使用指南](https://github.com/Qwerto107/OpenSesame/blob/main/docs/usage.md)。

## 配置与数据

| 系统 | 配置文件 |
| --- | --- |
| Windows | `%APPDATA%\OpenSesame\config.json` |
| Linux | `$XDG_CONFIG_HOME/OpenSesame/config.json`，未设置或无效时使用 `~/.config/OpenSesame/config.json` |
| macOS | `~/Library/Application Support/OpenSesame/config.json` |

配置与工作目录无关，首次添加或保存设置时创建。备份、恢复或跨平台迁移前先退出程序，再复制 `config.json`。配置包含明文服务器地址和敲门参数，请勿上传个人配置作为公开附件。字段见[配置参考](https://github.com/Qwerto107/OpenSesame/blob/main/docs/configuration.md)。

“序列已发送”不表示服务端一定放行。TCP 可连接只说明连接建立；UDP 使用空数据报，无响应时显示无法确认。当前仅提供开门。

## 升级与回退

退出应用并备份配置，替换同平台、同架构的程序或 `.app` 应用后运行。保留旧应用和对应配置备份即可回退，不要在窗口运行时覆盖文件。

故障排查见[部署指南](https://github.com/Qwerto107/OpenSesame/blob/main/docs/deployment.md)，版本变化见[更新记录](https://github.com/Qwerto107/OpenSesame/blob/main/docs/changelog.md)。

## 源码构建

若尚无附件，或需要自行修改，请按[开发指南](https://github.com/Qwerto107/OpenSesame/blob/main/docs/development.md)构建。检查、五平台打包与发布条件见[发布流程](https://github.com/Qwerto107/OpenSesame/blob/main/docs/releasing.md)。
