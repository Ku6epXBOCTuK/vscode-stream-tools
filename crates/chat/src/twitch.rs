use std::future::Future;
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
    let path = <R as HelixRequest>::PATH;

    let result = with_retry(
        "GET",
        path,
        || state.helix.req_get(request.clone(), &token),
        is_transport,
        describe,
    )
    .await;

    match result {
        Ok(response) => Ok(response.data),
        Err(err) if rejected(&err) => {
            log::warn!("Twitch rejected the token, refreshing: {err}");
            let token = refresh(state).await?;
            with_retry(
                "GET",
                path,
                || state.helix.req_get(request.clone(), &token),
                is_transport,
                describe,
            )
            .await
            .map(|response| response.data)
            .map_err(|err| classify(err, "GET", path))
        }
        Err(err) => Err(classify(err, "GET", path)),
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
    let path = <R as HelixRequest>::PATH;

    let result = with_retry(
        "POST",
        path,
        || state.helix.req_post(request.clone(), body.clone(), &token),
        can_replay_post,
        describe,
    )
    .await;

    match result {
        Ok(response) => Ok(response.data),
        Err(err) if rejected(&err) => {
            log::warn!("Twitch rejected the token, refreshing: {err}");
            let token = refresh(state).await?;
            with_retry(
                "POST",
                path,
                || state.helix.req_post(request.clone(), body.clone(), &token),
                can_replay_post,
                describe,
            )
            .await
            .map(|response| response.data)
            .map_err(|err| classify(err, "POST", path))
        }
        Err(err) => Err(classify(err, "POST", path)),
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

const TRANSPORT_ATTEMPTS: usize = 2;

async fn with_retry<T, E, F, Fut>(
    method: &str,
    path: &str,
    mut call: F,
    retryable: fn(&E) -> bool,
    describe_error: fn(&E) -> String,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut failure = None;
    for attempt in 1..=TRANSPORT_ATTEMPTS {
        match call().await {
            Ok(value) => return Ok(value),
            Err(err) if attempt < TRANSPORT_ATTEMPTS && retryable(&err) => {
                log::warn!(
                    "Twitch {method} {path} attempt {attempt} failed: {}, retrying",
                    describe_error(&err)
                );
                failure = Some(err);
            }
            Err(err) => return Err(err),
        }
    }
    Err(failure.expect("a retry leaves the last failure behind"))
}

fn is_transport(err: &ClientRequestError<reqwest::Error>) -> bool {
    matches!(
        err,
        ClientRequestError::RequestError(_) | ClientRequestError::HyperError(_)
    )
}

fn can_replay_post(err: &ClientRequestError<reqwest::Error>) -> bool {
    matches!(
        err,
        ClientRequestError::RequestError(source) if source.is_dns() || source.is_connect()
    )
}

fn describe(err: &ClientRequestError<reqwest::Error>) -> String {
    error_chain(err)
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

    #[derive(Debug, PartialEq)]
    enum FakeError {
        Retry,
        Fail,
    }

    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Retry => write!(f, "temporary"),
                Self::Fail => write!(f, "permanent"),
            }
        }
    }

    fn fake_retryable(err: &FakeError) -> bool {
        matches!(err, FakeError::Retry)
    }

    fn fake_describe(err: &FakeError) -> String {
        err.to_string()
    }

    #[tokio::test]
    async fn a_retry_keeps_the_pending_request_alive() {
        let mut calls = 0;
        let result: Result<&str, FakeError> = with_retry(
            "POST",
            "/chat/messages",
            || {
                calls += 1;
                if calls == 1 {
                    std::future::ready(Err(FakeError::Retry))
                } else {
                    std::future::ready(Ok("sent"))
                }
            },
            fake_retryable,
            fake_describe,
        )
        .await;

        assert_eq!(result, Ok("sent"));
        assert_eq!(calls, 2);
    }

    #[tokio::test]
    async fn a_non_retryable_error_fails_immediately() {
        let mut calls = 0;
        let result: Result<(), FakeError> = with_retry(
            "POST",
            "/chat/messages",
            || {
                calls += 1;
                std::future::ready(Err(FakeError::Fail))
            },
            fake_retryable,
            fake_describe,
        )
        .await;

        assert!(matches!(result, Err(FakeError::Fail)));
        assert_eq!(calls, 1);
    }

    #[tokio::test]
    async fn a_retry_stops_after_two_attempts() {
        let mut calls = 0;
        let result: Result<(), FakeError> = with_retry(
            "GET",
            "/users",
            || {
                calls += 1;
                std::future::ready(Err(FakeError::Retry))
            },
            fake_retryable,
            fake_describe,
        )
        .await;

        assert!(matches!(result, Err(FakeError::Retry)));
        assert_eq!(calls, TRANSPORT_ATTEMPTS);
    }

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
