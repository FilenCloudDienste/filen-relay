use std::{
	collections::HashMap,
	fmt::Display,
	sync::{Arc, OnceLock},
};

pub(crate) static ADMIN_EMAIL: OnceLock<String> = OnceLock::new();

use dioxus::{
	fullstack::extract::{FromRequestParts, Request},
	prelude::*,
	server::{
		axum::{self, middleware::Next},
		http::request::Parts,
	},
};
use filen_sdk_rs::auth::{http::ClientConfig, unauth::UnauthClient, Client};
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};
use tower_sessions::Session;

use crate::{api::LoginStatus, backend::db::DB};

// todo: improve this whole module?

static SESSION_TOKEN_KEY: &str = "session_token";

pub(crate) fn initialize_session_manager(router: axum::Router) -> axum::Router {
	router
		.layer(axum::middleware::from_fn(
			move |session: Session, mut req: Request, next: Next| async move {
				let session_token = match session.get(SESSION_TOKEN_KEY).await.unwrap_or_default() {
					Some(token) => token,
					None => {
						let token = SessionToken::new();
						session
							.insert(SESSION_TOKEN_KEY, token.clone())
							.await
							.unwrap();
						token
					}
				};
				req.extensions_mut().insert(session_token);
				next.run(req).await
			},
		))
		.layer(tower_sessions::SessionManagerLayer::new(
			tower_sessions::MemoryStore::default(),
		))
}

static SESSIONS: LazyLock<Mutex<HashMap<SessionToken, AuthSession>>> =
	LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct SessionToken(String);

impl SessionToken {
	fn new() -> Self {
		Self(uuid::Uuid::new_v4().to_string())
	}
}

impl Display for SessionToken {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}

#[derive(Clone)]
pub(crate) struct AuthSession {
	pub filen_email: String,
	pub filen_client: Arc<Client>,
	pub is_admin: bool,
}

impl<S> FromRequestParts<S> for AuthSession
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
					.get(token)
					.cloned()
					.ok_or_else(|| anyhow::anyhow!("Invalid session token"))
					.ok()
			})
			.ok_or(StatusCode::UNAUTHORIZED)
	}
}

pub(crate) async fn login_and_get_session_token(
	session: Session,
	email: String,
	password: String,
	two_factor_code: Option<String>,
) -> anyhow::Result<LoginStatus> {
	use filen_sdk_rs::ErrorKind;
	use filen_types::error::ResponseError;
	match UnauthClient::from_config(ClientConfig::default())?
		.login(
			email.clone(),
			&password,
			two_factor_code.as_deref().unwrap_or("XXXXXX"),
		)
		.await
	{
		Err(e) if e.kind() == ErrorKind::Server => match e.downcast::<ResponseError>() {
			Ok(ResponseError::ApiError { code, .. }) => {
				if code.as_deref() == Some("enter_2fa") {
					Ok(LoginStatus::TwoFactorRequired)
				} else if code.as_deref() == Some("email_or_password_wrong") {
					Ok(LoginStatus::InvalidCredentials)
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
		Ok(client) => {
			let allowed_users = DB
				.get_allowed_users()
				.map_err(|e| anyhow::anyhow!("Failed to get allowed users from database: {}", e))?;
			let is_admin = ADMIN_EMAIL.get() == Some(&email);
			let is_wildcard = allowed_users.contains(&"*".to_string());
			if is_admin || is_wildcard || allowed_users.contains(&email) {
				let session_token = session
					.get(SESSION_TOKEN_KEY)
					.await?
					.ok_or(anyhow::anyhow!("Session token not found"))?;
				SESSIONS.lock().unwrap().insert(
					session_token,
					AuthSession {
						filen_email: email,
						filen_client: Arc::new(client),
						is_admin,
					},
				);
				Ok(LoginStatus::LoggedIn)
			} else {
				Err(anyhow::anyhow!("User is not allowed"))
			}
		}
	}
}
