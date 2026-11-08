use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use twitch_api::helix::chat::get_channel_chat_badges::GetChannelChatBadgesRequest;
use twitch_api::helix::chat::get_global_chat_badges::GetGlobalChatBadgesRequest;
use twitch_api::helix::chat::send_chat_message::{SendChatMessageBody, SendChatMessageRequest};
use twitch_api::helix::moderation::ban_user::{BanUserBody, BanUserRequest};
use twitch_api::helix::moderation::delete_chat_messages::DeleteChatMessagesRequest;
use twitch_api::types::UserId;
use twitch_oauth2::{TwitchToken, UserToken};

use crate::chat::{self, ChatMessage, User};
use crate::config::Widget;
use crate::eventsub::ResolveError;
use crate::twitch::{self, TwitchError};
use crate::{ChatState, Link};

type ApiResult<T> = Result<T, (StatusCode, Json<Value>)>;

fn bad_request(message: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message })))
}

fn internal_error(message: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": message })),
    )
}

fn unauthorized(action: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": format!("sign in to {action}") })),
    )
}

fn twitch_status_error(status: StatusCode, message: String) -> (StatusCode, Json<Value>) {
    if status == StatusCode::UNAUTHORIZED {
        if let Some(scope) = missing_scope(&message) {
            let text = format!("sign in again to grant the {scope} scope");
            return (StatusCode::FORBIDDEN, Json(json!({ "error": text })));
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "sign in again: Twitch rejected the token" })),
        );
    }
    let text = match status {
        StatusCode::FORBIDDEN => "no rights to moderate this channel",
        StatusCode::NOT_FOUND => "Twitch did not find the message or the user",
        _ => return (StatusCode::BAD_GATEWAY, Json(json!({ "error": message }))),
    };
    (status, Json(json!({ "error": text })))
}

fn missing_scope(message: &str) -> Option<String> {
    let lower = message.to_ascii_lowercase();
    let start = lower.find("missing scope")?;
    let rest = message[start..].split_once(':')?.1.trim();
    let scope = rest.split_whitespace().next().unwrap_or_default();
    if scope.is_empty() {
        return None;
    }
    Some(
        scope
            .trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != ':')
            .to_string(),
    )
}

fn is_twitch_message_id(id: &str) -> bool {
    !id.is_empty() && !id.starts_with(chat::LOCAL_ID_PREFIX)
}

fn twitch_error(err: TwitchError, action: &str) -> (StatusCode, Json<Value>) {
    match err {
        TwitchError::SignIn => unauthorized(action),
        TwitchError::Status { status, message } => twitch_status_error(status, message),
        TwitchError::Transport => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "Twitch is not reachable right now" })),
        ),
    }
}

fn resolve_error(err: ResolveError, action: &str) -> (StatusCode, Json<Value>) {
    match err {
        ResolveError::NotConfigured => bad_request("set a Twitch channel first"),
        ResolveError::NotFound(channel) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("#{channel} does not exist on Twitch") })),
        ),
        ResolveError::Twitch(err) => twitch_error(err, action),
    }
}

async fn twitch_context(state: &Arc<ChatState>, action: &str) -> ApiResult<(UserToken, UserId)> {
    let token = match twitch::token(state).await {
        Ok(token) => token,
        Err(_) => return Err(unauthorized(action)),
    };

    match crate::eventsub::resolve_broadcaster(state).await {
        Ok(broadcaster) => Ok((token, broadcaster)),
        Err(err) => Err(resolve_error(err, action)),
    }
}

fn push_local(state: &Arc<ChatState>, kind: &str, text: &str, id: Option<String>) {
    let config = state.config.try_read();
    let channel = config
        .as_ref()
        .map(|config| config.channel.clone())
        .unwrap_or_default();
    let mut message = ChatMessage::new(
        channel,
        kind,
        User {
            login: "twitch-chat".to_string(),
            name: "Twitch Chat".to_string(),
            ..User::default()
        },
        text.to_string(),
    );
    if let Some(id) = id {
        message.id = id;
    }
    state.push(message);
}

#[derive(Serialize)]
pub struct ReadCursor {
    pub last_read: u64,
    pub unread: u64,
}

#[derive(Serialize)]
pub struct Info {
    pub channel: String,
    pub port: u16,
    pub authenticated: bool,
    pub login: String,
    pub widget: Widget,
    pub eventsub: Link,
    pub messages: u64,
}

