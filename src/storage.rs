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
        let root = std::env::var_os("APPDATA").ok_or("Windows AppData was not found.")?;
        Ok(Self {
            path: PathBuf::from(root).join("OpenSesame").join("config.json"),
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
        fs::create_dir_all(parent).map_err(|error| msg!("Could not create the configuration directory: {error}", "error" => error.to_string()))?;
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
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
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

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(store.load().unwrap(), config);
        fs::write(&store.path, "{invalid").unwrap();
        assert!(store.load().is_err());
        assert_eq!(fs::read_to_string(&store.path).unwrap(), "{invalid");
        fs::remove_file(&store.path).unwrap();
        fs::remove_dir(&folder).unwrap();
    }
}
