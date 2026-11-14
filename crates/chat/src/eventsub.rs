use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use twitch_api::eventsub::channel::chat::{Fragment, message::MessageType};
use twitch_api::eventsub::channel::{
    ChannelChatClearUserMessagesV1, ChannelChatClearV1, ChannelChatMessageDeleteV1,
    ChannelChatMessageV1, ChannelChatMessageV1Payload, ChannelChatNotificationV1, ChannelCheerV1,
    ChannelFollowV2, ChannelPointsCustomRewardRedemptionAddV1, ChannelRaidV1, ChannelSubscribeV1,
};
use twitch_api::eventsub::{
    Event, EventSubscription, EventType, EventsubWebsocketData, Message as EsMessage, Transport,
};
use twitch_api::helix::users::GetUsersRequest;
use twitch_api::types::UserId;
use twitch_oauth2::UserToken;

use crate::chat::{self, ChatMessage, Reply, User};
use crate::twitch::TwitchError;
use crate::{ChatState, Link};

const WS_URL: &str = "wss://eventsub.wss.twitch.tv/ws";
const MAX_BACKOFF_SECS: u64 = 30;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const WELCOME_TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_QUIET: Duration = Duration::from_millis(300);
const KEEPALIVE_GRACE_SECS: u64 = 5;
const DEFAULT_KEEPALIVE_SECS: u64 = 10;

const CRITICAL_TYPES: [EventType; 5] = [
    ChannelChatMessageV1::EVENT_TYPE,
    ChannelChatClearV1::EVENT_TYPE,
    ChannelChatClearUserMessagesV1::EVENT_TYPE,
    ChannelChatMessageDeleteV1::EVENT_TYPE,
    ChannelChatNotificationV1::EVENT_TYPE,
];

enum SessionEnd {
    Closed,
    Restart,
}

