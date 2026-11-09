#![windows_subsystem = "windows"]
#![feature(sync_nonpoison, nonpoison_mutex)]

mod config;
mod logging;
mod tray;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::nonpoison::Mutex;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chat::{Chat, ChatState};
use music::{Music, MusicState};
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    music: Arc<MusicState>,
    chat: Arc<ChatState>,
    port: u16,
    started: Instant,
    shutdown: CancellationToken,
}

#[cfg(target_os = "windows")]
fn attach_parent_console() {
    // NOTE: the binary is built with windows_subsystem (no console window), but
    // when started from a terminal we still want log output there
    unsafe {
        let _ = windows::Win32::System::Console::AttachConsole(
            windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_parent_console() {}

#[tokio::main]
async fn main() {
    attach_parent_console();
    logging::init();
    log::info!(
        "stream-tools v{} starting (pid {})",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );
    if let Ok(exe) = std::env::current_exe() {
        log::debug!("executable: {}", exe.display());
    }

    let args: Vec<String> = std::env::args().collect();
    let loaded = match config::load() {
        Ok(loaded) => loaded,
        Err(e) => {
            log::error!("cannot start: {e}");
            eprintln!("stream-tools cannot start.");
            eprintln!("config file: {}", e.path.display());
            eprintln!("reason: {}", e.reason);
            eprintln!(
                "the file was left untouched; fix it or delete it to start with the default config"
            );
            let backup = config::backup_path_of_config();
            if backup.exists() {
                eprintln!(
                    "a backup of the previous config may be at {}",
                    backup.display()
                );
            }
            std::process::exit(2);
        }
    };
    for warning in &loaded.warnings {
        log::warn!("config: {warning}");
    }

    let settings = loaded.config;
    let port = match cli_port(&args) {
        Ok(Some(port)) => {
            if port != settings.port {
                log::info!(
                    "port {port} requested on the command line, overriding {}",
                    settings.port
                );
            }
            port
        }
        Ok(None) => settings.port,
        Err(e) => {
            log::error!("{e}");
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let shared = Arc::new(Mutex::new(settings));
    let shutdown = CancellationToken::new();

    let music_hook: music::ChangeHook = {
        let shared = shared.clone();
        Arc::new(move |music_config: music::MusicConfig| {
            let mut config = shared.lock();
            config.music = music_config;
            if let Err(e) = config::save(&config) {
                log::error!("cannot save the config: {e}");
            }
        })
    };
    let chat_hook: chat::ChangeHook = {
        let shared = shared.clone();
        Arc::new(move |chat_config: chat::ChatConfig| {
            let mut config = shared.lock();
            config.chat = chat_config;
            if let Err(e) = config::save(&config) {
                log::error!("cannot save the config: {e}");
            }
        })
    };

    let music_config = shared.lock().music.clone();
    let chat_config = shared.lock().chat.clone();

    let music = match Music::start(music_config, music_hook) {
        Ok(music) => music,
        Err(e) => {
            log::error!("cannot start the music player: {e}");
            std::process::exit(1);
        }
    };
    let music_state = music.state();
    let commands = shared.lock().commands.clone();
    let now_playing: chat::NowPlayingSource = {
        let music = music_state.clone();
        Arc::new(move || music.current_track())
    };
    let chat = match Chat::start(
        chat_config,
        commands,
        now_playing,
        port,
        shutdown.clone(),
        chat_hook,
    )
    .await
    {
        Ok(chat) => chat,
        Err(e) => {
            log::error!("cannot start Twitch chat: {e}");
            std::process::exit(1);
        }
    };

    let state = AppState {
        music: music_state,
        chat: chat.state(),
        port,
        started: Instant::now(),
        shutdown: shutdown.clone(),
    };

    if config::assets_dir().join("app/panel.html").exists() {
        log::info!(
            "serving widget assets from {}",
            config::assets_dir().display()
        );
    } else {
        log::warn!(
            "widget assets are missing: {} has no app/panel.html",
            config::assets_dir().display()
        );
    }

    tray::run(port, shutdown.clone());

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/api/health", get(health_handler))
        .route("/api/shutdown", post(shutdown_handler))
        .route("/panel", get(panel_handler))
        .route("/overlay", get(overlay_handler))
        .nest_service("/assets", ServeDir::new(config::assets_dir()))
        .with_state(state)
        .merge(music::router(music.state()))
        .merge(chat::router(chat.state()))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            log::error!("failed to bind port {port}: {e}");
            log::error!("stream-tools may already be running, or another app uses this port");
            std::process::exit(1);
        }
    };

    log::info!("listening on http://{addr}");
    log::debug!("routes: / /api/health /api/shutdown /panel /overlay /assets /music/* /chat/*");

    let server_token = shutdown.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal(server_token, true))
            .await
    });

    shutdown_signal(shutdown.clone(), false).await;
    log::info!("shutting down");

    // NOTE: stream clients (OBS, VLC) hold infinite connections; do not wait for them
    let _ = tokio::time::timeout(Duration::from_secs(3), server).await;

    let chat_config = chat.state().config.read().await.clone();
    {
        let mut config = shared.lock();
        config.music = music.snapshot();
        config.chat = chat_config;
        if let Err(e) = config::save(&config) {
            log::error!("cannot save the config: {e}");
        }
    }
    log::info!("shutdown complete");
}

