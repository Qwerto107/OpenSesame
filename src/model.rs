use crate::{i18n::Message, msg};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Tcp,
    Udp,
}

impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct KnockStep {
    pub port: u16,
    pub protocol: Protocol,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub host: String,
    pub open_sequence: Vec<KnockStep>,
    pub interval_ms: u64,
    pub sequence_timeout_secs: u64,
    pub service_port: u16,
    pub service_protocol: Protocol,
    pub check_interval_secs: u64,
    pub check_timeout_secs: u64,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            host: String::new(),
            open_sequence: vec![KnockStep {
                port: 7000,
                protocol: Protocol::Tcp,
            }],
            interval_ms: 200,
            sequence_timeout_secs: 10,
            service_port: 22,
            service_protocol: Protocol::Tcp,
            check_interval_secs: 30,
            check_timeout_secs: 3,
        }
    }
}

impl Server {
    pub fn validate(&self) -> Result<(), Message> {
        if self.id.trim().is_empty() {
            return Err("Server ID cannot be empty.".into());
        }
        if self.name.trim().is_empty() {
            return Err("Enter a display name.".into());
        }
        let host = self.host.trim();
        if host.is_empty() || host.chars().any(char::is_whitespace) || host.contains('/') {
            return Err("Enter a valid IP address or hostname, without a protocol or path.".into());
        }
        validate_sequence(&self.open_sequence)?;
        if self.service_port == 0 {
            return Err("Service port must be between 1 and 65535.".into());
        }
        if !(10..=60_000).contains(&self.interval_ms) {
            return Err("Step interval must be between 10 and 60000 ms.".into());
        }
        if !(1..=300).contains(&self.sequence_timeout_secs) {
            return Err("Sequence timeout must be between 1 and 300 s.".into());
        }
        let minimum_ms = self
            .interval_ms
            .saturating_mul(self.open_sequence.len().saturating_sub(1) as u64)
            + 25;
        if minimum_ms >= self.sequence_timeout_secs * 1000 {
            return Err(
                "The sequence exceeds its timeout. Increase the timeout or shorten the interval."
                    .into(),
            );
        }
        validate_check_times(self.check_interval_secs, self.check_timeout_secs)
    }
}

pub fn validate_sequence(sequence: &[KnockStep]) -> Result<(), Message> {
    if sequence.is_empty() {
        return Err("Add at least one knock step.".into());
    }
    if sequence.iter().any(|step| step.port == 0) {
        return Err("Knock ports must be between 1 and 65535.".into());
    }
    Ok(())
}

