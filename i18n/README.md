# 语言资源

界面和 Rust 消息共用 gettext PO 文件，构建时嵌入 EXE。英文是源文案；简体中文资源位于 `zh-CN/LC_MESSAGES/opensesame.po`。

设置中的 `language` 保存语言标签，`system` 表示跟随系统。匹配时先检查完整标签，再检查主语言；没有支持的语言时使用英文。旧配置缺少这个字段时自动采用 `system`。

## 新增语言

1. 新建 `<语言标签>/LC_MESSAGES/opensesame.po`，例如 `ja/LC_MESSAGES/opensesame.po`。
2. 填写 UTF-8 翻译和 PO 头部的 `Language`、`Content-Type`、`Plural-Forms`、`X-Display-Name`。后者是下拉列表中的语言原名。
3. 保留参数名称和数量。界面 `@tr(...)` 使用 `{0}` 等位置参数；Rust `msg!(...)` 使用 `{name}` 等命名参数。
4. 重新构建。语言选择列表会自动包含新增资源，不需要修改配置模型或增加语言分支。

缺失或空白的翻译会回退到英文源文案。服务器名称、地址等用户内容不翻译，系统返回的原始诊断信息保持原样。

## 添加文案

- Slint 界面使用 `@tr("English source")`；完整句子作为一个翻译项，不拼接语言片段。
- Rust 使用 `msg!("English {value}", "value" => value)`，显示时调用 `render(language)`。后台任务保存原消息及参数，切换语言后再按当前语言显示。
- 参数默认按原文输出；只有显式传入 `Message` 的参数才会递归翻译，例如表单字段名称。
- 将中文翻译加入 PO 文件，并检查两种语言的布局。不要改变用户内容以适应语言。
