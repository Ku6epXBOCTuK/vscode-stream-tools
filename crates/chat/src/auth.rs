use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use serde::Deserialize;
use serde_json::json;
use twitch_oauth2::{
    AccessToken, ClientId, ClientSecret, RefreshToken, Scope, TwitchToken, UserToken,
    UserTokenBuilder,
};
use url::Url;

use crate::ChatState;
use crate::clock;
use crate::config::{self, ChatConfig};

pub const CALLBACK_PATH: &str = "/chat/auth/callback";
const MAX_PENDING_LOGINS: usize = 5;
const REFRESH_MARGIN: Duration = Duration::from_secs(600);
const REFRESH_CHECK_SECS: u64 = 60;
const REFRESH_RETRY_SECS: u64 = 300;

pub fn scopes() -> Vec<Scope> {
    vec![
        Scope::UserReadChat,
        Scope::UserWriteChat,
        Scope::ChannelReadRedemptions,
        Scope::ChannelReadSubscriptions,
        Scope::BitsRead,
        Scope::ModeratorReadFollowers,
        Scope::ModeratorManageChatMessages,
        Scope::ModeratorManageBannedUsers,
    ]
}

fn redirect_uri(port: u16) -> Url {
    Url::parse(&format!("http://localhost:{port}{CALLBACK_PATH}"))
        .expect("redirect uri is a compile-time constant apart from the port")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn page(status: StatusCode, title: &str, detail: &str) -> impl IntoResponse {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font-family:system-ui,sans-serif;margin:3rem auto;max-width:32rem;\
         line-height:1.5}}h1{{font-size:1.4rem}}</style></head><body><h1>{title}</h1>\
         <p>{detail}</p></body></html>",
        title = escape(title),
        detail = escape(detail)
    );
    (status, Html(html))
}

pub async fn url(State(state): State<Arc<ChatState>>) -> axum::response::Response {
    let config = state.config.read().await.clone();

    if !is_configured(&config) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Put client_id and client_secret of your Twitch application into config.json.",
            })),
        )
            .into_response();
    }

    let mut builder = UserTokenBuilder::new(
        ClientId::new(config.client_id.trim().to_string()),
        ClientSecret::new(config.client_secret.trim().to_string()),
        redirect_uri(state.port),
    );
    builder = builder.set_scopes(scopes()).force_verify(true);

    let (url, _) = builder.generate_url();
    {
        let mut pending = state.pending_login.lock().await;
        pending.push(builder);
        while pending.len() > MAX_PENDING_LOGINS {
            pending.remove(0);
        }
    }

    log::info!("sign-in url generated");
    Json(json!({ "url": url.to_string() })).into_response()
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

pub async fn callback(
    State(state): State<Arc<ChatState>>,
    Query(query): Query<CallbackQuery>,
) -> axum::response::Response {
    if let Some(error) = query.error {
        let detail = query.error_description.unwrap_or(error);
        log::warn!("sign-in refused by Twitch: {detail}");
        return page(StatusCode::BAD_REQUEST, "Sign-in was cancelled", &detail).into_response();
    }

    let (Some(code), Some(csrf)) = (query.code, query.state) else {
        return page(
            StatusCode::BAD_REQUEST,
            "Twitch did not send a code",
            "Start the sign-in again from VS Code.",
        )
        .into_response();
    };

    // NOTE: state is base64 with "+" in it; a query string parses that as a space
    let csrf = csrf.replace(' ', "+");

    let builder = {
        let mut pending = state.pending_login.lock().await;
        pending
            .iter()
            .position(|builder| builder.csrf_is_valid(&csrf))
            .map(|index| pending.remove(index))
    };

    let Some(builder) = builder else {
        log::warn!("sign-in callback with an unknown state");
        return page(
            StatusCode::BAD_REQUEST,
            "This sign-in link is stale",
            "The sign-in was started more than once or the server restarted. Start it again from VS Code.",
        )
        .into_response();
    };

    let token = match builder.get_user_token(&state.http, &csrf, &code).await {
        Ok(token) => token,
        Err(err) => {
            let detail = err.to_string();
            log::error!("sign-in failed: {detail}");
            return page(StatusCode::BAD_REQUEST, "Sign-in failed", &detail).into_response();
        }
    };

    let login = token.login.to_string();

    store_token(&state, &token).await;

    *state.token.write().await = Some(token);
    log::info!("signed in as {login}");
    push_notice(&state, format!("Signed in as {login}"));

    page(
        StatusCode::OK,
        "Signed in",
        &format!("Signed in as {login}. You can close this tab and return to VS Code."),
    )
    .into_response()
}

pub async fn status(State(state): State<Arc<ChatState>>) -> Json<serde_json::Value> {
    let config = state.config.read().await.clone();
    let pending = !state.pending_login.lock().await.is_empty();
    let token = state.token.read().await.clone();
    let authenticated = token.as_ref().is_some_and(|token| !token.is_elapsed());

    let state_name = if authenticated {
        "authorized"
    } else if pending {
        "pending"
    } else {
        "missing"
    };

    Json(json!({
        "state": state_name,
        "authenticated": authenticated,
        "login": if authenticated { config.tokens.login } else { String::new() },
        "client_id": !config.client_id.is_empty(),
    }))
}

