use serde::{Deserialize, Serialize};

pub const DEFAULT_VOLUME: f32 = 0.8;
pub const KIND_LOCAL_FOLDER: &str = "local_folder";
pub const KIND_NETWORK_RADIO: &str = "network_radio";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Playlist {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MusicConfig {
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub current_playlist_index: usize,
    #[serde(default)]
    pub playlists: Vec<Playlist>,
}

fn default_volume() -> f32 {
    DEFAULT_VOLUME
}

impl Default for MusicConfig {
    fn default() -> Self {
        Self {
            volume: DEFAULT_VOLUME,
            shuffle: false,
            current_playlist_index: 0,
            playlists: Vec::new(),
        }
    }
}

impl MusicConfig {
    pub fn same_content(&self, other: &Self) -> bool {
        self.volume == other.volume
            && self.shuffle == other.shuffle
            && self.current_playlist_index == other.current_playlist_index
            && self.playlists == other.playlists
    }

    pub fn sanitize(&mut self, warnings: &mut Vec<String>) {
        if !self.volume.is_finite() {
            warnings.push("volume is not a number, using the default".to_string());
            self.volume = DEFAULT_VOLUME;
        } else if !(0.0..=1.0).contains(&self.volume) {
            let clamped = self.volume.clamp(0.0, 1.0);
            warnings.push(format!(
                "volume {} is out of range, using {clamped}",
                self.volume
            ));
            self.volume = clamped;
        }

        if self.playlists.is_empty() {
            self.current_playlist_index = 0;
        } else if self.current_playlist_index >= self.playlists.len() {
            let last = self.playlists.len() - 1;
            warnings.push(format!(
                "current_playlist_index {} is out of range, using {last}",
                self.current_playlist_index
            ));
            self.current_playlist_index = last;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        let mut config = MusicConfig::default();
        let mut warnings = Vec::new();
        config.sanitize(&mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(config.volume, DEFAULT_VOLUME);
        assert_eq!(config.current_playlist_index, 0);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let mut config = MusicConfig {
            volume: 4.0,
            current_playlist_index: 9,
            playlists: vec![Playlist {
                name: "a".to_string(),
                kind: KIND_LOCAL_FOLDER.to_string(),
                path: "testdata".to_string(),
            }],
            ..MusicConfig::default()
        };
        let mut warnings = Vec::new();
        config.sanitize(&mut warnings);
        assert_eq!(config.volume, 1.0);
        assert_eq!(config.current_playlist_index, 0);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn nan_volume_falls_back_to_default() {
        let mut config = MusicConfig {
            volume: f32::NAN,
            ..MusicConfig::default()
        };
        let mut warnings = Vec::new();
        config.sanitize(&mut warnings);
        assert_eq!(config.volume, DEFAULT_VOLUME);
        assert_eq!(warnings.len(), 1);
    }
}
