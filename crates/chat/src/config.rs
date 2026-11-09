use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MIN_MESSAGES: usize = 1;
const MAX_MESSAGES: usize = 50;
const MAX_FADE_SECONDS: u32 = 3600;
const MIN_FONT_SIZE: u32 = 10;
const MAX_FONT_SIZE: u32 = 72;
const MAX_CHANNEL_LEN: usize = 25;

pub fn normalize_channel(raw: &str) -> Option<String> {
    let channel = raw.trim().trim_start_matches('#').to_ascii_lowercase();
    let valid = !channel.is_empty()
        && channel.len() <= MAX_CHANNEL_LEN
        && channel
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then_some(channel)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Widget {
    pub max_messages: usize,
    pub fade_seconds: u32,
    pub font_size: u32,
    pub show_badges: bool,
}

impl Default for Widget {
    fn default() -> Self {
        Self {
            max_messages: 12,
            fade_seconds: 0,
            font_size: 22,
            show_badges: false,
        }
    }
}

// NOTE: tokens sit in the config because the backend runs detached from VS Code
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Sound {
    pub enabled: bool,
    pub file: String,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            enabled: true,
            file: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_at: u64,
    pub login: String,
    pub user_id: String,
}

impl Tokens {
    pub fn is_authenticated(&self) -> bool {
        !self.access.is_empty()
    }
}

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

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct ChatConfig {
    pub channel: String,
    pub client_id: String,
    pub client_secret: String,
    pub tokens: Tokens,
    pub widget: Widget,
    pub sound: Sound,
}

impl ChatConfig {
    pub fn normalized(mut self) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();

        let raw_channel = self.channel.clone();
        match normalize_channel(&raw_channel) {
            Some(channel) => self.channel = channel,
            None => {
                if !raw_channel.trim().is_empty() {
                    warnings.push(format!("channel '{raw_channel}' is not a Twitch login"));
                }
                self.channel = String::new();
            }
        }

        let widget = &mut self.widget;
        let max_messages = widget.max_messages;
        widget.max_messages = max_messages.clamp(MIN_MESSAGES, MAX_MESSAGES);
        if widget.max_messages != max_messages {
            warnings.push(format!(
                "widget.max_messages {max_messages} is out of range, using {}",
                widget.max_messages
            ));
        }

        let fade = widget.fade_seconds;
        widget.fade_seconds = fade.min(MAX_FADE_SECONDS);
        if widget.fade_seconds != fade {
            warnings.push(format!(
                "widget.fade_seconds {fade} is too large, using {}",
                widget.fade_seconds
            ));
        }

        let font = widget.font_size;
        widget.font_size = font.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if widget.font_size != font {
            warnings.push(format!(
                "widget.font_size {font} is out of range, using {}",
                widget.font_size
            ));
        }

        let raw_file = self.sound.file.clone();
        self.sound.file = raw_file.trim().to_string();

        (self, warnings)
    }
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn resolve(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        exe_dir().join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalized(config: ChatConfig) -> ChatConfig {
        config.normalized().0
    }

    #[test]
    fn channel_is_lowercased_and_stripped() {
        let config = ChatConfig {
            channel: "  #Ku6epXBOCTuK ".to_string(),
            ..ChatConfig::default()
        };
        assert_eq!(normalized(config).channel, "ku6epxboctuk");
    }

    #[test]
    fn channel_validation_accepts_a_leading_hash() {
        assert_eq!(normalize_channel(" #User_1 "), Some("user_1".to_string()));
        assert_eq!(normalize_channel("bad!"), None);
        assert_eq!(normalize_channel("   "), None);
        assert_eq!(normalize_channel(&"a".repeat(26)), None);
    }

    #[test]
    fn invalid_channel_is_dropped() {
        let config = ChatConfig {
            channel: "bad channel!".to_string(),
            ..ChatConfig::default()
        };
        assert_eq!(normalized(config).channel, "");
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let config = ChatConfig {
            widget: Widget {
                max_messages: 1000,
                fade_seconds: 99_999,
                font_size: 2,
                show_badges: true,
            },
            ..ChatConfig::default()
        };
        let config = normalized(config);
        assert_eq!(config.widget.max_messages, MAX_MESSAGES);
        assert_eq!(config.widget.fade_seconds, MAX_FADE_SECONDS);
        assert_eq!(config.widget.font_size, MIN_FONT_SIZE);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let raw = r#"{"channel":"user","surprise":true}"#;
        let config: ChatConfig = serde_json::from_str(raw).expect("parse");
        assert_eq!(config.channel, "user");
    }

    #[test]
    fn a_missing_sound_section_keeps_the_system_sound() {
        let raw = r#"{"channel":"user"}"#;
        let config: ChatConfig = serde_json::from_str(raw).expect("parse");
        assert!(config.sound.enabled);
        assert!(config.sound.file.is_empty());
    }

    #[test]
    fn the_sound_file_is_trimmed() {
        let config = ChatConfig {
            sound: Sound {
                enabled: true,
                file: "  beep.mp3  ".to_string(),
            },
            ..ChatConfig::default()
        };
        assert_eq!(normalized(config).sound.file, "beep.mp3");
    }
}
