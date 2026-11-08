use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as Ws;
use twitch_api::eventsub::channel::chat::{Fragment, message::MessageType};
use twitch_api::eventsub::channel::{
    ChannelChatClearUserMessagesV1, ChannelChatClearV1, ChannelChatMessageDeleteV1,
    ChannelChatMessageV1, ChannelChatMessageV1Payload, ChannelChatNotificationV1, ChannelCheerV1,
    ChannelFollowV2, ChannelPointsCustomRewardRedemptionAddV1, ChannelRaidV1, ChannelSubscribeV1,
};
use twitch_api::eventsub::{Event, EventsubWebsocketData, Message as EsMessage, Transport};
use twitch_api::helix::users::GetUsersRequest;
use twitch_api::types::UserId;
use twitch_oauth2::UserToken;

use crate::chat::{self, ChatMessage, Reply, User};
use crate::twitch::TwitchError;
use crate::{ChatState, Link};

const WS_URL: &str = "wss://eventsub.wss.twitch.tv/ws";
const MAX_BACKOFF_SECS: u64 = 30;

enum SessionEnd {
    Reconnect(String),
    Closed,
    Restart,
}

pub fn spawn(state: Arc<ChatState>) {
    tokio::spawn(run(state));
}

async fn run(state: Arc<ChatState>) {
    let mut backoff = 1;
    let mut reconnect_url: Option<String> = None;

    loop {
        if state.shutdown.is_cancelled() {
            return;
        }

        let Some(token) = state.token.read().await.clone() else {
            set_link(&state, Link::Offline).await;
            if !wait(&state, 3).await {
                return;
            }
            continue;
        };

        set_link(&state, Link::Connecting).await;

        let (url, fresh) = match reconnect_url.take() {
            Some(url) => (url, false),
            None => (WS_URL.to_string(), true),
        };

        match session(&state, &token, &url, fresh).await {
            Ok(SessionEnd::Reconnect(url)) => {
                reconnect_url = Some(url);
                backoff = 1;
            }
            Ok(SessionEnd::Restart) => {
                log::info!("eventsub session dropped on request");
                reconnect_url = None;
                backoff = 1;
            }
            Ok(SessionEnd::Closed) => {
                set_link(&state, Link::Offline).await;
                backoff = 1;
                if !wait(&state, 1).await {
                    return;
                }
            }
            Err(err) => {
                log::warn!("eventsub: {err}");
                set_link(&state, Link::Offline).await;
                if !wait(&state, backoff).await {
                    return;
                }
                backoff = (backoff * 2).min(MAX_BACKOFF_SECS);
            }
        }
    }
}

async fn session(
    state: &Arc<ChatState>,
    token: &UserToken,
    url: &str,
    subscribe: bool,
) -> Result<SessionEnd, String> {
    let (stream, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|err| format!("cannot connect to {url}: {err}"))?;
    let (mut sink, mut source) = stream.split();
    log::debug!("eventsub connected to {url}");

    loop {
        let message = tokio::select! {
            _ = state.shutdown.cancelled() => return Ok(SessionEnd::Closed),
            _ = state.reconnect.notified() => return Ok(SessionEnd::Restart),
            next = source.next() => next,
        };

        let Some(message) = message else {
            return Ok(SessionEnd::Closed);
        };

        let text = match message.map_err(|err| format!("websocket error: {err}"))? {
            Ws::Text(text) => text,
            Ws::Close(frame) => {
                log::info!("eventsub closed: {frame:?}");
                return Ok(SessionEnd::Closed);
            }
            Ws::Ping(payload) => {
                sink.send(Ws::Pong(payload))
                    .await
                    .map_err(|err| format!("cannot answer ping: {err}"))?;
                continue;
            }
            _ => continue,
        };

        state.touch_event();
        let data = Event::parse_websocket(text.as_str())
            .map_err(|err| format!("cannot parse payload: {err}"))?;

        match data {
            EventsubWebsocketData::Welcome { payload, .. } => {
                let session_id = payload.session.id.to_string();
                log::info!("eventsub session {session_id}");
                set_link(state, Link::Connected).await;

                if subscribe {
                    match subscribe_all(state, &session_id).await {
                        Ok(count) => {
                            log::info!("subscribed to {count} event types");
                            crate::auth::push_notice(state, "Connected to chat".to_string());
                        }
                        Err(err) => log::error!("eventsub: {err}"),
                    }
                } else {
                    crate::auth::push_notice(state, "Reconnected to chat".to_string());
                }
            }
            EventsubWebsocketData::Keepalive { .. } => {}
            EventsubWebsocketData::Notification { payload, .. } => {
                on_event(state, token, payload).await;
            }
            EventsubWebsocketData::Reconnect { payload, .. } => {
                if let Some(url) = payload.session.reconnect_url {
                    log::info!("eventsub asked to reconnect");
                    return Ok(SessionEnd::Reconnect(url.to_string()));
                }
            }
            EventsubWebsocketData::Revocation { .. } => {
                log::warn!("eventsub revoked a subscription, signing in again may help");
            }
            _ => {}
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
    let mut count = 0;

    subscribe(
        state,
        token,
        &transport,
        ChannelChatMessageV1::new(broadcaster.clone(), user.clone()),
    )
    .await?;
    subscribe(
        state,
        token,
        &transport,
        ChannelChatClearV1::new(broadcaster.clone(), user.clone()),
    )
    .await?;
    subscribe(
        state,
        token,
        &transport,
        ChannelChatClearUserMessagesV1::new(broadcaster.clone(), user.clone()),
    )
    .await?;
    subscribe(
        state,
        token,
        &transport,
        ChannelChatMessageDeleteV1::new(broadcaster.clone(), user.clone()),
    )
    .await?;
    subscribe(
        state,
        token,
        &transport,
        ChannelChatNotificationV1::new(broadcaster.clone(), user.clone()),
    )
    .await?;
    count += 5;

    let follow = ChannelFollowV2::new(broadcaster.clone(), user.clone());
    if subscribe(state, token, &transport, follow).await.is_ok() {
        count += 1;
    }

    let points = ChannelPointsCustomRewardRedemptionAddV1::broadcaster_user_id(broadcaster.clone());
    if subscribe(state, token, &transport, points).await.is_ok() {
        count += 1;
    }

    let cheer = ChannelCheerV1::broadcaster_user_id(broadcaster.clone());
    if subscribe(state, token, &transport, cheer).await.is_ok() {
        count += 1;
    }

    let raid = ChannelRaidV1::to_broadcaster_user_id(broadcaster.clone());
    if subscribe(state, token, &transport, raid).await.is_ok() {
        count += 1;
    }

    let subscribe_event = ChannelSubscribeV1::broadcaster_user_id(broadcaster);
    if subscribe(state, token, &transport, subscribe_event)
        .await
        .is_ok()
    {
        count += 1;
    }

    Ok(count)
}

async fn subscribe<S>(
    state: &Arc<ChatState>,
    token: &UserToken,
    transport: &Transport,
    subscription: S,
) -> Result<(), String>
where
    S: twitch_api::eventsub::EventSubscription + Send,
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

async fn set_link(state: &Arc<ChatState>, link: Link) {
    let mut current = state.eventsub.write().await;
    if *current != link {
        log::info!("eventsub status: {link:?}");
        *current = link;
    }
}

async fn wait(state: &Arc<ChatState>, secs: u64) -> bool {
    tokio::select! {
        _ = state.shutdown.cancelled() => false,
        _ = tokio::time::sleep(Duration::from_secs(secs)) => true,
    }
}