pub fn validate_check_times(interval: u64, timeout: u64) -> Result<(), Message> {
    if !(1..=86_400).contains(&interval) {
        return Err("Check interval must be between 1 and 86400 s.".into());
    }
    if !(1..=120).contains(&timeout) {
        return Err("Check timeout must be between 1 and 120 s.".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub font_family: String,
    pub font_size: u16,
    pub auto_knock: bool,
    pub auto_check: bool,
    pub check_all: bool,
    pub check_interval_secs: u64,
    pub check_timeout_secs: u64,
    pub appearance: Appearance,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "system".into(),
            font_family: String::new(),
            font_size: 13,
            auto_knock: false,
            auto_check: true,
            check_all: true,
            check_interval_secs: 30,
            check_timeout_secs: 3,
            appearance: Appearance::System,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub servers: Vec<Server>,
    pub current_server: Option<String>,
    pub checked_servers: Vec<String>,
    pub settings: Settings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            servers: vec![],
            current_server: None,
            checked_servers: vec![],
            settings: Settings::default(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), Message> {
        if self.version != 1 {
            return Err(
                msg!("Unsupported configuration version: {version}", "version" => self.version),
            );
        }
        if !(12..=18).contains(&self.settings.font_size) {
            return Err("Font size must be between 12 and 18 px.".into());
        }
        let mut ids = std::collections::HashSet::new();
        for server in &self.servers {
            server.validate()?;
            if !ids.insert(&server.id) {
                return Err("The configuration contains duplicate server IDs.".into());
            }
        }
        validate_check_times(
            self.settings.check_interval_secs,
            self.settings.check_timeout_secs,
        )
    }
}

#[derive(Default)]
pub struct BatchUpdate {
    pub sequence: Option<Vec<KnockStep>>,
    pub service: Option<(u16, Protocol)>,
}

pub fn apply_batch(
    config: &mut Config,
    ids: &[String],
    update: &BatchUpdate,
) -> Result<usize, Message> {
    if ids.is_empty() {
        return Err("Select at least one server.".into());
    }
    if update.sequence.is_none() && update.service.is_none() {
        return Err("Choose at least one setting to update.".into());
    }
    if let Some(sequence) = &update.sequence {
        validate_sequence(sequence)?;
    }
    if update.service.is_some_and(|service| service.0 == 0) {
        return Err("Service port must be between 1 and 65535.".into());
    }
    // 先验证完整副本，任何一台不满足原有时间约束时，均不写入部分更新。
    let mut draft = config.clone();
    let mut changed = 0;
    for server in &mut draft.servers {
        if !ids.contains(&server.id) {
            continue;
        }
        if let Some(sequence) = &update.sequence {
            server.open_sequence = sequence.clone();
        }
        if let Some((port, protocol)) = update.service {
            server.service_port = port;
            server.service_protocol = protocol;
        }
        server.validate().map_err(
            |error| msg!("{name}: {error}", "name" => server.name.clone(), "error" => error),
        )?;
        changed += 1;
    }
    if changed != ids.len() {
        return Err("The selected servers changed. Select them again.".into());
    }
    *config = draft;
    Ok(changed)
}

pub fn new_id() -> String {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}-{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_configuration_defaults_to_system_language() {
        let config: Config = serde_json::from_str(r#"{"settings":{"auto_knock":true}}"#).unwrap();
        assert_eq!(config.settings.language, "system");
        assert_eq!(config.settings.font_family, "");
        assert_eq!(config.settings.font_size, 13);
        assert!(config.checked_servers.is_empty());
        assert!(config.settings.auto_knock);
    }

    #[test]
    fn font_settings_round_trip_and_validate_size() {
        let mut config = Config::default();
        config.settings.font_family = "Example Sans".into();
        for size in [12, 13, 18] {
            config.settings.font_size = size;
            assert!(config.validate().is_ok());
            let json = serde_json::to_string(&config).unwrap();
            assert_eq!(serde_json::from_str::<Config>(&json).unwrap(), config);
        }
        for size in [0, 11, 19, u16::MAX] {
            config.settings.font_size = size;
            assert!(config.validate().is_err());
        }
    }
    fn server() -> Server {
        Server {
            name: "本地测试".into(),
            host: "127.0.0.1".into(),
            ..Server::default()
        }
    }
    #[test]
    fn batch_preserves_unselected_fields() {
        let first = server();
        let second = server();
        let mut config = Config {
            servers: vec![first.clone(), second.clone()],
            ..Config::default()
        };
        apply_batch(
            &mut config,
            std::slice::from_ref(&first.id),
            &BatchUpdate {
                service: Some((443, Protocol::Udp)),
                sequence: None,
            },
        )
        .unwrap();
        assert_eq!(config.servers[0].open_sequence, first.open_sequence);
        assert_eq!(config.servers[0].interval_ms, first.interval_ms);
        assert_eq!(config.servers[0].name, first.name);
        assert_eq!(config.servers[0].service_port, 443);
        assert_eq!(config.servers[1], second);
    }
    #[test]
    fn invalid_batch_is_atomic() {
        let mut config = Config {
            servers: vec![server()],
            ..Config::default()
        };
        let before = config.clone();
        let sequence = vec![
            KnockStep {
                port: 10,
                protocol: Protocol::Tcp
            };
            100
        ];
        assert!(
            apply_batch(
                &mut config,
                &[before.servers[0].id.clone()],
                &BatchUpdate {
                    sequence: Some(sequence),
                    service: None
                }
            )
            .is_err()
        );
        assert_eq!(config, before);
    }
    #[test]
    fn repeated_knock_ports_are_valid() {
        let mut value = server();
        value.open_sequence = vec![
            KnockStep {
                port: 22,
                protocol: Protocol::Tcp
            };
            2
        ];
        assert!(value.validate().is_ok());
    }
}