struct SessionError {
    message: String,
    established: bool,
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct Conn {
    sink: SplitSink<Socket, Ws>,
    source: SplitStream<Socket>,
    session_id: String,
    keepalive_secs: u64,
}

pub fn spawn(state: Arc<ChatState>) {
    tokio::spawn(run(state));
}

async fn run(state: Arc<ChatState>) {
    let mut backoff = 1;

    loop {
        if state.shutdown.is_cancelled() {
            return;
        }

        let Some(token) = state.token.read().await.clone() else {
            set_link(&state, Link::Offline, "no token, sign-in is needed").await;
            if !wait(&state, 3).await {
                return;
            }
            continue;
        };

        set_link(&state, Link::Connecting, "opening the eventsub socket").await;

        match session(&state, &token).await {
            Ok(SessionEnd::Restart) => {
                log::info!("eventsub session dropped on request");
                backoff = 1;
            }
            Ok(SessionEnd::Closed) => {
                set_link(&state, Link::Offline, "the session closed").await;
                backoff = 1;
                if !wait(&state, 1).await {
                    return;
                }
            }
            Err(err) => {
                log::warn!("eventsub: {}", err.message);
                set_link(&state, Link::Offline, &err.message).await;
                if err.established {
                    backoff = 1;
                }
                if !wait(&state, backoff).await {
                    return;
                }
                backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
            }
        }
    }
}

async fn session(state: &Arc<ChatState>, token: &UserToken) -> Result<SessionEnd, SessionError> {
    let mut established = false;
    match run_session(state, token, &mut established).await {
        Ok(end) => Ok(end),
        Err(message) => Err(SessionError {
            message,
            established,
        }),
    }
}

async fn run_session(
    state: &Arc<ChatState>,
    token: &UserToken,
    established: &mut bool,
) -> Result<SessionEnd, String> {
    let mut conn = open(WS_URL).await?;
    *established = true;
    log::info!("eventsub session {}", conn.session_id);
    set_link(state, Link::Connected, "the welcome message arrived").await;

    match subscribe_all(state, &conn.session_id).await {
        Ok(count) => {
            log::info!(
                "eventsub subscribed to {count} event types on session {}",
                conn.session_id
            );
            crate::auth::push_notice(state, "Connected to chat".to_string());
        }
        Err(err) => return Err(err),
    }

    loop {
        let idle = Duration::from_secs(conn.keepalive_secs + KEEPALIVE_GRACE_SECS);
        let message = tokio::select! {
            _ = state.shutdown.cancelled() => return Ok(SessionEnd::Closed),
            _ = state.reconnect.notified() => return Ok(SessionEnd::Restart),
            next = conn.source.next() => next,
            _ = tokio::time::sleep(idle) => {
                return Err(format!(
                    "no frame for {}s, the connection is dead",
                    idle.as_secs()
                ));
            }
        };

        let Some(message) = message else {
            return Ok(SessionEnd::Closed);
        };

        let text = match message.map_err(|err| format!("websocket error: {err}"))? {
            Ws::Text(text) => text,
            Ws::Close(frame) => {
                log::info!("eventsub closed by Twitch {}", close_reason(frame));
                return Ok(SessionEnd::Closed);
            }
            Ws::Ping(payload) => {
                conn.sink
                    .send(Ws::Pong(payload))
                    .await
                    .map_err(|err| format!("cannot answer ping: {err}"))?;
                continue;
            }
            _ => continue,
        };

        state.touch_event();
        let data = match Event::parse_websocket(text.as_str()) {
            Ok(data) => data,
            Err(err) => {
                log::warn!("eventsub: cannot parse a frame: {err}");
                continue;
            }
        };

        match data {
            EventsubWebsocketData::Welcome { payload, .. } => {
                log::debug!("eventsub session {} welcomed twice", payload.session.id);
            }
            EventsubWebsocketData::Keepalive { .. } => {}
            EventsubWebsocketData::Notification { payload, .. } => {
                on_event(state, token, payload).await;
            }
            EventsubWebsocketData::Reconnect { payload, .. } => {
                let Some(url) = payload.session.reconnect_url else {
                    log::warn!("eventsub asked to reconnect without a url");
                    continue;
                };
                log::info!("eventsub asked to reconnect");
                let next = open(url.as_ref())
                    .await
                    .map_err(|err| format!("cannot follow the reconnect url: {err}"))?;
                log::info!("eventsub session {}", next.session_id);
                let carried = carry_over(state, token, &next.session_id).await;
                drain(state, token, &mut conn).await;
                if !carried {
                    return Err("the reconnected session has no subscriptions".to_string());
                }
                conn = next;
                state.count_reconnect();
                crate::auth::push_notice(state, "Reconnected to chat".to_string());
            }
            EventsubWebsocketData::Revocation { .. } => {
                log::warn!("eventsub revoked a subscription, signing in again may help");
            }
            _ => {}
        }
    }
}

async fn open(url: &str) -> Result<Conn, String> {
    let (stream, _) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
        .await
        .map_err(|_| format!("cannot connect within {}s", CONNECT_TIMEOUT.as_secs()))?
        .map_err(|err| format!("cannot connect: {err}"))?;
    let (mut sink, mut source) = stream.split();

    tokio::time::timeout(WELCOME_TIMEOUT, async move {
        loop {
            let Some(message) = source.next().await else {
                return Err("eventsub closed before the welcome message".to_string());
            };
            match message.map_err(|err| format!("websocket error: {err}"))? {
                Ws::Text(text) => match Event::parse_websocket(text.as_str()) {
                    Ok(EventsubWebsocketData::Welcome { payload, .. }) => {
                        let keepalive_secs = payload
                            .session
                            .keepalive_timeout_seconds
                            .and_then(|secs| u64::try_from(secs).ok())
                            .unwrap_or(DEFAULT_KEEPALIVE_SECS);
                        return Ok(Conn {
                            sink,
                            source,
                            session_id: payload.session.id.to_string(),
                            keepalive_secs,
                        });
                    }
                    Ok(_) => {}
                    Err(err) => return Err(format!("cannot parse the welcome message: {err}")),
                },
                Ws::Ping(payload) => {
                    sink.send(Ws::Pong(payload))
                        .await
                        .map_err(|err| format!("cannot answer ping: {err}"))?;
                }
                Ws::Close(frame) => {
                    return Err(format!(
                        "eventsub closed by Twitch before the welcome {}",
                        close_reason(frame)
                    ));
                }
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| {
        format!(
            "the welcome message did not arrive within {}s",
            WELCOME_TIMEOUT.as_secs()
        )
    })?
}

async fn drain(state: &Arc<ChatState>, token: &UserToken, conn: &mut Conn) {
    let mut kept = 0;
    loop {
        let next = tokio::time::timeout(DRAIN_QUIET, conn.source.next()).await;
        let Ok(Some(Ok(message))) = next else {
            break;
        };
        match message {
            Ws::Text(text) => match Event::parse_websocket(text.as_str()) {
                Ok(EventsubWebsocketData::Notification { payload, .. }) => {
                    state.touch_event();
                    on_event(state, token, payload).await;
                    kept += 1;
                }
                Ok(_) => {}
                Err(err) => log::debug!("eventsub: cannot parse a leftover frame: {err}"),
            },
            Ws::Ping(payload) => {
                let _ = conn.sink.send(Ws::Pong(payload)).await;
            }
            Ws::Close(_) => break,
            _ => {}
        }
    }
    if kept > 0 {
        log::info!("eventsub kept {kept} events from the old session");
    }
}

async fn carry_over(state: &Arc<ChatState>, token: &UserToken, session_id: &str) -> bool {
    let Some(found) = session_types(state, token, session_id).await else {
        return true;
    };

    let missing: Vec<EventType> = CRITICAL_TYPES
        .iter()
        .filter(|kind| !found.contains(kind))
        .copied()
        .collect();
    if missing.is_empty() {
        log::info!(
            "eventsub carried over {} subscriptions",
            CRITICAL_TYPES.len()
        );
        return true;
    }

    log::warn!(
        "eventsub carried over {} of {} subscriptions, resubscribing",
        CRITICAL_TYPES.len() - missing.len(),
        CRITICAL_TYPES.len()
    );
    let Ok(broadcaster) = resolve_broadcaster(state).await else {
        log::warn!("eventsub: cannot resubscribe without the channel id");
        return false;
    };
    let Ok(fresh) = crate::twitch::token(state).await else {
        log::warn!("eventsub: cannot resubscribe without a token");
        return false;
    };
    let transport = Transport::websocket(session_id);
    let subscriptions: Vec<Core> = Core::build(&broadcaster, &fresh.user_id)
        .into_iter()
        .filter(|subscription| missing.contains(&subscription.event_type()))
        .collect();

    match subscribe_critical(state, &fresh, &transport, &subscriptions).await {
        Ok(count) => {
            log::info!("eventsub resubscribed {count} event types");
            true
        }
        Err(err) => {
            log::warn!("eventsub: cannot resubscribe: {err}");
            false
        }
    }
}

async fn session_types(
    state: &Arc<ChatState>,
    token: &UserToken,
    session_id: &str,
) -> Option<Vec<EventType>> {
    let mut kinds = Vec::new();
    let mut pages =
        state
            .helix
            .get_eventsub_subscriptions(None, None, Some(token.user_id.as_ref()), token);

    while let Some(page) = pages.next().await {
        match page {
            Ok(page) => {
                for subscription in page.subscriptions {
                    if let Some(transport) = subscription.transport.as_websocket()
                        && transport.session_id == session_id
                    {
                        kinds.push(subscription.type_);
                    }
                }
            }
            Err(err) => {
                log::warn!("eventsub: cannot list subscriptions: {err}");
                return None;
            }
        }
    }

    Some(kinds)
}

enum Core {
    Message(ChannelChatMessageV1),
    Clear(ChannelChatClearV1),
    ClearUser(ChannelChatClearUserMessagesV1),
    Delete(ChannelChatMessageDeleteV1),
    Notification(ChannelChatNotificationV1),
}

impl Core {
    fn build(broadcaster: &UserId, user: &UserId) -> Vec<Self> {
        vec![
            Self::Message(ChannelChatMessageV1::new(broadcaster.clone(), user.clone())),
            Self::Clear(ChannelChatClearV1::new(broadcaster.clone(), user.clone())),
            Self::ClearUser(ChannelChatClearUserMessagesV1::new(
                broadcaster.clone(),
                user.clone(),
            )),
            Self::Delete(ChannelChatMessageDeleteV1::new(
                broadcaster.clone(),
                user.clone(),
            )),
            Self::Notification(ChannelChatNotificationV1::new(
                broadcaster.clone(),
                user.clone(),
            )),
        ]
    }

    fn event_type(&self) -> EventType {
        match self {
            Self::Message(_) => ChannelChatMessageV1::EVENT_TYPE,
            Self::Clear(_) => ChannelChatClearV1::EVENT_TYPE,
            Self::ClearUser(_) => ChannelChatClearUserMessagesV1::EVENT_TYPE,
            Self::Delete(_) => ChannelChatMessageDeleteV1::EVENT_TYPE,
            Self::Notification(_) => ChannelChatNotificationV1::EVENT_TYPE,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Message(_) => "channel.chat.message",
            Self::Clear(_) => "channel.chat.clear",
            Self::ClearUser(_) => "channel.chat.clear_user_messages",
            Self::Delete(_) => "channel.chat.message_delete",
            Self::Notification(_) => "channel.chat.notification",
        }
    }

    async fn create(
        &self,
        state: &Arc<ChatState>,
        token: &UserToken,
        transport: &Transport,
    ) -> Result<(), String> {
        match self {
            Self::Message(subscription) => {
                subscribe(state, token, transport, subscription.clone()).await
            }
            Self::Clear(subscription) => {
                subscribe(state, token, transport, subscription.clone()).await
            }
            Self::ClearUser(subscription) => {
                subscribe(state, token, transport, subscription.clone()).await
            }
            Self::Delete(subscription) => {
                subscribe(state, token, transport, subscription.clone()).await
            }
            Self::Notification(subscription) => {
                subscribe(state, token, transport, subscription.clone()).await
            }
        }
    }
}

async fn subscribe_all(state: &Arc<ChatState>, session_id: &str) -> Result<usize, String> {
    let broadcaster = resolve_broadcaster(state)
        .await
        .map_err(|err| err.to_string())?;
    let token = crate::twitch::token(state)
        .await
        .map_err(|_| "sign in again: Twitch rejected the token".to_string())?;
    let user = token.user_id.clone();
    let token = &token;
    let transport = Transport::websocket(session_id);

    let core = Core::build(&broadcaster, &user);
    let mut count = subscribe_critical(state, token, &transport, &core).await?;

    let follow = ChannelFollowV2::new(broadcaster.clone(), user.clone());
    count += optional(state, token, &transport, "channel.follow", follow).await;

    let points = ChannelPointsCustomRewardRedemptionAddV1::broadcaster_user_id(broadcaster.clone());
    count += optional(
        state,
        token,
        &transport,
        "channel.channel_points_custom_reward_redemption.add",
        points,
    )
    .await;

    let cheer = ChannelCheerV1::broadcaster_user_id(broadcaster.clone());
    count += optional(state, token, &transport, "channel.cheer", cheer).await;

    let raid = ChannelRaidV1::to_broadcaster_user_id(broadcaster.clone());
    count += optional(state, token, &transport, "channel.raid", raid).await;

    let subscribed = ChannelSubscribeV1::broadcaster_user_id(broadcaster);
    count += optional(state, token, &transport, "channel.subscribe", subscribed).await;

    Ok(count)
}

async fn subscribe_critical(
    state: &Arc<ChatState>,
    token: &UserToken,
    transport: &Transport,
    subscriptions: &[Core],
) -> Result<usize, String> {
    let mut created = 0;
    let mut errors = Vec::new();

    for subscription in subscriptions {
        match subscription.create(state, token, transport).await {
            Ok(()) => created += 1,
            Err(err) => {
                log::warn!(
                    "eventsub: cannot subscribe to {}: {err}",
                    subscription.name()
                );
                errors.push(subscription.name());
            }
        }
    }

    if created == 0 {
        let names = errors.join(", ");
        return Err(format!("no chat subscription was created: {names}"));
    }
    if !errors.is_empty() {
        log::warn!(
            "eventsub: {} of {} core subscriptions failed",
            errors.len(),
            subscriptions.len()
        );
    }

    Ok(created)
}

async fn optional<S>(
    state: &Arc<ChatState>,
    token: &UserToken,
    transport: &Transport,
    name: &str,
    subscription: S,
) -> usize
where
    S: EventSubscription + Send,
{
    match subscribe(state, token, transport, subscription).await {
        Ok(()) => 1,
        Err(err) => {
            log::warn!("eventsub: cannot subscribe to {name}: {err}");
            0
        }
    }
}

async fn subscribe<S>(
    state: &Arc<ChatState>,
    token: &UserToken,
    transport: &Transport,
    subscription: S,
) -> Result<(), String>
where
    S: EventSubscription + Send,
{
    state
        .helix
        .create_eventsub_subscription(subscription, transport.clone(), token)
        .await
        .map(|_| ())
        .map_err(|err| format!("cannot subscribe: {err}"))
}

#[derive(Debug)]
pub enum ResolveError {
    NotConfigured,
    NotFound(String),
    Twitch(TwitchError),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "no Twitch channel is set"),
            Self::NotFound(channel) => write!(f, "#{channel} does not exist on Twitch"),
            Self::Twitch(err) => write!(f, "{err}"),
        }
    }
}

pub async fn resolve_broadcaster(state: &Arc<ChatState>) -> Result<UserId, ResolveError> {
    let channel = state.config.read().await.channel.clone();
    if channel.is_empty() {
        return Err(ResolveError::NotConfigured);
    }

    let login: twitch_api::types::UserName = channel.as_str().into();
    let users = crate::twitch::get(state, GetUsersRequest::logins(&[login][..]))
        .await
        .map_err(ResolveError::Twitch)?;

    users
        .first()
        .map(|user| user.id.clone())
        .ok_or(ResolveError::NotFound(channel))
}

async fn on_event(state: &Arc<ChatState>, token: &UserToken, event: Event) {
    let me = token.login.to_string();
    let channel = state.config.read().await.channel.clone();

    match event {
        Event::ChannelChatMessageV1(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };

            let mut chat_message = chat_from_message(&channel, &me, &message);
            if let Some(reply) = &message.reply
                && reply.parent_user_login.as_str() == me
            {
                chat_message.highlighted = true;
            }
            if let Some(reply) = state.dispatcher.handle(&chat_message, &me) {
                let reply_to = (!chat_message.id.starts_with(chat::LOCAL_ID_PREFIX))
                    .then(|| chat_message.id.clone());
                let state = state.clone();
                tokio::spawn(async move {
                    crate::command::reply(&state, reply, reply_to).await;
                });
            }
            state.push(chat_message);
        }
        Event::ChannelChatNotificationV1(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };

            let text = if message.system_message.is_empty() {
                message.message.text.clone()
            } else {
                message.system_message.clone()
            };
            let mut chat_message =
                ChatMessage::new(&channel, chat::KIND_NOTICE, notice_user(), text);
            chat_message.id = message.message_id.to_string();
            state.push(chat_message);
        }
        Event::ChannelChatClearV1(payload) => {
            let EsMessage::Notification(_) = payload.message else {
                return;
            };
            let marker = ChatMessage::new(
                &channel,
                chat::KIND_CLEAR,
                notice_user(),
                "Chat was cleared",
            );
            state.push(marker);
        }
        Event::ChannelChatClearUserMessagesV1(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };
            let mut marker = ChatMessage::new(
                &channel,
                chat::KIND_CLEAR_USER,
                notice_user(),
                format!("Messages of {} were cleared", message.target_user_login),
            );
            marker.user.login = message.target_user_login.to_string();
            state.push(marker);
        }
        Event::ChannelChatMessageDeleteV1(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };
            let mut marker = ChatMessage::new(
                &channel,
                chat::KIND_DELETE,
                notice_user(),
                format!("A message of {} was deleted", message.target_user_login),
            );
            marker.id = message.message_id.to_string();
            state.push(marker);
        }
        Event::ChannelFollowV2(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };
            event_message(
                state,
                &channel,
                "follow",
                message.user_login.as_str(),
                message.user_name.as_str(),
                format!("{} followed", message.user_name),
                None,
            );
        }
        Event::ChannelSubscribeV1(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };
            event_message(
                state,
                &channel,
                "subscribe",
                message.user_login.as_str(),
                message.user_name.as_str(),
                format!(
                    "{} subscribed{}",
                    message.user_name,
                    if message.is_gift { " (gift)" } else { "" }
                ),
                None,
            );
        }
        Event::ChannelCheerV1(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };
            let login = message.user_login.as_ref().map(|v| v.to_string());
            let name = message.user_name.as_ref().map(|v| v.to_string());
            let who = name.clone().unwrap_or_else(|| "anonymous".to_string());
            event_message(
                state,
                &channel,
                "cheer",
                &login.unwrap_or_default(),
                &who,
                format!("{who} cheered {} bits", message.bits),
                Some(message.bits.max(0) as u64),
            );
        }
        Event::ChannelRaidV1(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };
            event_message(
                state,
                &channel,
                "raid",
                message.from_broadcaster_user_login.as_str(),
                message.from_broadcaster_user_name.as_str(),
                format!(
                    "{} raided with {} viewers",
                    message.from_broadcaster_user_name, message.viewers
                ),
                None,
            );
        }
        Event::ChannelPointsCustomRewardRedemptionAddV1(payload) => {
            let EsMessage::Notification(message) = payload.message else {
                return;
            };
            let mut chat_message = ChatMessage::new(
                &channel,
                chat::KIND_EVENT,
                user(
                    message.user_id.to_string(),
                    message.user_login.as_str(),
                    message.user_name.as_str(),
                    None,
                    Vec::new(),
                ),
                format!(
                    "{} redeemed {}{}",
                    message.user_name,
                    message.reward.title,
                    if message.user_input.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", message.user_input)
                    }
                ),
            );
            chat_message.highlighted = true;
            chat_message.event = Some(chat::Event {
                kind: "redemption".to_string(),
                summary: message.reward.title.clone(),
                amount: Some(message.reward.cost.max(0) as u64),
            });
            state.push(chat_message);
        }
        _ => {}
    }
}

