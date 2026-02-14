use std::{fmt::Display, sync::OnceLock};

pub(crate) static ADMIN_EMAIL: OnceLock<String> = OnceLock::new();

use dioxus::{
    fullstack::extract::{FromRequestParts, Request},
    prelude::*,
    server::{
        axum::{self, middleware::Next},
        http::request::Parts,
    },
};
use filen_sdk_rs::auth::Client;
use std::sync::{LazyLock, Mutex};

use crate::backend::db::DB;

static SESSIONS: LazyLock<Mutex<Vec<Session>>> = LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Clone, PartialEq)]
pub(crate) struct SessionToken(String);

impl Display for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone)]
pub(crate) struct Session {
    pub token: SessionToken,
    pub filen_email: String,
    pub filen_password: String,
    pub filen_2fa_code: Option<String>,
    pub is_admin: bool,
}

/// Axum middleware to extract session token from cookies
pub(crate) async fn middleware_extract_session_token(
    mut request: Request,
    next: Next,
) -> axum::http::Response<axum::body::Body> {
    if let Some(cookies) = request.headers().get("Cookie") {
        let token = cookies
            .to_str()
            .unwrap_or("")
            .split(';')
            .find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                if name == "Session" {
                    Some(value.to_string())
                } else {
                    None
                }
            });
        if let Some(token) = token {
            request.extensions_mut().insert(SessionToken(token));
        }
    }
    next.run(request).await
}

impl<S> FromRequestParts<S> for Session
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<SessionToken>()
            .and_then(|token| {
                SESSIONS
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|s| s.token == *token)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Invalid session token"))
                    .ok()
            })
            .ok_or(StatusCode::UNAUTHORIZED)
    }
}

pub(crate) async fn authenticate_filen_client(
    email: String,
    password: &str,
    two_factor_code: Option<String>,
) -> Result<Client, anyhow::Error> {
    use filen_sdk_rs::ErrorKind;
    use filen_types::error::ResponseError;
    match Client::login(
        email.clone(),
        password,
        two_factor_code.as_deref().unwrap_or("XXXXXX"),
    )
    .await
    {
        Err(e) if e.kind() == ErrorKind::Server => match e.downcast::<ResponseError>() {
            Ok(ResponseError::ApiError { code, .. }) => {
                if code.as_deref() == Some("enter_2fa") {
                    Err(anyhow::anyhow!("2FA required"))
                } else if code.as_deref() == Some("email_or_password_wrong") {
                    Err(anyhow::anyhow!("Email or password wrong"))
                } else {
                    Err(anyhow::anyhow!(
                        "Failed to log in (code {})",
                        code.as_deref().unwrap_or("")
                    ))
                }
            }
            _ => Err(anyhow::anyhow!("Failed to log in")),
        },
        Err(e) => Err(anyhow::anyhow!("Failed to log in: {}", e)),
        Ok(client) => Ok(client),
    }
}

pub(crate) async fn login_and_get_session_token(
    email: String,
    password: String,
    two_factor_code: Option<String>,
) -> anyhow::Result<SessionToken> {
    match authenticate_filen_client(email.clone(), &password, two_factor_code.clone()).await {
        Err(e) => Err(e.context("Failed to log in")),
        Ok(_client) => {
            let allowed_users = DB
                .get_allowed_users()
                .map_err(|e| anyhow::anyhow!("Failed to get allowed users from database: {}", e))?;
            let is_admin = ADMIN_EMAIL.get() == Some(&email);
            let is_wildcard = allowed_users.contains(&"*".to_string());
            if is_admin || is_wildcard || allowed_users.contains(&email) {
                let token = SessionToken(uuid::Uuid::new_v4().to_string());
                SESSIONS.lock().unwrap().push(Session {
                    token: token.clone(),
                    filen_email: email.to_string(),
                    filen_password: password,
                    filen_2fa_code: two_factor_code,
                    is_admin,
                });
                Ok(token)
            } else {
                Err(anyhow::anyhow!("User is not allowed"))
            }
        }
    }
}