fn cli_port(args: &[String]) -> Result<Option<u16>, String> {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let value = if let Some(rest) = arg.strip_prefix("--port=") {
            Some(rest.to_string())
        } else if arg == "--port" {
            Some(iter.next().cloned().ok_or("--port needs a value")?)
        } else {
            None
        };
        if let Some(value) = value {
            let port = value
                .parse::<u32>()
                .map_err(|_| format!("--port '{value}' is not a number"))?;
            if !(1024..=65535).contains(&port) {
                return Err(format!("--port {port} is out of range (1024-65535)"));
            }
            return Ok(Some(port as u16));
        }
    }
    Ok(None)
}

async fn health_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let music = state.music.snapshot();
    let chat_config = state.chat.config.read().await;
    let link = *state.chat.eventsub.read().await;
    let authenticated = state.chat.token.read().await.is_some();
    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "port": state.port,
        "pid": std::process::id(),
        "exe": exe,
        "uptime_seconds": state.started.elapsed().as_secs(),
        "music": {
            "volume": music.volume,
            "shuffle": music.shuffle,
            "current_playlist_index": music.current_playlist_index,
            "playlists": music.playlists.len(),
        },
        "chat": {
            "channel": chat_config.channel,
            "link": link,
            "authenticated": authenticated,
            "unread": state.chat.store.unread(),
        },
    }))
}

async fn shutdown_handler(State(state): State<AppState>) -> StatusCode {
    log::info!("shutdown requested via /api/shutdown");
    state.shutdown.cancel();
    StatusCode::OK
}

async fn root_handler() -> Redirect {
    Redirect::temporary("/panel")
}

async fn panel_handler() -> Response {
    page("panel.html")
}

async fn overlay_handler() -> Response {
    page("overlay.html")
}

fn page(name: &str) -> Response {
    let path = config::assets_dir().join("app").join(name);
    match std::fs::read_to_string(&path) {
        Ok(html) => ([(header::CACHE_CONTROL, "no-store")], Html(html)).into_response(),
        Err(e) => {
            log::error!("cannot read {}: {e}", path.display());
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the widget assets are missing: build the frontend first\n",
            )
                .into_response()
        }
    }
}

async fn shutdown_signal(token: CancellationToken, quiet: bool) {
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(e) = result {
                log::error!("failed to listen for Ctrl+C: {e}");
            }
            if !quiet {
                log::info!("shutdown requested via Ctrl+C");
            }
        }
        _ = token.cancelled() => {}
    }
}