fn event_message(
    state: &Arc<ChatState>,
    channel: &str,
    kind: &str,
    login: &str,
    name: &str,
    summary: String,
    amount: Option<u64>,
) {
    let mut chat_message = ChatMessage::new(
        channel,
        chat::KIND_EVENT,
        user(String::new(), login, name, None, Vec::new()),
        summary.clone(),
    );
    chat_message.event = Some(chat::Event {
        kind: kind.to_string(),
        summary,
        amount,
    });
    state.push(chat_message);
}

fn chat_from_message(
    channel: &str,
    me: &str,
    message: &ChannelChatMessageV1Payload,
) -> ChatMessage {
    let badges = message
        .badges
        .iter()
        .map(|badge| chat::Badge {
            name: badge.set_id.to_string(),
            version: badge.id.to_string(),
        })
        .collect::<Vec<_>>();

    let has_badge = |name: &str| badges.iter().any(|badge| badge.name == name);
    let moderator = has_badge("moderator");
    let subscriber = has_badge("subscriber") || has_badge("founder");
    let vip = has_badge("vip");
    let broadcaster = has_badge("broadcaster");

    let color = message.color.to_string();
    let mut user = user(
        message.chatter_user_id.to_string(),
        message.chatter_user_login.as_str(),
        message.chatter_user_name.as_str(),
        if color.is_empty() || color == "#000000" {
            None
        } else {
            Some(color)
        },
        badges,
    );
    user.moderator = moderator;
    user.subscriber = subscriber;
    user.vip = vip;
    user.broadcaster = broadcaster;

    let fragments = message
        .message
        .fragments
        .iter()
        .map(|fragment| match fragment {
            Fragment::Text { text } => chat::Fragment::Text { text: text.clone() },
            Fragment::Emote { text, emote } => chat::Fragment::Emote {
                text: text.clone(),
                id: emote.id.to_string(),
            },
            Fragment::Cheermote {
                text, cheermote, ..
            } => chat::Fragment::Cheermote {
                text: text.clone(),
                prefix: cheermote.prefix.clone(),
                bits: cheermote.bits.max(0) as u64,
                tier: cheermote.tier.max(0) as u64,
            },
            Fragment::Mention { text, mention } => chat::Fragment::Mention {
                text: text.clone(),
                login: mention.user_login.to_string(),
                name: mention.user_name.to_string(),
            },
            _ => chat::Fragment::Text {
                text: fragment.text().to_string(),
            },
        })
        .collect::<Vec<_>>();

    let mut text = message.message.text.clone();
    let mut action = false;
    if let Some(rest) = text.strip_prefix("/me ") {
        text = rest.to_string();
        action = true;
    }

    let reply = message.reply.as_ref().map(|reply| Reply {
        id: reply.parent_message_id.to_string(),
        user: reply.parent_user_login.to_string(),
        text: reply.parent_message_body.clone(),
    });

    let mentions_me = text
        .to_ascii_lowercase()
        .contains(&format!("@{me}").to_ascii_lowercase());
    let highlighted =
        mentions_me || matches!(message.message_type, MessageType::ChannelPointsHighlighted);

    let mut chat_message = ChatMessage::new(channel, chat::KIND_CHAT, user, text);
    chat_message.id = message.message_id.to_string();
    chat_message.fragments = fragments;
    chat_message.reply = reply;
    chat_message.action = action;
    chat_message.highlighted = highlighted;
    chat_message.moderatable = true;
    chat_message
}

