# Web 版本

OpenSesame Web 使用 Rust + Axum，HTML、CSS、JavaScript 和字体均内嵌在程序中。服务器清单、勾选状态和设置保存在 SQLite。发行平台为 Linux amd64，无需桌面环境、Node.js 或独立数据库服务。

## 启动

```sh
sha256sum -c opensesame-web-linux-amd64.tar.gz.sha256
tar -xzf opensesame-web-linux-amd64.tar.gz
cd opensesame-web-linux-amd64
export OPENSESAME_WEB_PASSWORD_FILE=/etc/opensesame/access-password
export OPENSESAME_WEB_DB=/var/lib/opensesame/opensesame-web.sqlite3
export OPENSESAME_WEB_ORIGIN=https://knock.example.com
./opensesame-web
```

密码文件由你创建，内容为至少 12 字节的访问密码，仅服务账号可读。数据库目录需提前创建，并允许服务账号写入。程序默认监听 `127.0.0.1:18010`。远程访问时，将 HTTPS 反向代理指向此地址；`OPENSESAME_WEB_ORIGIN` 必须与浏览器访问的协议、域名和端口一致。

启动后，服务器终端显示 `OpenSesame Web v版本号_提交短号`、版权信息和监听地址；浏览器开发者控制台也会显示带颜色的版本标识。交互式终端使用绿色标签样式，设置 `NO_COLOR` 可关闭终端颜色。

本机测试可只配置访问密码，打开 `http://127.0.0.1:18010`。首次启动清单为空，点击“新增服务器”添加。密码和 Google 登录拥有相同的管理权限，不区分用户数据。

| 环境变量 | 默认值或用途 |
| --- | --- |
| `OPENSESAME_WEB_LISTEN` | `127.0.0.1:18010`，监听地址 |
| `OPENSESAME_WEB_ORIGIN` | `http://127.0.0.1:18010`，浏览器访问来源，含非标准端口 |
| `OPENSESAME_WEB_DB` | 工作目录下的 `opensesame-web.sqlite3` |
| `OPENSESAME_WEB_PASSWORD_FILE` | 访问密码文件，优先于密码环境变量 |
| `OPENSESAME_WEB_PASSWORD` | 无密码文件时使用，12～1024 字节 |
| `OPENSESAME_GOOGLE_CLIENT_ID` | 可选 Google OAuth 客户端 ID |
| `OPENSESAME_GOOGLE_CLIENT_SECRET` | Google OAuth 客户端密钥 |
| `OPENSESAME_GOOGLE_EMAILS` | 允许登录的邮箱，多个邮箱用英文逗号分隔 |

**敲门和检测报文从 Rust 后端所在服务器发出。** knockd 放行的来源 IP 是后端服务器的出口 IP，浏览器所在电脑不会因此获得放行。

## Docker 部署

镜像为 `ghcr.io/qwerto107/opensesame:latest`，支持 Linux amd64。容器名称为 `opensesame`，以 UID/GID `65532:65532` 运行，根目录只读，SQLite 保存在 `./data`。

```sh
mkdir -p opensesame/data
sudo chown 65532:65532 opensesame/data
sudo chmod 700 opensesame/data
cd opensesame
curl -fL https://raw.githubusercontent.com/Qwerto107/OpenSesame/main/docker-compose.yml -o docker-compose.yml
```

在 `docker-compose.yml` 同一目录创建权限为 `0600` 的 `.env` 文件，填写访问密码：

```dotenv
OPENSESAME_WEB_PASSWORD='replace-with-at-least-12-bytes'
OPENSESAME_WEB_ORIGIN=http://127.0.0.1:18010
```

Compose 通过 `env_file: .env` 将文件中的变量直接传入容器，变量名使用上表中的程序配置项。未配置的项目使用镜像或程序默认值；镜像默认监听 `0.0.0.0:18010`，数据库路径为 `/data/opensesame-web.sqlite3`。如需修改监听地址或数据库路径，可在 `.env` 中设置已有的 `OPENSESAME_WEB_LISTEN` 和 `OPENSESAME_WEB_DB`，并相应调整端口映射或数据挂载。

