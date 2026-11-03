use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    let commit = env::var("GITHUB_SHA")
        .ok()
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
        })
        .filter(|value| {
            value.trim().len() >= 8 && value.trim().chars().all(|c| c.is_ascii_hexdigit())
        })
        .map(|value| value.trim()[..8].to_owned())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=OPENSESAME_BUILD_COMMIT={commit}");
    println!("cargo:rerun-if-changed=i18n");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/windows.rc");
    let mut directories: Vec<_> = fs::read_dir("i18n")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    let mut languages = vec![("en".to_string(), "English".to_string())];
    let mut catalog = String::from(
        "fn lookup<'a>(language: &str, source: &'a str) -> &'a str { match (language, source) {\n",
    );
    for directory in directories {
        let tag = directory.file_name().unwrap().to_str().unwrap().to_owned();
        let path = directory.join("LC_MESSAGES/opensesame.po");
        let po = rspolib::pofile(path.to_str().unwrap()).expect("语言资源格式错误");
        let name = po
            .metadata
            .get("X-Display-Name")
            .cloned()
            .unwrap_or_else(|| tag.clone());
        languages.push((tag.clone(), name));
        for entry in po.entries {
            if let Some(text) = entry.msgstr.filter(|text| !text.is_empty()) {
                catalog.push_str(&format!("({tag:?}, {:?}) => {text:?},\n", entry.msgid));
            }
        }
    }
    catalog.push_str("_ => source, } }\n");
    catalog.push_str(&format!(
        "pub const LANGUAGES: &[(&str, &str)] = &{languages:?};\n"
    ));
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("catalog.rs"),
        catalog,
    )
    .unwrap();
    #[cfg(feature = "desktop")]
    desktop();
}

#[cfg(feature = "desktop")]
fn desktop() {
    // Web 构建只生成消息目录，不编译或链接桌面界面。
    let config = slint_build::CompilerConfiguration::new()
        .with_style("fluent".into())
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles)
        .with_default_translation_context(slint_build::DefaultTranslationContext::None)
        .with_bundled_translations("i18n");
    slint_build::compile_with_config("ui/app.slint", config).expect("无法编译 Slint 界面");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/windows.rc", embed_resource::NONE)
            .manifest_required()
            .expect("无法嵌入 EXE 图标");
    }
}
