use std::path::Path as FsPath;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::nonpoison::RwLock;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::{broadcast, mpsc, watch};

use crate::audio;
use crate::config::{KIND_LOCAL_FOLDER, KIND_NETWORK_RADIO, MusicConfig, Playlist};
use crate::player;
use crate::streamer;

pub type ChangeHook = Arc<dyn Fn(MusicConfig) + Send + Sync>;

pub struct MusicState {
    audio: Arc<streamer::SharedAudio>,
    player: player::Shared,
    playlists: player::SharedPlaylists,
    shuffle: player::SharedShuffle,
    cmd_tx: mpsc::Sender<player::Command>,
    on_change: ChangeHook,
}

impl MusicState {
    pub fn snapshot(&self) -> MusicConfig {
        let (volume, index) = {
            let s = self.player.read();
            (s.volume, s.playlist_index)
        };
        MusicConfig {
            volume,
            shuffle: self.shuffle.load(Ordering::Relaxed),
            current_playlist_index: index,
            playlists: self.playlists.read().clone(),
        }
    }

    fn persist(&self) {
        (self.on_change)(self.snapshot());
    }
}

pub struct Music {
    state: Arc<MusicState>,
}

impl Music {
    pub fn start(config: MusicConfig, on_change: ChangeHook) -> Result<Self, String> {
        let queue = initial_queue(&config);
        let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<f32>>(128);
        let (epoch_tx, epoch_rx) = watch::channel(0u64);
        let playlists: player::SharedPlaylists = Arc::new(RwLock::new(config.playlists.clone()));
        let shuffle: player::SharedShuffle =
            Arc::new(std::sync::atomic::AtomicBool::new(config.shuffle));
        let (player_state, cmd_tx) = player::spawn(
            playlists.clone(),
            config.current_playlist_index,
            queue,
            config.volume,
            shuffle.clone(),
            pcm_tx,
            epoch_tx,
        );
        let audio = streamer::start_encoder(pcm_rx, player_state.clone(), epoch_rx)
            .map_err(|e| format!("failed to start audio broadcast: {e}"))?;

        Ok(Self {
            state: Arc::new(MusicState {
                audio: Arc::new(audio),
                player: player_state,
                playlists,
                shuffle,
                cmd_tx,
                on_change,
            }),
        })
    }

    pub fn state(&self) -> Arc<MusicState> {
        self.state.clone()
    }

    pub fn snapshot(&self) -> MusicConfig {
        self.state.snapshot()
    }
}

fn initial_queue(config: &MusicConfig) -> Vec<PathBuf> {
    let Some(playlist) = config.playlists.get(config.current_playlist_index) else {
        log::warn!("no playlists configured: the queue is empty");
        return Vec::new();
    };
    match playlist.kind.as_str() {
        KIND_NETWORK_RADIO => {
            log::info!(
                "start playlist[{}] '{}' is a radio station: {}",
                config.current_playlist_index,
                playlist.name,
                playlist.path
            );
            Vec::new()
        }
        KIND_LOCAL_FOLDER => {
            log::info!(
                "start playlist[{}] '{}' local folder: {}",
                config.current_playlist_index,
                playlist.name,
                playlist.path
            );
            match audio::scan_folder(FsPath::new(&playlist.path)) {
                Ok(files) => files,
                Err(e) => {
                    log::error!("failed to scan playlist folder {}: {e}", playlist.path);
                    Vec::new()
                }
            }
        }
        other => {
            log::error!(
                "unknown playlist type '{other}' for playlist[{}] '{}'",
                config.current_playlist_index,
                playlist.name
            );
            Vec::new()
        }
    }
}

pub fn router(state: Arc<MusicState>) -> Router {
    Router::new()
        .route("/music/control", post(control_handler))
        .route("/music/volume", post(volume_handler))
        .route("/music/now_playing", get(now_playing_handler))
        .route(
            "/music/playlists",
            get(list_playlists_handler).post(add_playlist_handler),
        )
        .route(
            "/music/playlists/{index}",
            patch(update_playlist_handler).delete(delete_playlist_handler),
        )
        .route("/music/stream", get(stream_handler))
        .with_state(state)
}

