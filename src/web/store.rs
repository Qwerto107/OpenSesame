use crate::model::Config;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub struct Store {
    pub connection: Connection,
    pub config: Config,
    pub revision: u64,
    pub auth_salt: String,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        #[cfg(unix)]
        if path != Path::new(":memory:") {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(path)?;
        }
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > 1 {
            return Err("不支持此 SQLite 数据库版本".into());
        }
        connection.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS servers(
                 id TEXT PRIMARY KEY, position INTEGER NOT NULL, selected INTEGER NOT NULL,
                 config TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions(
                 token_hash TEXT PRIMARY KEY, expires INTEGER NOT NULL, remembered INTEGER NOT NULL,
                 credential TEXT NOT NULL);
             PRAGMA user_version=1;",
        )?;
        let settings: Option<String> = connection
            .query_row("SELECT value FROM metadata WHERE key='settings'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let current: Option<String> = connection
            .query_row("SELECT value FROM metadata WHERE key='current'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let mut config = Config {
            settings: crate::model::Settings {
                appearance: crate::model::Appearance::Light,
                ..Default::default()
            },
            ..Default::default()
        };
        if let Some(settings) = settings {
            config.settings = serde_json::from_str(&settings)?;
        }
        config.current_server = current.filter(|value| !value.is_empty());
        {
            let mut query =
                connection.prepare("SELECT config, selected FROM servers ORDER BY position")?;
            for row in query.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
            })? {
                let (json, selected) = row?;
                let server: crate::model::Server = serde_json::from_str(&json)?;
                if selected {
                    config.checked_servers.push(server.id.clone());
                }
                config.servers.push(server);
            }
        }
        config.validate()?;
        let revision = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='revision'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|value| value.parse())
            .transpose()?
            .unwrap_or(0);
        let auth_salt = connection
            .query_row(
                "SELECT value FROM metadata WHERE key='auth_salt'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(super::auth::salt);
        connection.execute(
            "INSERT OR IGNORE INTO metadata(key,value) VALUES('auth_salt',?1)",
            [&auth_salt],
        )?;
        Ok(Self {
            connection,
            config,
            revision,
            auth_salt,
        })
    }

    pub fn save(&mut self, config: Config) -> Result<(), super::ApiError> {
        config.validate().map_err(super::ApiError::validation)?;
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM servers", [])?;
        for (index, server) in config.servers.iter().enumerate() {
            transaction.execute(
                "INSERT INTO servers(id, position, selected, config) VALUES(?1, ?2, ?3, ?4)",
                params![
                    server.id,
                    index as i64,
                    config.checked_servers.contains(&server.id),
                    serde_json::to_string(server)?
                ],
            )?;
        }
        for (key, value) in [
            ("settings", serde_json::to_string(&config.settings)?),
            ("revision", (self.revision + 1).to_string()),
            ("current", config.current_server.clone().unwrap_or_default()),
        ] {
            transaction.execute(
                "INSERT OR REPLACE INTO metadata(key,value) VALUES(?1,?2)",
                params![key, value],
            )?;
        }
        // 完整事务落盘后才替换内存状态，失败时保留原清单与勾选状态。
        transaction.commit()?;
        self.config = config;
        self.revision += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Server;

    #[test]
    fn sqlite_persists_selection_and_rejects_invalid_updates() {
        let path =
            std::env::temp_dir().join(format!("opensesame-{}.sqlite", crate::model::new_id()));
        {
            let mut store = Store::open(&path).unwrap();
            assert_eq!(
                store.config.settings.appearance,
                crate::model::Appearance::Light
            );
            let server = Server {
                name: "测试服务器".into(),
                host: "127.0.0.1".into(),
                ..Server::default()
            };
            let mut config = store.config.clone();
            config.checked_servers.push(server.id.clone());
            config.servers.push(server);
            store.save(config.clone()).unwrap();
            let mut invalid = config.clone();
            invalid.servers[0].service_port = 0;
            assert!(store.save(invalid).is_err());
            assert_eq!(store.config, config);
            store.connection.execute_batch("CREATE TEMP TRIGGER reject_server BEFORE INSERT ON servers BEGIN SELECT RAISE(FAIL, 'test write failure'); END;").unwrap();
            assert!(store.save(config.clone()).is_err());
            assert_eq!(store.config, config);
            assert_eq!(
                store
                    .connection
                    .query_row("SELECT count(*) FROM servers", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.config.servers.len(), 1);
        assert_eq!(reopened.revision, 1);
        assert_eq!(
            reopened.config.checked_servers,
            vec![reopened.config.servers[0].id.clone()]
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
}