```sh
docker compose config -q
docker compose up -d --pull always
```

修改 `.env` 后运行 `docker compose up -d --force-recreate`，将新配置传入容器；仅运行 `docker compose restart` 不会重新加载环境变量。`config -q` 只校验配置，不输出密码。

打开 `http://127.0.0.1:18010` 登录。Compose 默认只向宿主机回环地址映射端口；远程访问时配置 HTTPS 反向代理，并将 `.env` 中的 `OPENSESAME_WEB_ORIGIN` 改为实际访问来源。Google 登录可在 `.env` 中配置三个对应环境变量。

升级使用 `docker compose pull && docker compose up -d`。备份时先停止容器，再复制整个 `data` 目录。可将 Compose 的镜像标签固定为 `1.0.5` 以便回退。访问密码每次启动都需要提供，修改后旧会话失效。

程序提供 `opensesame-web healthcheck` 命令，检查本机认证状态接口是否正常响应，不依赖 Shell。源码构建可运行 `docker build --build-arg GITHUB_SHA="$(git rev-parse HEAD)" -t opensesame:local .`，再将 Compose 镜像替换为本地标签。

### 自定义 TCP Flags

新增或编辑服务器时可选择 `SYN / ACK / FIN / RST / PSH / URG`，也可使用 `SYN`、`SYN+ACK`、`FIN`、`XMAS` 快捷组合；`XMAS` 为 `FIN+PSH+URG`，全部取消表示不设置标志位（NONE）。同一服务器的 TCP 步骤共用这组标志位，UDP 步骤和业务端口检测不受影响。弹窗实时显示发送预览，详情页显示已保存的 Flags。旧数据默认使用 SYN。

默认 SYN 沿用普通套接字，无需额外权限；其他组合通过 Linux 原始套接字发送，支持 IPv4 和 IPv6。权限不足会在序列发送前报错，不进入后续业务端口检测。

容器镜像为程序设置 `cap_net_raw=ep` 文件能力，Compose 显式保留 `NET_RAW`，服务仍以 UID/GID `65532:65532` 运行。不要对该容器启用 `no-new-privileges`，否则文件能力无法生效。无需使用特权容器。

直接运行 Linux 二进制时，可为已安装的程序授予能力：

```sh
sudo setcap cap_net_raw=ep /opt/opensesame/opensesame-web
getcap /opt/opensesame/opensesame-web
```

替换二进制后需重新执行 `setcap`。systemd 部署也可在服务中设置 `AmbientCapabilities=CAP_NET_RAW` 和 `CapabilityBoundingSet=CAP_NET_RAW`，让服务账号获得该能力。

## Google 登录

在 Google Cloud 创建 Web OAuth 客户端，将授权回调地址配置为：

```text
https://knock.example.com/auth/google/callback
```

同时设置三个 `OPENSESAME_GOOGLE_*` 环境变量。程序启动时检查配置完整性，配置成功才显示“使用 Google 登录”。远程回调要求 HTTPS，本机开发可使用 HTTP loopback 地址。配置变更后重启服务。

后端使用授权码、PKCE、一次性 state 和 nonce，验证 Google ID Token 的签名、签发者、受众、有效期、邮箱验证状态和邮箱白名单。未列入白名单的 Google 账号无法登录。凭据放在服务器环境中，不会发送给前端。

