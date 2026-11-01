# 发布流程

[文档中心](README.md) · [开发指南](development.md) · [部署指南](deployment.md)

工作流为[构建、检查与发布](../.github/workflows/ci.yml)，按“常规检查 → 多平台打包 → 标签发布”流程组织。Slint 资源随 Rust 程序编译。

[触发](#触发方式) · [构建顺序](#构建顺序) · [产物](#产物) · [权限](#仓库权限) · [发布步骤](#发布与交付)

## 触发方式

| 触发 | 检查 | 产物与发布 |
| --- | --- | --- |
| 分支提交或 PR 命中工作流路径 | 格式、Clippy 和现有库测试 | 不打包、不发布 |
| 手动运行 | 常规检查 | 五组二进制 Actions 产物，不创建 Release |
| 手动启用 `full_checks` | 常规检查加依赖漏洞检查 | 五组二进制，不创建 Release |
| 推送 `vX.Y.Z` | 常规检查及依赖漏洞检查 | 正式 Release 与五组附件，更新 latest |
| 推送 `vX.Y.Z-rc.1` 等 | 同正式版 | 预发布 Release，不更新 latest |

`full_checks` 默认关闭。常规检查在单一 Windows 环境执行，依赖审计扫描整个 Cargo.lock，不限 Windows 依赖。日常检查保留最新提交，标签运行不中途取消。手动选择标签运行也不会发布 Release，发布任务只接受标签 push。

路径过滤包含 `src`、`ui`、`assets`、`i18n`、`scripts`、`.github`、Cargo 文件、构建脚本、工具链、`.gitattributes`、`docs/binary.md` 和 `docs/releasing.md`。仅修改其他文档不触发普通分支检查，标签 push 不受路径过滤限制。

标签格式为 `v主版本.次版本.修订版本[-预发布标识]`，最多 100 字符，数字段不带前导零，不使用 `+` 构建信息。去掉 `v` 后必须与 Cargo 包版本完全一致。

## 构建顺序

1. 校验版本标签，在单一环境运行 Rust 格式、Clippy 和现有库测试；标签或手动启用 `full_checks` 时增加依赖审计。
2. 检查通过后，手动运行或标签触发五平台原生构建、打包和 SHA-256 生成。Windows 使用 GNU 工具链，Linux 使用对应架构的 Ubuntu 22.04，macOS 使用 Intel / ARM runner。
3. 五组二进制任务成功后，标签任务下载附件，创建 Release 草稿，上传缺失文件，重新下载核对 SHA-256 后公开。单个平台构建失败不会取消其他平台，但会阻止发布。

必要任务失败不公开新 Release。重跑不覆盖已有附件，远端校验文件必须与本次构建一致，压缩包本身也必须通过 SHA-256；不一致时保留当前发布状态并失败。

## 产物

| 平台 | runner | Rust 目标 | 压缩包后缀 |
| --- | --- | --- | --- |
| `windows-amd64` | `windows-latest` | `x86_64-pc-windows-gnu` | `.zip` |
| `linux-amd64` | `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| `linux-arm64` | `ubuntu-22.04-arm` | `aarch64-unknown-linux-gnu` | `.tar.gz` |
| `darwin-amd64` | `macos-15-intel` | `x86_64-apple-darwin` | `.zip` |
| `darwin-arm64` | `macos-15` | `aarch64-apple-darwin` | `.zip` |

每个平台提供 `opensesame-{平台}.{后缀}` 和同名 `.sha256`，共十个 Release 附件。包内包含应用、来自 `docs/binary.md` 的 `README.md` 和 `licenses/fonts.txt`，不包含用户配置或工具链。

Actions 二进制产物 `opensesame-binaries-{平台}` 保留 30 天。各任务只读取自己的架构缓存，默认仅 `main` 分支允许保存缓存。

打包固定条目顺序和提交时间，Unix 还固定权限及所有者，Windows 去除 PE 当前链接时间戳。相同环境、输入和提交时间可得到一致压缩包，不保证跨编译器版本产生相同二进制。Windows 未签名，macOS 仅临时签名、未公证。下载与兼容性说明见[二进制说明](binary.md)。

## 仓库权限

默认任务使用 `contents: read`，只有 Release 任务使用 `contents: write`，通过内置 `GITHUB_TOKEN` 上传，无需 PAT、服务器 SSH 或部署凭据。外部 Action 固定提交 SHA；不使用 `pull_request_target`，检出不保留写凭据。

仓库需启用 Actions 并允许声明的令牌权限。macOS 正式签名、公证需要额外开发者凭据，当前未配置或读取这类凭据。

## 发布与交付

1. 更新 Cargo.toml、Cargo.lock 的版本，将[更新记录](changelog.md)的“未发布”内容整理为目标版本并完成验收。
2. 获得提交、推送授权后上传代码，确认常规检查成功；手动启用 `full_checks`，下载各平台候选包验收。
3. 创建并推送与包版本一致的版本标签，由 Actions 发布附件和 Release。

以下版本仅为格式示例，实际版本必须与 Cargo 一致：

```sh
git tag -a v1.2.3 -m "OpenSesame v1.2.3"
git push origin v1.2.3
```

本地检查通过不代表 Linux / macOS runner 已执行，新增平台以对应 Actions 和实机结果为准。流水线不移动已有标签，不更新用户电脑或服务端规则。安装、升级和回退见[部署指南](deployment.md#升级与回退)。
