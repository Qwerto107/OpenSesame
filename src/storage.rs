use crate::model::{Config, new_id};
use crate::{i18n::Message, msg};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct ConfigStore {
    pub path: PathBuf,
}

impl ConfigStore {
    pub fn app_data() -> Result<Self, Message> {
        Ok(Self {
            path: config_directory(|name| std::env::var_os(name))?.join("config.json"),
        })
    }

    pub fn load(&self) -> Result<Config, Message> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Config::default());
            }
            Err(error) => {
                return Err(
                    msg!("Could not read the configuration: {error}", "error" => error.to_string()),
                );
            }
        };
        let config: Config = serde_json::from_slice(&bytes)
            .map_err(|error| msg!("Invalid configuration; the original file was preserved: {error}", "error" => error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, config: &Config) -> Result<(), Message> {
        config.validate()?;
        let parent = self
            .path
            .parent()
            .ok_or("Invalid configuration directory.")?;
        let mut directories = fs::DirBuilder::new();
        directories.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directories.mode(0o700);
        }
        directories.create(parent).map_err(|error| msg!("Could not create the configuration directory: {error}", "error" => error.to_string()))?;
        let temp = parent.join(format!("config-{}.tmp", new_id()));
        let result = self.write_and_replace(config, &temp);
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }

    fn write_and_replace(&self, config: &Config, temp: &Path) -> Result<(), Message> {
        let bytes =
            serde_json::to_vec_pretty(config).map_err(|error| Message::from(error.to_string()))?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // 配置含服务器地址与敲门参数，Unix 文件只允许当前用户读写。
            options.mode(0o600);
        }
        let mut file = options
            .open(temp)
            .map_err(|error| Message::from(error.to_string()))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| msg!("Could not write the configuration: {error}", "error" => error.to_string()))?;
        drop(file);
        // 同目录写入后替换，避免写到一半破坏上一次保存的配置。
        fs::rename(temp, &self.path).map_err(|error| msg!("Could not replace the configuration: {error}", "error" => error.to_string()))
    }
}

fn config_directory(get: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<PathBuf, Message> {
    let required = |name| {
        get(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| Message::from("The user configuration directory could not be found."))
    };
    #[cfg(windows)]
    let root = required("APPDATA")?;
    #[cfg(target_os = "macos")]
    let root = required("HOME")?.join("Library/Application Support");
    #[cfg(target_os = "linux")]
    let root = match get("XDG_CONFIG_HOME").map(PathBuf::from) {
        // XDG 只接受绝对路径；空值或相对路径回退到用户目录。
        Some(path) if path.is_absolute() => path,
        _ => required("HOME")?.join(".config"),
    };
    Ok(root.join("OpenSesame"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_directory_respects_platform_conventions() {
        #[cfg(windows)]
        let (variable, value, expected) = (
            "APPDATA",
            r"C:\Users\test\AppData\Roaming",
            r"C:\Users\test\AppData\Roaming\OpenSesame",
        );
        #[cfg(target_os = "macos")]
        let (variable, value, expected) = (
            "HOME",
            "/Users/test",
            "/Users/test/Library/Application Support/OpenSesame",
        );
        #[cfg(target_os = "linux")]
        let (variable, value, expected) = ("HOME", "/home/test", "/home/test/.config/OpenSesame");
        assert_eq!(
            config_directory(|name| (name == variable).then(|| value.into())).unwrap(),
            PathBuf::from(expected)
        );
        assert!(config_directory(|_| None).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn xdg_directory_must_be_absolute() {
        assert_eq!(
            config_directory(|name| (name == "XDG_CONFIG_HOME").then(|| "/custom/config".into()))
                .unwrap(),
            PathBuf::from("/custom/config/OpenSesame")
        );
        for value in ["", "relative", "/custom/config"] {
            let path = config_directory(|name| match name {
                "HOME" => Some("/home/test".into()),
                "XDG_CONFIG_HOME" => Some(value.into()),
                _ => None,
            })
            .unwrap();
            let expected = if value.starts_with('/') {
                "/custom/config/OpenSesame"
            } else {
                "/home/test/.config/OpenSesame"
            };
            assert_eq!(path, PathBuf::from(expected));
        }
    }

    #[test]
    fn saves_replaces_and_preserves_invalid_files() {
        let folder = std::env::temp_dir().join(format!("opensesame-test-{}", new_id()));
        let store = ConfigStore {
            path: folder.join("config.json"),
        };
        let mut config = Config::default();
        store.save(&config).unwrap();
        config.settings.auto_knock = true;
        store.save(&config).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&store.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(store.load().unwrap(), config);
        fs::write(&store.path, "{invalid").unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read_to_string(&store.path).unwrap(), "{invalid");
        fs::remove_file(&store.path).unwrap();
        fs::remove_dir(&folder).unwrap();
    }
}
