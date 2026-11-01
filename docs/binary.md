# 二进制说明

[文档中心](https://github.com/Qwerto107/OpenSesame/blob/main/docs/README.md) · [部署指南](https://github.com/Qwerto107/OpenSesame/blob/main/docs/deployment.md) · [下载版本](https://github.com/Qwerto107/OpenSesame/releases)

发行包包含 Windows 桌面程序、使用说明和字体授权文件。解压后直接运行，无需安装 WebView2、.NET 或 Rust。

[下载](#下载与校验) · [启动](#启动程序) · [配置](#配置与数据) · [升级](#升级与回退) · [源码](#源码构建)

## 下载与校验

从 [Releases](https://github.com/Qwerto107/OpenSesame/releases) 选择版本。正式版与预发布均提供同名附件，是否已有附件以对应页面为准。

| 平台 | 压缩包 | 校验文件 |
| --- | --- | --- |
| Windows 10 / 11 x64 | `opensesame-windows-amd64.zip` | `opensesame-windows-amd64.zip.sha256` |

在 PowerShell 中进入下载目录，校验 ZIP：

```powershell
$expected = ((Get-Content .\opensesame-windows-amd64.zip.sha256 -Raw).Trim() -split '\s+')[0]
$actual = (Get-FileHash .\opensesame-windows-amd64.zip -Algorithm SHA256).Hash
if ($actual -ine $expected) { throw 'SHA-256 不一致，请重新下载' }
Expand-Archive .\opensesame-windows-amd64.zip -DestinationPath .\OpenSesame
```

校验文件用于检查下载内容是否一致，不等同于数字签名。当前构建流程不对 EXE 进行代码签名。

## 启动程序

解压后的目录结构：

```text
opensesame-windows-amd64/
├── OpenSesame.exe
├── README.md
└── licenses/
    └── fonts.txt
```

双击 `OpenSesame.exe`，或在上述示例的解压位置运行：

```powershell
& .\OpenSesame\opensesame-windows-amd64\OpenSesame.exe
```

程序使用普通 Windows 用户权限。首次打开为空服务器清单，点击“新增”填写服务端对应的序列及业务端口。默认开启每 30 秒一次的自动检测，启动自动开门默认关闭。完整操作见[使用指南](https://github.com/Qwerto107/OpenSesame/blob/main/docs/usage.md)。

目标系统为 Windows 10 / 11 x64，当前本机实际验证为 Windows 11。默认图形界面使用 FemtoVG，需要可用的 OpenGL 图形驱动；字体及界面资源已内嵌，程序不要求与 README 或字体授权文件位于同一工作目录。

## 配置与数据

配置保存在 `%APPDATA%\OpenSesame\config.json`，与程序所在目录无关。备份、恢复或迁移前先退出程序，再复制该文件。首次添加或保存设置时创建配置。

配置包含明文服务器地址和敲门参数，请勿将个人配置作为公开附件上传。字段和默认值见[配置参考](https://github.com/Qwerto107/OpenSesame/blob/main/docs/configuration.md)。

“序列已发送”不表示服务端一定放行。TCP 可连接只说明连接建立；UDP 使用空数据报，无响应时显示无法确认。当前仅提供开门，不包含关门操作。

## 升级与回退

退出程序并备份配置，将新版本 EXE 放到原目录或新的目录后运行。保留旧 EXE 和对应配置备份即可回退。不要在窗口仍运行时覆盖文件。

故障排查与维护方法见[部署指南](https://github.com/Qwerto107/OpenSesame/blob/main/docs/deployment.md)，版本差异见[更新记录](https://github.com/Qwerto107/OpenSesame/blob/main/docs/changelog.md)。

## 源码构建

若尚无发行附件，或需要自行修改，请按照[开发指南](https://github.com/Qwerto107/OpenSesame/blob/main/docs/development.md)构建。Actions 的检查、打包和发布条件见[发布流程](https://github.com/Qwerto107/OpenSesame/blob/main/docs/releasing.md)。