async fn stream_handler(State(state): State<Arc<MusicState>>) -> Response {
    log::info!(
        "stream client connected (total subscribers: {})",
        state.audio.tx.receiver_count()
    );
    let (mut rx, snapshot) = {
        let recent = state.audio.recent.read();
        let rx = state.audio.tx.subscribe();
        let snapshot: Vec<bytes::Bytes> = recent.iter().cloned().collect();
        (rx, snapshot)
    };
    let header_bytes = state.audio.header.clone();

    let body = async_stream::stream! {
        yield Ok::<bytes::Bytes, std::io::Error>(header_bytes);
        for chunk in snapshot {
            yield Ok(chunk);
        }
        loop {
            match rx.recv().await {
                Ok(chunk) => yield Ok(chunk),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("stream client lagged by {n} chunks");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
        log::info!("stream client disconnected");
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/flac")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(body))
        .expect("failed to build stream response")
}

#[derive(Deserialize)]
struct ControlRequest {
    action: String,
}

async fn control_handler(
    State(state): State<Arc<MusicState>>,
    Json(req): Json<ControlRequest>,
) -> StatusCode {
    match req.action.as_str() {
        "play" | "pause" | "stop" => {
            let status = match req.action.as_str() {
                "play" => player::Status::Playing,
                "pause" => player::Status::Paused,
                _ => player::Status::Stopped,
            };
            state.player.write().status = status;
            log::info!("control: {}", status.as_str());
            StatusCode::OK
        }
        "next_playlist" | "prev_playlist" | "next_track" | "toggle_shuffle" => {
            let cmd = match req.action.as_str() {
                "next_playlist" => player::Command::NextPlaylist,
                "prev_playlist" => player::Command::PrevPlaylist,
                "next_track" => player::Command::NextTrack,
                _ => player::Command::ToggleShuffle,
            };
            match state.cmd_tx.try_send(cmd) {
                Ok(()) => {
                    log::info!("control: {}", req.action);
                    StatusCode::OK
                }
                Err(e) => {
                    log::error!("failed to queue {}: {e}", req.action);
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        }
        other => {
            log::warn!("control: unknown action '{other}'");
            StatusCode::BAD_REQUEST
        }
    }
}

#[derive(Deserialize)]
struct VolumeRequest {
    volume: f32,
}

async fn volume_handler(
    State(state): State<Arc<MusicState>>,
    Json(req): Json<VolumeRequest>,
) -> StatusCode {
    if !(0.0..=1.0).contains(&req.volume) {
        log::warn!("volume rejected: {} is out of range", req.volume);
        return StatusCode::BAD_REQUEST;
    }
    state.player.write().volume = req.volume;
    state.persist();
    log::info!("volume: {:.2}", req.volume);
    StatusCode::OK
}

async fn now_playing_handler(State(state): State<Arc<MusicState>>) -> Json<serde_json::Value> {
    let (status, track, volume, index) = {
        let s = state.player.read();
        (
            s.status,
            s.current_track.clone(),
            s.volume,
            s.playlist_index,
        )
    };
    let (playlist, playlist_type) = {
        let playlists = state.playlists.read();
        match playlists.get(index) {
            Some(pl) => (Some(pl.name.clone()), Some(pl.kind.clone())),
            None => (None, None),
        }
    };
    Json(serde_json::json!({
        "status": status.as_str(),
        "track": track,
        "volume": volume,
        "playlist_index": index,
        "playlist": playlist,
        "playlist_type": playlist_type,
        "shuffle": state.shuffle.load(Ordering::Relaxed),
    }))
}

async fn list_playlists_handler(State(state): State<Arc<MusicState>>) -> Json<serde_json::Value> {
    let playlists = state.playlists.read();
    let current = state.player.read().playlist_index;
    Json(serde_json::json!({
        "current": current,
        "playlists": *playlists,
    }))
}

#[derive(Deserialize)]
struct AddPlaylistRequest {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    path: String,
}

fn validate_playlist(kind: &str, path: &str) -> Result<(), String> {
    match kind {
        KIND_LOCAL_FOLDER => match audio::scan_folder(FsPath::new(path)) {
            Ok(files) if files.is_empty() => Err(format!("no audio files in '{path}'")),
            Ok(_) => Ok(()),
            Err(e) => Err(format!("cannot scan '{path}': {e}")),
        },
        KIND_NETWORK_RADIO => {
            if path.starts_with("http://") || path.starts_with("https://") {
                Ok(())
            } else {
                Err(format!("'{path}' is not an http(s) URL"))
            }
        }
        other => Err(format!("unknown type '{other}'")),
    }
}

async fn add_playlist_handler(
    State(state): State<Arc<MusicState>>,
    Json(req): Json<AddPlaylistRequest>,
) -> StatusCode {
    let name = req.name.trim();
    let path = req.path.trim();
    if name.is_empty() || path.is_empty() {
        log::warn!("add playlist rejected: empty name or path");
        return StatusCode::BAD_REQUEST;
    }

    if let Err(e) = validate_playlist(&req.kind, path) {
        log::warn!("add playlist rejected: {e}");
        return StatusCode::BAD_REQUEST;
    }

    {
        let mut playlists = state.playlists.write();
        playlists.push(Playlist {
            name: name.to_string(),
            kind: req.kind,
            path: path.to_string(),
        });
        log::info!(
            "added playlist '{}' at index {} (total playlists: {})",
            name,
            playlists.len() - 1,
            playlists.len()
        );
    }
    state.persist();
    StatusCode::OK
}

#[derive(Deserialize)]
struct UpdatePlaylistRequest {
    name: Option<String>,
    path: Option<String>,
}

async fn update_playlist_handler(
    State(state): State<Arc<MusicState>>,
    Path(index): Path<usize>,
    Json(req): Json<UpdatePlaylistRequest>,
) -> StatusCode {
    let updated = {
        let mut playlists = state.playlists.write();
        let Some(playlist) = playlists.get(index).cloned() else {
            log::warn!("update playlist rejected: index {index} does not exist");
            return StatusCode::NOT_FOUND;
        };

        let name = match req.name.as_deref() {
            Some(raw) => raw.trim().to_string(),
            None => playlist.name.clone(),
        };
        let path = match req.path.as_deref() {
            Some(raw) => raw.trim().to_string(),
            None => playlist.path.clone(),
        };
        if name.is_empty() || path.is_empty() {
            log::warn!("update playlist rejected: empty name or path");
            return StatusCode::BAD_REQUEST;
        }

        let path_changed = path != playlist.path;
        if path_changed && let Err(e) = validate_playlist(&playlist.kind, &path) {
            log::warn!("update playlist rejected: {e}");
            return StatusCode::BAD_REQUEST;
        }

        let updated = Playlist {
            name,
            kind: playlist.kind,
            path,
        };
        playlists[index] = updated.clone();
        (updated, path_changed)
    };

    let (updated, path_changed) = updated;
    let current = state.player.read().playlist_index;
    if path_changed
        && index == current
        && state
            .cmd_tx
            .try_send(player::Command::SwitchTo(index))
            .is_err()
    {
        log::error!("failed to queue the reload of playlist index {index}");
    }
    log::info!(
        "updated playlist index {index} ('{}'): {}",
        updated.name,
        updated.path
    );
    state.persist();
    StatusCode::OK
}

async fn delete_playlist_handler(
    State(state): State<Arc<MusicState>>,
    Path(index): Path<usize>,
) -> StatusCode {
    let removed = {
        let mut playlists = state.playlists.write();
        if index >= playlists.len() {
            log::warn!("delete playlist rejected: index {index} does not exist");
            return StatusCode::NOT_FOUND;
        }
        if playlists.len() == 1 {
            log::warn!("delete playlist rejected: at least one playlist must remain");
            return StatusCode::CONFLICT;
        }
        playlists.remove(index)
    };
    let remaining = state.playlists.read().len();

    let current = state.player.read().playlist_index;
    if index < current {
        state.player.write().playlist_index = current - 1;
    } else if index == current {
        let next = index.min(remaining - 1);
        state.player.write().playlist_index = next;
        if state
            .cmd_tx
            .try_send(player::Command::SwitchTo(next))
            .is_err()
        {
            log::error!("failed to queue the switch to playlist index {next}");
        }
    }
    log::info!("removed playlist '{}' at index {index}", removed.name);
    state.persist();
    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use http_body_util::BodyExt;
    use std::sync::nonpoison::Mutex;
    use std::time::Duration;
    use tower::ServiceExt;

    fn start(config: MusicConfig) -> (Music, Arc<Mutex<Vec<MusicConfig>>>) {
        let saved = Arc::new(Mutex::new(Vec::new()));
        let sink = saved.clone();
        let music = Music::start(config, Arc::new(move |config| sink.lock().push(config)))
            .expect("music must start");
        (music, saved)
    }

    fn json_request(method: &str, uri: &str, body: &str) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("request")
    }

    #[tokio::test]
    async fn volume_update_is_saved_and_reported() {
        let (music, saved) = start(MusicConfig::default());
        let router = router(music.state());

        let response = router
            .clone()
            .oneshot(json_request("POST", "/music/volume", r#"{"volume":0.35}"#))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);

        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .uri("/music/now_playing")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert!((value["volume"].as_f64().expect("volume") - 0.35).abs() < 1e-6);

        let saved = saved.lock();
        assert_eq!(saved.last().expect("saved config").volume, 0.35);
    }

    #[tokio::test]
    async fn stream_serves_a_flac_header() {
        let (music, _saved) = start(MusicConfig::default());
        let response = router(music.state())
            .oneshot(
                axum::http::Request::builder()
                    .uri("/music/stream")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "audio/flac");

        let body = response.into_body();
        let mut frames = body.into_data_stream();
        let frame = tokio::time::timeout(Duration::from_secs(2), frames.next())
            .await
            .expect("a frame must arrive")
            .expect("stream must have a frame")
            .expect("data frame");
        assert!(frame.starts_with(b"fLaC"));
    }

    #[tokio::test]
    async fn unknown_playlist_kind_is_rejected() {
        let (music, _saved) = start(MusicConfig::default());
        let response = router(music.state())
            .oneshot(json_request(
                "POST",
                "/music/playlists",
                r#"{"name":"x","type":"weird","path":"http://example.com"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn deleting_the_last_playlist_is_rejected() {
        let config = MusicConfig {
            playlists: vec![Playlist {
                name: "a".to_string(),
                kind: KIND_NETWORK_RADIO.to_string(),
                path: "http://example.com/stream".to_string(),
            }],
            ..MusicConfig::default()
        };
        let (music, _saved) = start(config);
        let response = router(music.state())
            .oneshot(
                axum::http::Request::builder()
                    .method("DELETE")
                    .uri("/music/playlists/0")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
}
