# 文档中心

[项目首页](../README.md) · [下载版本](https://github.com/Qwerto107/OpenSesame/releases)

## 使用

| 要做的事 | 文档 |
| --- | --- |
| 了解敲门序列如何触发服务端放行 | [knockd 工作机制](usage.md#knockd-工作机制) |
| 添加服务器、开门和查看端口状态 | [使用指南](usage.md) |
| 配置自动开门、周期检测和批量设置 | [自动行为](usage.md#自动行为) · [批量操作](usage.md#批量操作) |
| 查看配置字段、默认值与限制 | [配置参考](configuration.md) |
| 查看版本变化 | [更新记录](changelog.md) |

## 部署

下载对应系统和架构的发行包，解压后运行。程序与配置分开保存，升级时替换应用即可。

| 要做的事 | 文档 |
| --- | --- |
| 了解运行要求和安装方式 | [部署指南](deployment.md) |
| 下载、校验并运行发行程序 | [二进制说明](binary.md) |
| 备份、恢复、升级或排查故障 | [备份与恢复](deployment.md#备份与恢复) · [升级与回退](deployment.md#升级与回退) · [故障排查](deployment.md#故障排查) |

## 开发

| 要做的事 | 文档 |
| --- | --- |
| 本地开发、源码构建、验证及了解内部实现 | [开发指南](development.md) |
| 配置 CI、构建产物并发布版本 | [发布流程](releasing.md) |
| 扩展语言资源 | [语言资源说明](../i18n/README.md) |

## 默认路径

| 内容 | 路径 | 说明 |
| --- | --- | --- |
| 发行程序 | Windows：`OpenSesame.exe`；Linux：`OpenSesame`；macOS：`OpenSesame.app` | 可自行选择目录 |
| 用户配置 | 见[平台配置目录](configuration.md#配置位置) | 当前用户，首次保存时创建 |
| 本地构建程序 | Windows：`dist/OpenSesame.exe`；Unix：`dist/OpenSesame` | 从源码根目录构建 |
| 发行压缩包 | `dist/release/opensesame-{平台}.{zip或tar.gz}` | 与同名 `.sha256` 一起交付 |

界面预览见[项目首页](../README.md#界面一览)，平台及附件以对应 Release 为准。
