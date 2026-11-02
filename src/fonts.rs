pub fn installed_families() -> Vec<String> {
    // 与 Slint 使用同一字体枚举库，包含系统及当前用户安装的字体。
    let mut collection = fontique::Collection::new(fontique::CollectionOptions::default());
    let mut families: Vec<_> = collection
        .family_names()
        .filter(|name| !name.is_empty() && !name.starts_with('@'))
        .map(str::to_owned)
        .collect();
    families.sort_by_cached_key(|name| name.to_lowercase());
    families.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    families
}

pub fn default_family() -> &'static str {
    if cfg!(windows) {
        "Microsoft YaHei"
    } else if cfg!(target_os = "macos") {
        "PingFang SC"
    } else {
        "Noto Sans CJK SC"
    }
}

pub fn resolve_family<'a>(preference: &str, installed: &'a [String]) -> &'a str {
    // 配置可跨设备迁移；缺失的字体保留偏好，但显示时回退到平台默认字体。
    installed
        .iter()
        .find(|family| family.eq_ignore_ascii_case(preference))
        .map_or(default_family(), String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_fonts_fall_back_without_changing_preference() {
        let installed = vec!["Example Sans".into()];
        assert_eq!(resolve_family("example sans", &installed), "Example Sans");
        assert_eq!(resolve_family("", &installed), default_family());
        assert_eq!(resolve_family("Missing Font", &installed), default_family());
    }
}
