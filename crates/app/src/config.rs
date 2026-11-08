use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

use chat::ChatConfig;
use music::{KIND_LOCAL_FOLDER, KIND_NETWORK_RADIO, MusicConfig, Playlist};

pub const CONFIG_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 45890;
const MIN_PORT: u32 = 1024;
const MAX_PORT: u32 = 65535;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct TrackCommand {
    pub enabled: bool,
    pub aliases: Vec<String>,
    pub template: String,
    pub cooldown_seconds: u64,
}

impl Default for TrackCommand {
    fn default() -> Self {
        Self {
            enabled: true,
            aliases: vec!["!трек".to_string(), "!track".to_string()],
            template: "Сейчас играет: {track}".to_string(),
            cooldown_seconds: 10,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Commands {
    pub track: TrackCommand,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Config {
    pub version: u32,
    pub port: u16,
    pub music: MusicConfig,
    pub chat: ChatConfig,
    pub commands: Commands,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            port: DEFAULT_PORT,
            music: MusicConfig::default(),
            chat: ChatConfig::default(),
            commands: Commands::default(),
        }
    }
}

#[derive(Debug)]
pub struct ConfigError {
    pub path: PathBuf,
    pub reason: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.reason)
    }
}

#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    pub warnings: Vec<String>,
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    exe_dir().join("config.json")
}

pub fn assets_dir() -> PathBuf {
    exe_dir().join("assets")
}

pub fn backup_path_of_config() -> PathBuf {
    backup_path(&config_path())
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_file_name(format!("{}.bak", file_name_of(path)))
}

fn second_backup_path(path: &Path) -> PathBuf {
    path.with_file_name(format!("{}.bak.2", file_name_of(path)))
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.json".to_string())
}

type Migration = fn(&mut Value) -> Result<(), String>;

const MIGRATIONS: &[(u32, Migration)] = &[];

pub fn load() -> Result<Loaded, ConfigError> {
    let path = config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            log::info!("no config at {} ({e}), creating defaults", path.display());
            let config = Config::default();
            if let Err(e) = save(&config) {
                return Err(ConfigError {
                    path,
                    reason: format!("cannot write the default config: {e}"),
                });
            }
            return Ok(Loaded {
                config,
                warnings: Vec::new(),
            });
        }
        Err(e) => {
            return Err(ConfigError {
                path,
                reason: format!("cannot read the config file: {e}"),
            });
        }
    };

    parse(&text, &path)
}

fn parse(text: &str, path: &Path) -> Result<Loaded, ConfigError> {
    let mut value: Value = serde_json::from_str(text).map_err(|e| ConfigError {
        path: path.to_path_buf(),
        reason: format!("the config file is not valid JSON: {e}"),
    })?;

    migrate(&mut value).map_err(|reason| ConfigError {
        path: path.to_path_buf(),
        reason,
    })?;

    let mut warnings = Vec::new();
    let mut music = parse_music(value.get("music"), &mut warnings);
    music.sanitize(&mut warnings);
    let (chat, chat_warnings) = parse_chat(value.get("chat"), &mut warnings).normalized();
    warnings.extend(chat_warnings);
    let config = Config {
        version: CONFIG_VERSION,
        port: parse_port(value.get("port"), &mut warnings),
        music,
        chat,
        commands: parse_commands(value.get("commands"), &mut warnings),
    };

    Ok(Loaded { config, warnings })
}

fn migrate(value: &mut Value) -> Result<(), String> {
    if !value.is_object() {
        return Err("the config file must contain a JSON object".to_string());
    }
    let version = match value.get("version") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
        Some(Value::Null) | None => CONFIG_VERSION,
        Some(_) => return Err("'version' must be a number".to_string()),
    };
    if version > CONFIG_VERSION {
        return Err(format!(
            "the config file was written by a newer version (config version {version}, supported {CONFIG_VERSION})"
        ));
    }
    let mut current = version;
    for (from, migration) in MIGRATIONS {
        if *from == current {
            migration(value)
                .map_err(|e| format!("config migration {from} -> {} failed: {e}", from + 1))?;
            current += 1;
        }
    }
    if current != version {
        value["version"] = Value::from(current);
    }
    Ok(())
}

fn parse_port(value: Option<&Value>, warnings: &mut Vec<String>) -> u16 {
    match value {
        None | Some(Value::Null) => DEFAULT_PORT,
        Some(Value::Number(n)) => {
            let raw = n.as_u64().unwrap_or(0);
            if !(MIN_PORT as u64..=MAX_PORT as u64).contains(&raw) {
                let clamped = raw.clamp(MIN_PORT as u64, MAX_PORT as u64) as u16;
                warnings.push(format!("port {raw} is out of range, using {clamped}"));
                clamped
            } else {
                raw as u16
            }
        }
        Some(_) => {
            warnings.push(format!("port is not a number, using {DEFAULT_PORT}"));
            DEFAULT_PORT
        }
    }
}

