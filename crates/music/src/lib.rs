#![feature(sync_nonpoison, nonpoison_mutex, nonpoison_rwlock)]

mod api;
mod audio;
mod config;
mod player;
mod pump;
mod radio;
mod streamer;

pub use api::{ChangeHook, Music, MusicState, router};
pub use config::{KIND_LOCAL_FOLDER, KIND_NETWORK_RADIO, MusicConfig, Playlist};
