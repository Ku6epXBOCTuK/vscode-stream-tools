use std::sync::Arc;
use std::sync::nonpoison::Mutex as StdMutex;
use std::sync::nonpoison::MutexGuard;
use std::time::Instant;

use axum::Router;
use axum::routing::{delete, get, post};
use serde::Serialize;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio_util::sync::CancellationToken;
use tower_http::cors::CorsLayer;
use twitch_api::helix::HelixClient;
use twitch_oauth2::{UserToken, UserTokenBuilder};

use crate::api;
use crate::auth;
use crate::chat::{self, Store};
use crate::command::{Dispatcher, NowPlayingSource};
use crate::config::{ChatConfig, Commands};
use crate::eventsub;
use crate::sound::Sound;

pub type ChangeHook = Arc<dyn Fn(ChatConfig) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Link {
    Offline,
    Connecting,
    Connected,
}

#[derive(Default)]
pub struct Activity {
    pub last_event: Option<Instant>,
    pub last_message: Option<Instant>,
}

pub struct ChatState {
    pub store: Store,
    pub config: RwLock<ChatConfig>,
    pub eventsub: RwLock<Link>,
    pub token: RwLock<Option<UserToken>>,
    pub pending_login: Mutex<Vec<UserTokenBuilder>>,
    pub refresh_lock: Mutex<()>,
    pub http: reqwest::Client,
    pub helix: HelixClient<'static, reqwest::Client>,
    pub badges: RwLock<api::BadgeCache>,
    pub shutdown: CancellationToken,
    pub started: Instant,
    pub activity: StdMutex<Activity>,
    pub reconnect: Notify,
    pub sound: Sound,
    pub dispatcher: Dispatcher,
    pub port: u16,
    on_change: ChangeHook,
}

impl ChatState {
    pub fn push(&self, message: chat::ChatMessage) -> chat::ChatMessage {
        self.touch_message();
        if message.kind == chat::KIND_CHAT {
            self.sound.play();
        }
        self.store.push(message)
    }

    fn lock_activity(&self) -> MutexGuard<'_, Activity> {
        self.activity.lock()
    }

    pub fn touch_event(&self) {
        self.lock_activity().last_event = Some(Instant::now());
    }

    pub fn touch_message(&self) {
        self.lock_activity().last_message = Some(Instant::now());
    }

    pub fn since_event(&self) -> Option<u64> {
        self.lock_activity()
            .last_event
            .map(|last| last.elapsed().as_secs())
    }

    pub fn since_message(&self) -> Option<u64> {
        self.lock_activity()
            .last_message
            .map(|last| last.elapsed().as_secs())
    }

    pub fn uptime(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub async fn persist_async(&self) {
        let config = self.config.read().await.clone();
        (self.on_change)(config);
    }
}

pub struct Chat {
    state: Arc<ChatState>,
}

impl Chat {
    pub async fn start(
        config: ChatConfig,
        commands: Commands,
        now_playing: NowPlayingSource,
        port: u16,
        shutdown: CancellationToken,
        on_change: ChangeHook,
    ) -> Result<Chat, String> {
        let http = reqwest::Client::new();
        let sound = Sound::new(&config.sound);
        let dispatcher = Dispatcher::new(&commands, now_playing);
        let state = Arc::new(ChatState {
            store: Store::new(),
            config: RwLock::new(config),
            eventsub: RwLock::new(Link::Offline),
            token: RwLock::new(None),
            pending_login: Mutex::new(Vec::new()),
            refresh_lock: Mutex::new(()),
            helix: HelixClient::with_client(http.clone()),
            badges: RwLock::new(api::BadgeCache::default()),
            http,
            shutdown: shutdown.clone(),
            started: Instant::now(),
            activity: StdMutex::new(Activity::default()),
            reconnect: Notify::new(),
            sound,
            dispatcher,
            port,
            on_change,
        });

        match auth::restore(&state).await {
            Ok(true) => {}
            Ok(false) => log::info!("not signed in, Twitch chat stays empty until sign-in"),
            Err(err) => log::warn!("sign-in could not be restored: {err}"),
        }

        auth::spawn_refresher(state.clone());
        eventsub::spawn(state.clone());

        Ok(Chat { state })
    }

    pub fn state(&self) -> Arc<ChatState> {
        self.state.clone()
    }

    pub fn snapshot(&self) -> ChatConfig {
        self.state.config.blocking_read().clone()
    }
}

pub fn router(state: Arc<ChatState>) -> Router {
    Router::new()
        .route("/chat/state", get(api::info))
        .route("/chat/messages", get(api::messages).post(api::send_message))
        .route("/chat/read", post(api::mark_read))
        .route("/chat/messages/{id}", delete(api::delete_message))
        .route("/chat/moderation/timeout", post(api::timeout_user))
        .route("/chat/moderation/ban", post(api::ban_user))
        .route("/chat/channel", post(api::set_channel))
        .route("/chat/auth/url", get(auth::url))
        .route("/chat/auth/status", get(auth::status))
        .route("/chat/auth/callback", get(auth::callback))
        .route("/chat/events", get(api::events))
        .route("/chat/eventsub/reconnect", post(api::reconnect))
        .route("/chat/badges", get(api::badges))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> Arc<ChatState> {
        let http = reqwest::Client::new();
        let sound = Sound::new(&crate::config::Sound::default());
        Arc::new(ChatState {
            store: Store::new(),
            config: RwLock::new(ChatConfig::default()),
            eventsub: RwLock::new(Link::Offline),
            token: RwLock::new(None),
            pending_login: Mutex::new(Vec::new()),
            refresh_lock: Mutex::new(()),
            helix: HelixClient::with_client(http.clone()),
            badges: RwLock::new(api::BadgeCache::default()),
            http,
            shutdown: CancellationToken::new(),
            started: Instant::now(),
            activity: StdMutex::new(Activity::default()),
            reconnect: Notify::new(),
            sound,
            dispatcher: Dispatcher::new(&Default::default(), Arc::new(|| None)),
            port: 45890,
            on_change: Arc::new(|_| {}),
        })
    }

    async fn status(app: Router, uri: &str) -> StatusCode {
        app.oneshot(Request::get(uri).body(Body::empty()).expect("request"))
            .await
            .expect("response")
            .status()
    }

    #[tokio::test]
    async fn state_is_served_under_chat() {
        assert_eq!(
            status(router(test_state()), "/chat/state").await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn the_twitch_callback_lives_under_chat() {
        assert_eq!(
            status(router(test_state()), "/chat/auth/callback").await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn the_old_callback_path_is_gone() {
        assert_eq!(
            status(router(test_state()), "/auth/callback").await,
            StatusCode::NOT_FOUND
        );
    }
}
