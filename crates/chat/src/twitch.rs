use std::sync::Arc;

use axum::http::StatusCode;
use twitch_api::helix::{
    ClientRequestError, HelixRequestDeleteError, HelixRequestGetError, HelixRequestPostError,
    Request as HelixRequest, RequestDelete, RequestGet, RequestPost,
};
use twitch_oauth2::{TwitchToken, UserToken};

use crate::ChatState;
use crate::auth;

#[derive(Clone, Debug)]
pub enum TwitchError {
    SignIn,
    Status { status: StatusCode, message: String },
    Transport,
}

impl std::fmt::Display for TwitchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignIn => write!(f, "sign in to Twitch again"),
            Self::Status { status, message } => {
                write!(f, "Twitch rejected the request ({status}): {message}")
            }
            Self::Transport => write!(f, "Twitch is not reachable right now"),
        }
    }
}

pub async fn token(state: &Arc<ChatState>) -> Result<UserToken, TwitchError> {
    let _ = auth::refresh_if_needed(state).await;

    let Some(token) = state.token.read().await.clone() else {
        return Err(TwitchError::SignIn);
    };

    if token.is_elapsed() {
        return Err(TwitchError::SignIn);
    }

    Ok(token)
}

pub async fn get<R>(state: &Arc<ChatState>, request: R) -> Result<R::Response, TwitchError>
where
    R: RequestGet + Clone,
{
    let token = token(state).await?;

    match state.helix.req_get(request.clone(), &token).await {
        Ok(response) => Ok(response.data),
        Err(err) if rejected(&err) => {
            log::warn!("Twitch rejected the token, refreshing: {err}");
            let token = refresh(state).await?;
            state
                .helix
                .req_get(request, &token)
                .await
                .map(|response| response.data)
                .map_err(|err| classify(err, "GET", <R as HelixRequest>::PATH))
        }
        Err(err) => Err(classify(err, "GET", <R as HelixRequest>::PATH)),
    }
}

pub async fn post<R>(
    state: &Arc<ChatState>,
    request: R,
    body: R::Body,
) -> Result<R::Response, TwitchError>
where
    R: RequestPost + Clone,
    R::Body: Clone,
{
    let token = token(state).await?;

    match state
        .helix
        .req_post(request.clone(), body.clone(), &token)
        .await
    {
        Ok(response) => Ok(response.data),
        Err(err) if rejected(&err) => {
            log::warn!("Twitch rejected the token, refreshing: {err}");
            let token = refresh(state).await?;
            state
                .helix
                .req_post(request, body, &token)
                .await
                .map(|response| response.data)
                .map_err(|err| classify(err, "POST", <R as HelixRequest>::PATH))
        }
        Err(err) => Err(classify(err, "POST", <R as HelixRequest>::PATH)),
    }
}

pub async fn delete<R>(state: &Arc<ChatState>, request: R) -> Result<R::Response, TwitchError>
where
    R: RequestDelete + Clone,
{
    let token = token(state).await?;

    match state.helix.req_delete(request.clone(), &token).await {
        Ok(response) => Ok(response.data),
        Err(err) if rejected(&err) => {
            log::warn!("Twitch rejected the token, refreshing: {err}");
            let token = refresh(state).await?;
            state
                .helix
                .req_delete(request, &token)
                .await
                .map(|response| response.data)
                .map_err(|err| classify(err, "DELETE", <R as HelixRequest>::PATH))
        }
        Err(err) => Err(classify(err, "DELETE", <R as HelixRequest>::PATH)),
    }
}

fn retry_helps(status: StatusCode, message: &str) -> bool {
    status == StatusCode::UNAUTHORIZED && !message.to_ascii_lowercase().contains("missing scope")
}

fn rejected(err: &ClientRequestError<reqwest::Error>) -> bool {
    status_message(err).is_some_and(|(status, message)| retry_helps(status, &message))
}

fn status_message(err: &ClientRequestError<reqwest::Error>) -> Option<(StatusCode, String)> {
    match err {
        ClientRequestError::HelixRequestGetError(HelixRequestGetError::Error {
            status,
            message,
            ..
        })
        | ClientRequestError::HelixRequestPostError(HelixRequestPostError::Error {
            status,
            message,
            ..
        })
        | ClientRequestError::HelixRequestDeleteError(HelixRequestDeleteError::Error {
            status,
            message,
            ..
        }) => Some((*status, message.clone())),
        _ => None,
    }
}

fn classify(err: ClientRequestError<reqwest::Error>, method: &str, path: &str) -> TwitchError {
    log::error!("Twitch {method} {path} failed: {}", error_chain(&err));
    match status_message(&err) {
        Some((status, message)) => TwitchError::Status { status, message },
        None => TwitchError::Transport,
    }
}

fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

async fn refresh(state: &Arc<ChatState>) -> Result<UserToken, TwitchError> {
    auth::refresh(state, "Twitch rejected the token")
        .await
        .map_err(|_| TwitchError::SignIn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_token_is_worth_a_retry() {
        assert!(retry_helps(StatusCode::UNAUTHORIZED, "Invalid OAuth token"));
    }

    #[test]
    fn a_missing_scope_is_not_worth_a_retry() {
        assert!(!retry_helps(
            StatusCode::UNAUTHORIZED,
            "Missing scope: user:write:chat"
        ));
        assert!(!retry_helps(StatusCode::FORBIDDEN, "Invalid OAuth token"));
    }

    #[derive(Debug)]
    struct Cause;

    impl std::fmt::Display for Cause {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "connection reset")
        }
    }

    impl std::error::Error for Cause {}

    #[derive(Debug)]
    struct Failure(Cause);

    impl std::fmt::Display for Failure {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "request failed")
        }
    }

    impl std::error::Error for Failure {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn an_error_chain_keeps_the_underlying_cause() {
        assert_eq!(
            error_chain(&Failure(Cause)),
            "request failed: connection reset"
        );
    }
}