impl Info {
    async fn from_config(state: &ChatState, config: &crate::config::ChatConfig) -> Self {
        let token = state.token.read().await;
        Self {
            channel: config.channel.clone(),
            port: state.port,
            authenticated: token.as_ref().is_some_and(|token| !token.is_elapsed()),
            login: config.tokens.login.clone(),
            widget: config.widget.clone(),
            eventsub: *state.eventsub.read().await,
            messages: state.store.last_seq(),
        }
    }
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    pub after: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct SetChannel {
    pub channel: String,
}

#[derive(Deserialize)]
pub struct SendMessage {
    pub text: String,
    pub reply_to: Option<String>,
}

const MAX_TIMEOUT_SECONDS: u32 = 1_209_600;

#[derive(Deserialize)]
pub struct TimeoutRequest {
    pub user_id: String,
    pub duration: u32,
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct BanRequest {
    pub user_id: String,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct MessagesPage {
    pub items: Vec<ChatMessage>,
    pub last_seq: u64,
}

const BADGE_TTL: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct BadgeCache {
    fetched: Option<Instant>,
    images: std::collections::HashMap<String, String>,
}

#[derive(Serialize)]
pub struct Badges {
    pub badges: std::collections::HashMap<String, String>,
}

pub async fn badges(State(state): State<Arc<ChatState>>) -> ApiResult<Json<Badges>> {
    {
        let cache = state.badges.read().await;
        if let Some(fetched) = cache.fetched
            && fetched.elapsed() < BADGE_TTL
        {
            return Ok(Json(Badges {
                badges: cache.images.clone(),
            }));
        }
    }

    let (_token, broadcaster) = twitch_context(&state, "load chat badges").await?;
    let mut images = std::collections::HashMap::new();

    match twitch::get(&state, GetGlobalChatBadgesRequest::new()).await {
        Ok(response) => collect_badges(&response, &mut images),
        Err(err) => log::warn!("cannot load global chat badges: {err}"),
    }

    match twitch::get(
        &state,
        GetChannelChatBadgesRequest::broadcaster_id(broadcaster),
    )
    .await
    {
        Ok(response) => collect_badges(&response, &mut images),
        Err(err) => log::warn!("cannot load channel chat badges: {err}"),
    }

    if images.is_empty() {
        return Err(internal_error("Twitch did not return chat badges"));
    }

    let mut cache = state.badges.write().await;
    cache.fetched = Some(Instant::now());
    cache.images = images.clone();

    Ok(Json(Badges { badges: images }))
}

fn collect_badges(
    sets: &[twitch_api::helix::chat::BadgeSet],
    images: &mut std::collections::HashMap<String, String>,
) {
    for set in sets {
        for version in &set.versions {
            images.insert(
                format!("{}/{}", set.set_id, version.id),
                version.image_url_2x.clone(),
            );
        }
    }
}

pub async fn mark_read(State(state): State<Arc<ChatState>>) -> Json<ReadCursor> {
    Json(ReadCursor {
        last_read: state.store.mark_read(),
        unread: state.store.unread(),
    })
}

pub async fn info(State(state): State<Arc<ChatState>>) -> Json<Info> {
    let config = state.config.read().await;
    Json(Info::from_config(&state, &config).await)
}

pub async fn set_channel(
    State(state): State<Arc<ChatState>>,
    Json(body): Json<SetChannel>,
) -> ApiResult<Json<Info>> {
    let Some(channel) = crate::config::normalize_channel(&body.channel) else {
        return Err(bad_request(
            "channel must be 1-25 letters, digits or underscores",
        ));
    };

    let mut updated = state.config.read().await.clone();
    let changed = updated.channel != channel;
    updated.channel = channel.clone();

    let info = Info::from_config(&state, &updated).await;
    *state.config.write().await = updated;
    state.persist_async().await;

    if changed {
        log::info!("channel switched to {channel}");
        let user = User {
            login: "twitch-chat".to_string(),
            name: "Twitch Chat".to_string(),
            ..User::default()
        };
        let mut message = ChatMessage::new(
            channel.clone(),
            "notice",
            user,
            format!("Channel set to {channel}"),
        );
        message.id = format!("{}{}", chat::LOCAL_ID_PREFIX, message.ts);
        state.push(message);
    }

    Ok(Json(info))
}

pub async fn messages(
    State(state): State<Arc<ChatState>>,
    Query(query): Query<MessagesQuery>,
) -> Json<MessagesPage> {
    let limit = query.limit.unwrap_or(200).clamp(1, 500);
    Json(MessagesPage {
        items: state.store.snapshot(query.after.unwrap_or(0), limit),
        last_seq: state.store.last_seq(),
    })
}

pub async fn send_message(
    State(state): State<Arc<ChatState>>,
    Json(body): Json<SendMessage>,
) -> ApiResult<Json<Value>> {
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return Err(bad_request("message is empty"));
    }
    if text.chars().count() > 500 {
        return Err(bad_request("message is longer than 500 characters"));
    }

    let (token, broadcaster) = twitch_context(&state, "send messages").await?;

    let mut outgoing = SendChatMessageBody::new(broadcaster, token.user_id.clone(), text.as_str());
    if let Some(reply) = body.reply_to {
        outgoing = outgoing.reply_parent_message_id(reply);
    }

    let response = match twitch::post(&state, SendChatMessageRequest::new(), outgoing).await {
        Ok(response) => response,
        Err(err) => return Err(twitch_error(err, "send messages")),
    };

    if !response.is_sent {
        let reason = response
            .drop_reason
            .map(|reason| reason.message)
            .unwrap_or_else(|| "message was not delivered".to_string());
        log::warn!("message dropped: {reason}");
        return Err((StatusCode::BAD_GATEWAY, Json(json!({ "error": reason }))));
    }

    log::debug!("message sent");
    Ok(Json(json!({
        "sent": true,
        "id": response.message_id.map(|id| id.to_string()),
    })))
}

pub(crate) async fn send_text(
    state: &Arc<ChatState>,
    text: &str,
    reply_to: Option<&str>,
) -> Result<(), String> {
    let (token, broadcaster) = twitch_context(state, "send messages")
        .await
        .map_err(|err| {
            err.1
                .0
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("cannot send the message")
                .to_string()
        })?;
    let mut outgoing = SendChatMessageBody::new(broadcaster, token.user_id.clone(), text);
    if let Some(reply) = reply_to {
        outgoing = outgoing.reply_parent_message_id(reply);
    }

    let response = twitch::post(state, SendChatMessageRequest::new(), outgoing)
        .await
        .map_err(|err| err.to_string())?;
    if !response.is_sent {
        return Err(response
            .drop_reason
            .map(|reason| reason.message)
            .unwrap_or_else(|| "message was not delivered".to_string()));
    }
    Ok(())
}

pub async fn delete_message(
    State(state): State<Arc<ChatState>>,
    Path(message_id): Path<String>,
) -> ApiResult<Json<Value>> {
    if !is_twitch_message_id(&message_id) {
        return Err(bad_request("only live chat messages can be deleted"));
    }
    let (token, broadcaster) = twitch_context(&state, "moderate").await?;
    let request = DeleteChatMessagesRequest::new(broadcaster, token.user_id.clone())
        .message_id(message_id.as_str());

    match twitch::delete(&state, request).await {
        Ok(_) => {
            log::debug!("message deleted");
            push_local(
                &state,
                chat::KIND_DELETE,
                "A message was deleted",
                Some(message_id),
            );
            Ok(Json(json!({ "deleted": true })))
        }
        Err(err) => Err(twitch_error(err, "moderate")),
    }
}

pub async fn timeout_user(
    State(state): State<Arc<ChatState>>,
    Json(body): Json<TimeoutRequest>,
) -> ApiResult<Json<Value>> {
    if body.user_id.is_empty() {
        return Err(bad_request("user_id is required"));
    }
    if !(1..=MAX_TIMEOUT_SECONDS).contains(&body.duration) {
        return Err(bad_request(
            "duration must be between 1 and 1209600 seconds",
        ));
    }

    let (token, broadcaster) = twitch_context(&state, "moderate").await?;
    let reason = body.reason.unwrap_or_default();
    let outgoing = BanUserBody::new(body.user_id.as_str(), reason.as_str(), Some(body.duration));
    ban(&state, &token, broadcaster, outgoing, body.duration).await
}

pub async fn ban_user(
    State(state): State<Arc<ChatState>>,
    Json(body): Json<BanRequest>,
) -> ApiResult<Json<Value>> {
    if body.user_id.is_empty() {
        return Err(bad_request("user_id is required"));
    }

    let (token, broadcaster) = twitch_context(&state, "moderate").await?;
    let reason = body.reason.unwrap_or_default();
    let outgoing = BanUserBody::new(body.user_id.as_str(), reason.as_str(), None);
    ban(&state, &token, broadcaster, outgoing, 0).await
}

async fn ban(
    state: &Arc<ChatState>,
    token: &UserToken,
    broadcaster: UserId,
    outgoing: BanUserBody<'_>,
    duration: u32,
) -> ApiResult<Json<Value>> {
    let request = BanUserRequest::new(broadcaster, token.user_id.clone());
    match twitch::post(state, request, outgoing).await {
        Ok(_) => {
            let text = if duration == 0 {
                "A user was banned".to_string()
            } else {
                format!("A user was timed out for {duration} seconds")
            };
            log::debug!("{text}");
            push_local(state, chat::KIND_NOTICE, &text, None);
            Ok(Json(json!({ "ok": true })))
        }
        Err(err) => Err(twitch_error(err, "moderate")),
    }
}

const PING_INTERVAL: Duration = Duration::from_secs(10);

pub async fn events(
    State(state): State<Arc<ChatState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.store.subscribe();
    let status = json!({
        "channel": state.config.read().await.channel,
        "eventsub": *state.eventsub.read().await,
        "last_seq": state.store.last_seq(),
        "last_event_seconds": state.since_event(),
    });

    let stream = async_stream::stream! {
        let first = Event::default().event("status").json_data(status);
        if let Ok(first) = first {
            yield Ok(first);
        }

        let mut ticker = tokio::time::interval(PING_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;

        loop {
            tokio::select! {
                received = receiver.recv() => match received {
                    Ok(message) => match Event::default().event("message").json_data(&message) {
                        Ok(event) => yield Ok(event),
                        Err(err) => log::warn!("cannot serialize message: {err}"),
                    },
                    Err(RecvError::Lagged(skipped)) => {
                        log::warn!("sse client is too slow, dropped {skipped} messages");
                    }
                    Err(RecvError::Closed) => break,
                },
                _ = ticker.tick() => {
                    let ping = json!({
                        "last_seq": state.store.last_seq(),
                        "eventsub": *state.eventsub.read().await,
                        "last_event_seconds": state.since_event(),
                    });
                    let event = Event::default().event("ping").json_data(ping);
                    if let Ok(event) = event {
                        yield Ok(event);
                    }
                }
            }
        }
    };

    Sse::new(stream)
}

pub async fn reconnect(State(state): State<Arc<ChatState>>) -> Json<Value> {
    log::info!("eventsub reconnect requested over the api");
    state.reconnect.notify_one();
    Json(json!({ "status": "reconnecting" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_scope_detects_scope_name() {
        assert_eq!(
            missing_scope("Missing scope: moderator:manage:chat_messages").as_deref(),
            Some("moderator:manage:chat_messages")
        );
        assert_eq!(
            missing_scope("missing scope: moderator:manage:chat_messages").as_deref(),
            Some("moderator:manage:chat_messages")
        );
    }

    #[test]
    fn missing_scope_ignores_other_errors() {
        assert_eq!(missing_scope("Invalid OAuth token"), None);
        assert_eq!(missing_scope(""), None);
    }

    #[test]
    fn unauthorized_without_scope_stays_unauthorized() {
        let (status, Json(body)) =
            twitch_status_error(StatusCode::UNAUTHORIZED, "Invalid OAuth token".into());
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body["error"].as_str().unwrap().contains("token"));
    }

    #[test]
    fn unauthorized_with_scope_becomes_forbidden() {
        let (status, Json(body)) = twitch_status_error(
            StatusCode::UNAUTHORIZED,
            "Missing scope: moderator:manage:chat_messages".into(),
        );
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("moderator:manage:chat_messages")
        );
    }

    #[test]
    fn a_rejected_token_asks_to_sign_in_again() {
        let (status, Json(body)) = resolve_error(
            ResolveError::Twitch(TwitchError::Status {
                status: StatusCode::UNAUTHORIZED,
                message: "Invalid OAuth token".into(),
            }),
            "send messages",
        );
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(body["error"].as_str().unwrap().contains("sign in again"));
    }

    #[test]
    fn a_missing_channel_is_named() {
        let (status, Json(body)) =
            resolve_error(ResolveError::NotFound("nobody".into()), "moderate");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body["error"].as_str().unwrap().contains("#nobody"));
    }

    #[test]
    fn a_signed_out_user_is_asked_to_sign_in() {
        let (status, Json(body)) = twitch_error(TwitchError::SignIn, "send messages");
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(
            body["error"]
                .as_str()
                .unwrap()
                .contains("sign in to send messages")
        );
    }

    #[test]
    fn an_unreachable_twitch_is_a_bad_gateway() {
        let (status, Json(body)) = twitch_error(TwitchError::Transport, "send messages");
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(body["error"].as_str().unwrap().contains("reachable"));
    }

    #[test]
    fn a_resolve_error_does_not_leak_the_library() {
        let err = ResolveError::Twitch(TwitchError::Status {
            status: StatusCode::BAD_REQUEST,
            message: "bad thing".into(),
        });
        assert!(!err.to_string().to_ascii_lowercase().contains("helix"));
    }

    #[test]
    fn only_twitch_ids_are_deletable() {
        assert!(is_twitch_message_id("a1b2c3d4-0000-4000-8000-000000000000"));
        assert!(!is_twitch_message_id(""));
        assert!(!is_twitch_message_id("local-2026-11-04T08:19:31.011Z"));
    }
}