fn user(
    id: String,
    login: &str,
    name: &str,
    color: Option<String>,
    badges: Vec<chat::Badge>,
) -> User {
    User {
        id,
        login: login.to_string(),
        name: name.to_string(),
        color,
        badges,
        moderator: false,
        subscriber: false,
        vip: false,
        broadcaster: false,
    }
}

fn notice_user() -> User {
    user(
        String::new(),
        "twitch-chat",
        "Twitch Chat",
        None,
        Vec::new(),
    )
}

fn close_reason(frame: Option<CloseFrame>) -> String {
    let Some(frame) = frame else {
        return "without a close frame".to_string();
    };
    let code = u16::from(frame.code);
    if frame.reason.is_empty() {
        format!("with code {code}")
    } else {
        format!("with code {code}: {}", frame.reason)
    }
}

async fn set_link(state: &Arc<ChatState>, link: Link, reason: &str) {
    let mut current = state.eventsub.write().await;
    if *current != link {
        log::info!("eventsub status: {} ({reason})", link.as_str());
        *current = link;
    }
}

async fn wait(state: &Arc<ChatState>, secs: u64) -> bool {
    tokio::select! {
        _ = state.shutdown.cancelled() => false,
        _ = tokio::time::sleep(Duration::from_secs(secs)) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_subscriptions_cover_every_critical_type() {
        let user = UserId::from_static("1");
        let kinds: Vec<EventType> = Core::build(&user, &user)
            .iter()
            .map(Core::event_type)
            .collect();

        assert_eq!(kinds, CRITICAL_TYPES);
    }
}