fn parse_music(value: Option<&Value>, warnings: &mut Vec<String>) -> MusicConfig {
    let Some(value) = value else {
        return MusicConfig::default();
    };
    if !value.is_object() {
        warnings.push("music is not an object, using defaults".to_string());
        return MusicConfig::default();
    }

    let mut config = MusicConfig::default();

    config.volume = match value.get("volume") {
        None | Some(Value::Null) => config.volume,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN) as f32,
        Some(_) => {
            warnings.push("music.volume is not a number, using the default".to_string());
            config.volume
        }
    };

    config.shuffle = match value.get("shuffle") {
        Some(Value::Bool(b)) => *b,
        None | Some(Value::Null) => false,
        Some(_) => {
            warnings.push("music.shuffle is not a boolean, using the default".to_string());
            false
        }
    };

    config.playlists = match value.get("playlists") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => {
            let mut playlists = Vec::new();
            for (i, item) in items.iter().enumerate() {
                match parse_playlist(item) {
                    Ok(playlist) => playlists.push(playlist),
                    Err(reason) => {
                        warnings.push(format!("playlist[{i}] is invalid ({reason}), skipped"))
                    }
                }
            }
            playlists
        }
        Some(_) => {
            warnings.push("music.playlists is not an array, using an empty list".to_string());
            Vec::new()
        }
    };

    config.current_playlist_index = match value.get("current_playlist_index") {
        None | Some(Value::Null) => 0,
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as usize,
        Some(_) => {
            warnings.push("music.current_playlist_index is not a number, using 0".to_string());
            0
        }
    };

    config
}

fn parse_playlist(item: &Value) -> Result<Playlist, String> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .ok_or("'name' is missing or not a string")?
        .trim()
        .to_string();
    let kind = item
        .get("type")
        .and_then(Value::as_str)
        .ok_or("'type' is missing or not a string")?
        .trim()
        .to_string();
    let path = item
        .get("path")
        .and_then(Value::as_str)
        .ok_or("'path' is missing or not a string")?
        .trim()
        .to_string();
    let playlist = Playlist { name, kind, path };
    if playlist.kind != KIND_LOCAL_FOLDER && playlist.kind != KIND_NETWORK_RADIO {
        return Err(format!("unknown playlist type '{}'", playlist.kind));
    }
    if playlist.name.is_empty() {
        return Err("'name' is empty".to_string());
    }
    if playlist.path.is_empty() {
        return Err("'path' is empty".to_string());
    }
    Ok(playlist)
}

fn parse_chat(value: Option<&Value>, warnings: &mut Vec<String>) -> ChatConfig {
    let Some(value) = value else {
        return ChatConfig::default();
    };
    match serde_json::from_value::<ChatConfig>(value.clone()) {
        Ok(config) => config,
        Err(e) => {
            warnings.push(format!("chat section is invalid ({e}), using defaults"));
            ChatConfig::default()
        }
    }
}

fn parse_commands(value: Option<&Value>, warnings: &mut Vec<String>) -> Commands {
    let Some(value) = value else {
        return Commands::default();
    };
    match serde_json::from_value::<Commands>(value.clone()) {
        Ok(commands) => commands,
        Err(e) => {
            warnings.push(format!("commands section is invalid ({e}), using defaults"));
            Commands::default()
        }
    }
}