pub fn push_notice(state: &ChatState, text: String) {
    let channel = state
        .config
        .try_read()
        .map(|config| config.channel.clone())
        .unwrap_or_default();
    let user = crate::chat::User {
        login: "twitch-chat".to_string(),
        name: "twitch-chat".to_string(),
        ..crate::chat::User::default()
    };
    let mut message = crate::chat::ChatMessage::new(channel, "notice", user, text);
    message.id = format!("{}{}", crate::chat::LOCAL_ID_PREFIX, message.ts);
    state.push(message);
}

async fn store_token(state: &ChatState, token: &UserToken) {
    let mut config = state.config.read().await.clone();
    config.tokens = to_config(token);
    *state.config.write().await = config;
    state.persist_async().await;
}

fn to_config(token: &UserToken) -> config::Tokens {
    config::Tokens {
        access: token.access_token.secret().to_string(),
        refresh: token
            .refresh_token
            .as_ref()
            .map(|refresh| refresh.secret().to_string())
            .unwrap_or_default(),
        expires_at: clock::now_unix() + token.expires_in().as_secs(),
        login: token.login.to_string(),
        user_id: token.user_id.to_string(),
    }
}

pub async fn restore(state: &Arc<ChatState>) -> Result<bool, String> {
    let config = state.config.read().await.clone();

    if !config.tokens.is_authenticated() {
        return Ok(false);
    }

    if config.client_id.trim().is_empty() {
        return Err("client_id is missing from config.json".to_string());
    }

    let client_id = ClientId::new(config.client_id.trim().to_string());
    let client_secret = ClientSecret::new(config.client_secret.trim().to_string());
    let access = AccessToken::new(config.tokens.access.clone());

    let token = if config.tokens.refresh.is_empty() {
        UserToken::from_existing(&state.http, access, None, Some(client_secret))
            .await
            .map_err(|err| err.to_string())?
    } else {
        UserToken::from_existing_or_refresh_token(
            &state.http,
            access,
            RefreshToken::new(config.tokens.refresh.clone()),
            client_id,
            Some(client_secret),
        )
        .await
        .map_err(|err| err.to_string())?
    };

    let refreshed = to_config(&token);
    let changed = refreshed != config.tokens;

    if changed {
        let mut updated = config.clone();
        updated.tokens = refreshed;
        *state.config.write().await = updated;
        state.persist_async().await;
    }

    log::info!("signed in as {}", token.login);
    *state.token.write().await = Some(token);
    Ok(true)
}

pub fn is_configured(config: &ChatConfig) -> bool {
    !config.client_id.trim().is_empty() && !config.client_secret.trim().is_empty()
}

pub fn needs_refresh(remaining: Duration) -> bool {
    remaining <= REFRESH_MARGIN
}

pub async fn refresh_if_needed(state: &Arc<ChatState>) -> Result<(), String> {
    let Some(token) = state.token.read().await.clone() else {
        return Ok(());
    };
    if !needs_refresh(token.expires_in()) {
        return Ok(());
    }
    refresh(state, "the token is about to expire")
        .await
        .map(|_| ())
}

pub async fn refresh(state: &Arc<ChatState>, reason: &str) -> Result<UserToken, String> {
    let Some(before) = state.token.read().await.clone() else {
        return Err("not signed in".to_string());
    };

    let _guard = state.refresh_lock.lock().await;
    let Some(mut token) = state.token.read().await.clone() else {
        return Err("not signed in".to_string());
    };
    if token.token().secret() != before.token().secret() {
        return Ok(token);
    }

    log::info!("refreshing the token: {reason}");
    if let Err(err) = token.refresh_token(&state.http).await {
        log::error!("cannot refresh the token: {err:?}");
        return Err(format!("cannot refresh the token: {err}"));
    }

    store_token(state, &token).await;

    *state.token.write().await = Some(token.clone());
    log::info!(
        "token refreshed, {} seconds left",
        token.expires_in().as_secs()
    );

    Ok(token)
}

pub fn spawn_refresher(state: Arc<ChatState>) {
    tokio::spawn(async move {
        let mut delay = Duration::ZERO;

        loop {
            tokio::select! {
                _ = state.shutdown.cancelled() => return,
                _ = tokio::time::sleep(delay) => {}
            }

            delay = match refresh_if_needed(&state).await {
                Ok(()) => Duration::from_secs(REFRESH_CHECK_SECS),
                Err(_) => Duration::from_secs(REFRESH_RETRY_SECS),
            };
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_refreshed_before_it_expires() {
        assert!(needs_refresh(Duration::ZERO));
        assert!(needs_refresh(REFRESH_MARGIN));
        assert!(!needs_refresh(REFRESH_MARGIN + Duration::from_secs(1)));
        assert!(!needs_refresh(Duration::from_secs(3600)));
    }
}
