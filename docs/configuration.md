# 配置参考

[文档中心](README.md) · [使用指南](usage.md) · [部署指南](deployment.md)

配置通常通过界面编辑，文件位置遵循各系统规范。手动修改前请退出程序并备份原文件。

[位置](#配置位置) · [示例](#完整示例) · [根字段](#根字段) · [服务器](#服务器字段) · [设置](#应用设置) · [保存](#校验与保存)

## 配置位置

| 系统 | 配置文件 |
| --- | --- |
| Windows | `%APPDATA%\OpenSesame\config.json` |
| Linux | `$XDG_CONFIG_HOME/OpenSesame/config.json`，未设置或不是绝对路径时使用 `~/.config/OpenSesame/config.json` |
| macOS | `~/Library/Application Support/OpenSesame/config.json` |

配置格式在各平台一致，可在退出程序后复制迁移。设置页显示实际使用的文件路径。

## 完整示例

下列地址为文档专用示例，需替换成自己的服务器；端口、顺序、协议和时间应与服务端配置一致。

```json
{
  "version": 1,
  "servers": [
    {
      "id": "example-server",
      "name": "示例服务器",
      "host": "203.0.113.10",
      "open_sequence": [
        { "port": 7000, "protocol": "tcp" },
        { "port": 8000, "protocol": "udp" },
        { "port": 9000, "protocol": "tcp" }
      ],
      "interval_ms": 200,
      "sequence_timeout_secs": 10,
      "service_port": 22,
      "service_protocol": "tcp",
      "check_interval_secs": 30,
      "check_timeout_secs": 3
    }
  ],
  "current_server": "example-server",
  "settings": {
    "language": "system",
    "auto_knock": false,
    "auto_check": true,
    "check_all": true,
    "check_interval_secs": 30,
    "check_timeout_secs": 3,
    "appearance": "system"
  }
}
```

## 根字段

| 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `version` | `1` | 配置格式版本，目前只接受 `1` |
| `servers` | `[]` | 服务器数组，首次启动无演示记录 |
| `current_server` | `null` | 上次选中服务器的 ID，启动自动开门使用此选择 |
| `settings` | 见下表 | 应用级设置，缺失字段使用默认值 |

## 服务器字段

| 字段 | 默认值 | 范围与含义 |
| --- | --- | --- |
| `id` | 自动生成 | 非空且所有服务器之间唯一，界面创建后无需手工修改 |
| `name` | 空字符串 | 保存时必须填写显示名称 |
| `host` | 空字符串 | 保存时必须填写 IP 或域名，不含空白、协议或路径 |
| `open_sequence` | `[{"port":7000,"protocol":"tcp"}]` | 至少一个步骤，按数组顺序发送 |
| `open_sequence[].port` | 必填 | 敲门端口，整数 `1..65535` |
| `open_sequence[].protocol` | 必填 | `tcp` 或 `udp`，同一序列可混合 |
| `interval_ms` | `200` | 步骤间隔，整数 `10..60000` 毫秒 |
| `sequence_timeout_secs` | `10` | 序列总时限，整数 `1..300` 秒 |
| `service_port` | `22` | 业务端口，整数 `1..65535` |
| `service_protocol` | `tcp` | 业务检测协议，`tcp` 或 `udp` |
| `check_interval_secs` | `30` | 此服务器检测周期，整数 `1..86400` 秒 |
| `check_timeout_secs` | `3` | 此服务器单次检测超时，整数 `1..120` 秒 |

序列还需满足 `(步骤数 - 1) × interval_ms + 25 < sequence_timeout_secs × 1000`，否则保存被拒绝。这里的 25 毫秒用于为单步 TCP 发送保留时间，不代表一定能在该时限内完成真实网络操作。

通过界面新增服务器时，检测时间取应用当前默认值。批量设置步骤不会重置间隔和总时限，批量设置业务端口不会修改检测时间。

## 应用设置

| 字段 | 默认值 | 说明 |
| --- | --- | --- |
| `language` | `system` | `system`、`zh-CN` 或 `en`；保存后立即应用 |
| `auto_knock` | `false` | 启动后为恢复的当前服务器敲门一次 |
| `auto_check` | `true` | 开启周期检测，与自动敲门独立 |
| `check_all` | `true` | 检测全部服务器；`false` 时只检测当前服务器 |
| `check_interval_secs` | `30` | 默认检测周期，整数 `1..86400` 秒 |
| `check_timeout_secs` | `3` | 默认检测超时，整数 `1..120` 秒 |
| `appearance` | `system` | `system`、`light` 或 `dark` |

保存新默认时间时，服务器对应字段等于旧默认值才会跟随更新，其他值保留。当前不单独保存“跟随默认”的标记，因此手工设置为与默认值相同的数值，也会跟随下一次更新。

语言优先匹配完整代码，再匹配基础语言；未知语言回退到 English。服务器名称和输入内容不翻译。缺少 `language` 的旧配置按 `system` 处理。

## 校验与保存

保存前验证整个配置。批量更新先在副本上验证，全部通过后一次应用，不会只修改部分服务器。写入采用临时文件后替换原文件的方式。Unix 新建应用配置目录使用 `0700` 权限，写入配置文件使用 `0600`，仅当前用户可访问。

损坏、无法读取或版本不支持的配置不会被默认值覆盖，程序显示错误并进入禁止写入状态。修复或恢复备份后重新启动。更多维护操作见[备份与恢复](deployment.md#备份与恢复)。

配置为明文，不应提交包含个人地址或序列的实际配置文件。程序不提供 HTTP API、远程配置接口或关门序列字段。