配置方式参考 [Google OpenID Connect 文档](https://developers.google.com/identity/openid-connect/openid-connect)。

## 页面操作

- 服务器列表支持全选、部分选择、单台开门、立即检测、编辑和删除。勾选立即保存，关闭浏览器或重启服务后保留。
- “批量设置”仅修改选中的项目；整组验证成功后在同一事务中保存。“批量开门”在服务器列表中显示总体进度与行内轨道，发送计数、节点与进度条按后端实际发送情况更新。
- 详情页的开门路径显示真实步骤、数据包和发送完成动画，并显示业务端口、最近与下次检测时间，以及最近 10 条运行记录。运行记录保存在内存中，重启后清空。
- 设置页支持自动检测、检测范围、间隔、超时和深浅色主题。Web 版首次打开默认亮色，之后保留已保存的外观选择。“仅当前服务器”指最后打开详情页的服务器，该选择在所有浏览器之间共享。
- 自动检测由前端定时触发，仅在网页可见、已登录且开启自动检测时运行；切到后台、关闭网页或退出登录后暂停。回到网页后恢复检测，已发起的检测和敲门任务正常结束。Rust 后端执行实际 TCP/UDP 探测，不再独立定时检测。编辑或删除服务器会取消该服务器的旧任务。
- 多个浏览器同时修改时，旧版本提交会被拒绝。取消并重新打开编辑窗口，或在设置页点击“取消”，确认最新值后再修改。

登录页的“在此浏览器保存密码”实际保存 HttpOnly 会话 Cookie，浏览器不会保存明文访问密码。勾选后会话有效期为 30 天；未勾选时使用最长 8 小时的浏览器会话。Google 登录使用浏览器会话。设置页可清除保存状态或退出登录。修改密码、Google 配置或站点来源后，旧会话失效。

密码登录按后端观察到的 IP 限制为每分钟 10 次。反向代理后的请求共用代理地址，不信任客户端传入的转发头。所有写入 API 要求同源自定义请求头，服务不开放跨域 API。

前端空闲时每 10 秒同步一次状态，有任务运行时每秒同步一次，操作后立即同步。页面隐藏时暂停同步，恢复可见后重新获取状态；详情页倒计时在浏览器本地更新。敲门期间通过经过登录校验的 SSE 连接逐步推送发送进度，任务结束或页面隐藏时关闭连接。状态版本用于避免较旧的轮询结果覆盖最新步骤。

反向代理需允许 `/api/events` 的 `text/event-stream` 响应及时转发。后端设置了 `X-Accel-Buffering: no`；Nginx 也可对该路径配置 `proxy_buffering off`。事件连接失败时仍保留普通轮询。锁打开和绿色光晕表示序列已发送，业务端口是否可连接以下方检测结果为准。

## 数据与升级

每个实例使用独立数据库，同一个 SQLite 文件只运行一个服务实例。SQLite 内嵌构建，不需要安装 SQLite 动态库。Linux 新建数据库权限为 `0600`；数据库目录也应限制为服务账号可访问。

备份前停止服务，再复制 SQLite 文件；若存在同名 `-wal`、`-shm` 文件，也应保留。升级时先备份数据库，再替换 `opensesame-web`。数据库保存服务器参数和会话摘要，应作为私有数据管理。Web 版不读取或覆盖桌面版的 `config.json`。

可通过 systemd 的 `WorkingDirectory`、`EnvironmentFile`、`User` 和 `ExecStart` 配置服务，环境文件权限应为 `0600`。在同机 HTTPS 反向代理后保留默认 loopback 监听即可。

## 源码构建

```sh
# 独立检查，不加载 Slint 桌面依赖
bash scripts/build.sh check-web
bash scripts/build.sh test-web
# 前端自动检测的可见性、登录状态及周期验证
node scripts/test-web-auto-check.cjs
# TCP Flags 快捷组合、切换和发送预览验证
node scripts/test-web-tcp-flags.cjs
# 具有 CAP_NET_RAW 的 Linux 环境可额外运行回环抓包测试
cargo test --locked --no-default-features --features web captures_custom_flags -- --ignored
# Linux amd64 发行构建
bash scripts/build.sh release-web
# 打包及生成 SHA-256
bash .github/scripts/package-web.sh
```

构建输出 `dist/opensesame-web`，发行包为 `dist/release/opensesame-web-linux-amd64.tar.gz`，同时生成同名 `.sha256`。压缩包包含程序、此文档和字体授权。前端修改后需要重新编译 Rust 程序，预览页面需要刷新。

Copyright © 2026 Qwerto107. All rights reserved.
