# 发布流程

[文档中心](README.md) · [开发指南](development.md) · [部署指南](deployment.md)

工作流为[构建、检查与发布](../.github/workflows/ci.yml)，按 QPix 的检查、按需验证、附件命名、保留时间及草稿发布结构编排，交付 Windows x64 完整程序。源码命令见[开发指南](development.md#完整程序)。

[触发](#触发方式) · [构建顺序](#构建顺序) · [产物](#产物) · [权限](#仓库权限) · [发布步骤](#发布与交付)

## 触发方式

| 触发 | 检查 | 产物与发布 |
| --- | --- | --- |
| 分支提交或 PR 命中工作流路径 | Rust 格式、Clippy、库测试 | 不打包、不发布 |
| 手动运行 | 常规检查 | Windows x64 压缩包作为 Actions 产物，不创建 Release |
| 手动启用 `full_checks` | 加依赖审计、隔离界面与本机网络验证、双语预览、发行程序启动检查 | 二进制附件及界面预览，不创建 Release |
| 推送 `vX.Y.Z` | 常规检查、依赖审计、隔离界面与本机网络验证 | 正式 Release 与附件，更新 latest |
| 推送 `vX.Y.Z-rc.1` 等 | 同正式版 | 预发布 Release 与附件，不更新 latest |

`full_checks` 默认关闭。日常检查保留最新提交，版本标签运行不中途取消。手动选择标签运行也不会发布 Release，发布任务只接受标签 push 事件。

工作流路径包含 `src`、`ui`、`assets`、`i18n`、`scripts`、`.github`、Cargo 文件、构建脚本、工具链文件、`.gitattributes`，以及参与打包的 `docs/binary.md` 和发布说明 `docs/releasing.md`。仅修改其他文档时不触发普通分支检查；标签 push 不受路径过滤限制。

标签遵循 `v主版本.次版本.修订版本[-预发布标识]`，最多 100 个字符，数字段不带前导零，不使用 `+` 构建元信息。去掉开头 `v` 后必须与 `Cargo.toml` 的包版本完全一致，不一致时在检查阶段失败。

## 构建顺序

1. Windows runner 使用固定 Rust GNU 工具链与 MSYS2 MINGW64 GCC，运行格式、静态检查和测试。
2. 标签与完整手动检查增加 Cargo.lock 漏洞审计和隔离界面自检；完整手动检查另生成中英文预览。
3. 手动运行或版本标签在检查成功后构建 release EXE，内嵌字体、语言和 Windows 图标，打包并生成 SHA-256。
4. 检查 EXE 平台、GUI 子系统、外部运行库和图标，完整手动检查另验证中英文窗口启动。
5. 标签发布下载二进制产物，验证本地校验文件，创建 Release 草稿并补充缺失附件，重新下载核对后公开。

必要任务失败时不公开新 Release。重跑不覆盖已上传附件，已有校验文件必须与本次构建一致，远端 ZIP 也必须通过 SHA-256；不一致时保留当前 Release 状态并失败，需要人工调查。

## 产物

| 内容 | 名称 | 保留时间 |
| --- | --- | --- |
| Windows 发行附件 | `opensesame-windows-amd64.zip` 与 `.zip.sha256` | 随 Release 保留 |
| Actions 二进制产物 | `opensesame-binaries-windows-amd64` | 30 天 |
| 完整手动检查的预览 | `opensesame-ui-preview` | 7 天 |

压缩包内是 `opensesame-windows-amd64/` 目录，包含 `OpenSesame.exe`、来自 `docs/binary.md` 的 `README.md` 和 `licenses/fonts.txt`。包内不含用户配置、工具链或开发测试文件。

ZIP 固定文件顺序与提交时间，链接器不插入 PE 当前时间。相同输入文件和时间的打包结果一致；工具链、依赖或输入变化仍可能产生不同二进制。程序不进行代码签名。下载与校验见[二进制说明](binary.md#下载与校验)。

## 仓库权限

默认与检查、打包任务使用 `contents: read`，只有 Release 任务使用 `contents: write`，通过内置 `GITHUB_TOKEN` 上传附件，无需配置 PAT、服务器 SSH 或部署凭据。所有外部 Action 固定到提交 SHA。

仓库需启用 Actions，并允许使用工作流声明的令牌权限。PR 检查不使用 `pull_request_target`，检出不保留写入凭据，缓存仅允许 `main` 分支运行保存。

## 发布与交付

1. 更新 `Cargo.toml` 与对应 `Cargo.lock` 中的版本，将[更新记录](changelog.md)的“未发布”内容整理为目标版本，并完成验收。
2. 获得提交和推送授权后上传代码，先确认分支检查通过；可手动启用 `full_checks` 下载并验收候选产物。
3. 创建并推送与包版本一致的版本标签，由 Actions 构建附件和 Release。

以下版本号仅为格式示例，实际值必须与包版本一致：

```sh
git tag -a v1.2.3 -m "OpenSesame v1.2.3"
git push origin v1.2.3
```

首次接入空仓库时，需先将源码和 `.github` 工作流上传到默认分支，Actions 页才会出现手动入口。本地验证脚本通过不等于 GitHub runner 已执行，远端结果以 Actions 页面为准。

工作流自动创建 Release，无需本地手工发布，不移动已有标签，也不更新用户电脑或服务端规则。安装、升级与回退见[部署指南](deployment.md#升级与回退)。