pub fn save(config: &Config) -> Result<(), String> {
    let path = config_path();
    let text =
        serde_json::to_string_pretty(config).map_err(|e| format!("cannot serialize: {e}"))?;
    if let Err(e) = rotate_backup(&path) {
        log::warn!("config backup failed: {e}");
    }
    write_atomic(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn rotate_backup(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let bak = backup_path(path);
    let bak2 = second_backup_path(path);
    if bak.exists() {
        copy_atomic(&bak, &bak2)?;
    }
    copy_atomic(path, &bak)
}

fn copy_atomic(src: &Path, dst: &Path) -> Result<(), String> {
    let text = std::fs::read(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
    write_atomic(dst, &String::from_utf8_lossy(&text))
}

fn write_atomic(target: &Path, contents: &str) -> Result<(), String> {
    let tmp = target.with_file_name(format!("{}.tmp", file_name_of(target)));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)
            .map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        file.sync_all()
            .map_err(|e| format!("cannot flush {}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot replace {}: {e}", target.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_text(text: &str) -> Result<Loaded, ConfigError> {
        parse(text, Path::new("config.json"))
    }

    #[test]
    fn empty_config_gets_defaults() {
        let loaded = parse_text("{}").expect("empty config must load");
        assert_eq!(loaded.config.version, CONFIG_VERSION);
        assert_eq!(loaded.config.port, DEFAULT_PORT);
        assert_eq!(loaded.config.music.volume, 0.8);
        assert!(loaded.config.chat.channel.is_empty());
        assert_eq!(loaded.config.commands.track.aliases, ["!трек", "!track"]);
        assert!(loaded.config.commands.track.enabled);
        assert_eq!(loaded.config.commands.track.cooldown_seconds, 10);
    }

    #[test]
    fn sections_are_loaded_together() {
        let raw = r#"{
            "version": 1,
            "port": 45900,
            "music": {
                "volume": 0.5,
                "shuffle": true,
                "current_playlist_index": 1,
                "playlists": [
                    {"name": "a", "type": "local_folder", "path": "testdata"},
                    {"name": "b", "type": "network_radio", "path": "http://example.com/stream"}
                ]
            },
            "chat": {"channel": "  #Ku6epXBOCTuK ", "widget": {"max_messages": 1000}},
            "commands": {"track": {"enabled": false, "cooldown_seconds": 30}}
        }"#;
        let loaded = parse_text(raw).expect("config must load");
        assert_eq!(loaded.config.port, 45900);
        assert_eq!(loaded.config.music.volume, 0.5);
        assert_eq!(loaded.config.music.playlists.len(), 2);
        assert_eq!(loaded.config.chat.channel, "ku6epxboctuk");
        assert_eq!(loaded.config.chat.widget.max_messages, 50);
        assert!(!loaded.config.commands.track.enabled);
        assert_eq!(loaded.config.commands.track.cooldown_seconds, 30);
        assert_eq!(loaded.config.commands.track.aliases, ["!трек", "!track"]);
    }

    #[test]
    fn a_music_section_without_optional_fields_loads() {
        let raw = r#"{"music":{"volume":0.5,"playlists":[{"name":"a","type":"local_folder","path":"testdata"}]}}"#;
        let loaded = parse_text(raw).expect("config must load");
        assert_eq!(loaded.config.music.volume, 0.5);
        assert_eq!(loaded.config.music.playlists.len(), 1);
        assert!(!loaded.config.music.shuffle);
        assert!(loaded.config.chat.channel.is_empty());
        assert_eq!(loaded.config.commands.track.aliases.len(), 2);
    }

    #[test]
    fn out_of_range_port_is_clamped() {
        let loaded = parse_text(r#"{"port":99999}"#).expect("config must load");
        assert_eq!(loaded.config.port, MAX_PORT as u16);
        assert_eq!(loaded.warnings.len(), 1);
    }

    #[test]
    fn broken_playlist_is_skipped() {
        let loaded = parse_text(
            r#"{"music":{"playlists":[{"name":"a","type":"local_folder","path":"testdata"},{"name":"b","type":"weird","path":"x"},{"name":"","type":"local_folder","path":"y"}]}}"#,
        )
        .expect("config must load");
        assert_eq!(loaded.config.music.playlists.len(), 1);
        assert_eq!(loaded.config.music.playlists[0].name, "a");
        assert_eq!(loaded.warnings.len(), 2);
    }

    #[test]
    fn an_invalid_section_falls_back_to_defaults() {
        let loaded = parse_text(r#"{"chat":"nope","commands":"nope"}"#).expect("config must load");
        assert!(loaded.config.chat.channel.is_empty());
        assert!(loaded.config.commands.track.enabled);
        assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
    }

    #[test]
    fn invalid_json_is_rejected() {
        assert!(parse_text("{not json}").is_err());
    }

    #[test]
    fn a_newer_version_is_rejected() {
        let err = parse_text(r#"{"version":99}"#).expect_err("newer version must be rejected");
        assert!(err.reason.contains("newer version"));
    }

    #[test]
    fn atomic_write_creates_backups() {
        let dir = std::env::temp_dir().join(format!("stream-tools-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let target = dir.join("config.json");

        write_atomic(&target, "{\"a\":1}").expect("write");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "{\"a\":1}");

        rotate_backup(&target).expect("backup");
        assert!(backup_path(&target).exists());

        write_atomic(&target, "{\"a\":2}").expect("write");
        rotate_backup(&target).expect("backup");
        assert!(second_backup_path(&target).exists());
        assert_eq!(
            std::fs::read_to_string(backup_path(&target)).unwrap(),
            "{\"a\":2}"
        );
        assert_eq!(
            std::fs::read_to_string(second_backup_path(&target)).unwrap(),
            "{\"a\":1}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
