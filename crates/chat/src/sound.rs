use std::io::Cursor;
use std::sync::Arc;
use std::sync::nonpoison::Mutex;
use std::time::{Duration, Instant};

use rodio::{Decoder, MixerDeviceSink, Player};

use crate::config;

const MIN_GAP: Duration = Duration::from_secs(2);

struct Output {
    _sink: MixerDeviceSink,
    player: Player,
}

impl Output {
    fn open() -> Result<Self, String> {
        let mut sink =
            rodio::DeviceSinkBuilder::open_default_sink().map_err(|error| error.to_string())?;
        // NOTE: rodio prints to stderr when the sink is dropped, and the backend runs without a console
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        Ok(Self {
            _sink: sink,
            player,
        })
    }
}

pub struct Sound {
    enabled: bool,
    bytes: Option<Arc<[u8]>>,
    output: Mutex<Option<Output>>,
    last: Mutex<Option<Instant>>,
    failure: Mutex<Option<String>>,
}

impl Sound {
    pub fn new(settings: &config::Sound) -> Self {
        let mut bytes = None;

        if !settings.file.is_empty() {
            let path = config::resolve(&settings.file);
            if path.is_file() {
                match std::fs::read(&path) {
                    Ok(data) => bytes = Some(Arc::from(data)),
                    Err(error) => log::warn!(
                        "cannot read sound file {}: {error}, using the system notification sound",
                        path.display()
                    ),
                }
            } else {
                log::warn!(
                    "sound file {} not found, using the system notification sound",
                    path.display()
                );
            }
        }

        Self {
            enabled: settings.enabled,
            bytes,
            output: Mutex::new(None),
            last: Mutex::new(None),
            failure: Mutex::new(None),
        }
    }

    pub fn play(&self) {
        if !self.enabled || !self.claim() {
            return;
        }

        if let Some(bytes) = &self.bytes {
            match self.play_file(bytes) {
                Ok(()) => {
                    self.forget();
                    return;
                }
                Err(error) => self.report(error),
            }
        }

        if let Err(error) = platform::play_alias() {
            log::warn!("cannot play the notification sound: {error}");
        }
    }

    fn play_file(&self, bytes: &Arc<[u8]>) -> Result<(), String> {
        let mut output = self.output.lock();

        if output.is_none() {
            *output = Some(Output::open()?);
        }

        let source = source(bytes)?;

        match output.as_ref() {
            Some(output) => {
                output.player.append(source);
                Ok(())
            }
            None => Err("the audio output is not available".to_string()),
        }
    }

    fn report(&self, error: String) {
        let mut failure = self.failure.lock();
        if failure.as_deref() == Some(error.as_str()) {
            return;
        }
        log::warn!("cannot play the sound file, using the system notification sound: {error}");
        *failure = Some(error);
    }

    fn forget(&self) {
        *self.failure.lock() = None;
    }

    fn claim(&self) -> bool {
        let now = Instant::now();
        let mut last = self.last.lock();
        if !gap_elapsed(*last, now) {
            return false;
        }
        *last = Some(now);
        true
    }
}

fn source(bytes: &Arc<[u8]>) -> Result<Decoder<Cursor<Arc<[u8]>>>, String> {
    Decoder::new(Cursor::new(Arc::clone(bytes))).map_err(|error| error.to_string())
}

fn gap_elapsed(last: Option<Instant>, now: Instant) -> bool {
    match last {
        None => true,
        Some(previous) => now.saturating_duration_since(previous) >= MIN_GAP,
    }
}

#[cfg(windows)]
mod platform {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
    use windows::core::HSTRING;

    const SYSTEM_SOUND: &str = "SystemNotification";

    pub fn play_alias() -> Result<(), String> {
        let played = unsafe {
            PlaySoundW(
                &HSTRING::from(SYSTEM_SOUND),
                None,
                SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
            )
        };
        if played.as_bool() {
            Ok(())
        } else {
            Err("the system notification sound is not available".to_string())
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn play_alias() -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav() -> Vec<u8> {
        let data: [u8; 2] = [0, 0];
        let mut file = Vec::with_capacity(44 + data.len());
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        file.extend_from_slice(b"WAVEfmt ");
        file.extend_from_slice(&16u32.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&8000u32.to_le_bytes());
        file.extend_from_slice(&16000u32.to_le_bytes());
        file.extend_from_slice(&2u16.to_le_bytes());
        file.extend_from_slice(&16u16.to_le_bytes());
        file.extend_from_slice(b"data");
        file.extend_from_slice(&(data.len() as u32).to_le_bytes());
        file.extend_from_slice(&data);
        file
    }

    #[test]
    fn the_first_sound_is_not_throttled() {
        assert!(gap_elapsed(None, Instant::now()));
    }

    #[test]
    fn a_recent_sound_blocks_the_next_one() {
        let now = Instant::now();
        let recent = now
            .checked_sub(Duration::from_millis(500))
            .expect("instant");
        assert!(!gap_elapsed(Some(recent), now));
    }

    #[test]
    fn an_old_sound_lets_the_next_one_through() {
        let now = Instant::now();
        let old = now.checked_sub(MIN_GAP).expect("instant");
        assert!(gap_elapsed(Some(old), now));
    }

    #[test]
    fn a_wav_file_decodes_into_samples() {
        let bytes: Arc<[u8]> = Arc::from(wav());
        let source = source(&bytes).expect("decode");
        assert_eq!(source.count(), 1);
    }

    #[test]
    fn garbage_is_not_a_sound() {
        let bytes: Arc<[u8]> = Arc::from(vec![0u8; 64]);
        assert!(source(&bytes).is_err());
    }
}
