use crate::common::{Share, ShareId};
use dioxus::fullstack::response::Response;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
use crate::backend::{auth, db::DB};

#[derive(Serialize, Deserialize)]
pub(crate) struct User {
    pub email: String,
    pub is_admin: bool,
}

#[post("/api/user", session: auth::AuthSession)]
pub(crate) async fn get_user() -> Result<User> {
    Ok(User {
        email: session.filen_email,
        is_admin: session.is_admin,
    })
}

#[derive(Serialize, Deserialize)]
pub(crate) enum LoginStatus {
    InvalidCredentials,
    TwoFactorRequired,
    LoggedIn,
}

#[post("/api/login", session: tower_sessions::Session)]
pub(crate) async fn login(
    email: String,
    password: String,
    two_factor_code: Option<String>,
) -> Result<LoginStatus, anyhow::Error> {
    let login_status = auth::login_and_get_session_token(session, email, password, two_factor_code)
        .await
        .inspect_err(|e| dioxus::logger::tracing::error!("Login error: {}", e))?;
    Ok(login_status)
}

#[post("/api/logout")]
pub(crate) async fn logout() -> Result<Response> {
    use dioxus::fullstack::{body::Body, response::Response};
    Ok(Response::builder()
        .header("Set-Cookie", "Session=; HttpOnly; Path=/")
        .body(Body::empty())
        .unwrap())
}

#[get("/api/shares", session: auth::AuthSession)]
pub(crate) async fn get_shares() -> Result<Vec<Share>, anyhow::Error> {
    Ok(DB
        .get_shares()
        .map_err(|e| anyhow::anyhow!("Failed to get shares from database: {}", e))?
        .into_iter()
        .filter(|s| session.is_admin || s.filen_email == session.filen_email)
        .collect())
}

#[post("/api/shares/add", session: auth::AuthSession)]
pub(crate) async fn add_share(
    root: String,
    read_only: bool,
    password: Option<String>,
) -> Result<(), anyhow::Error> {
    let root = format!("/{}", root.trim_start_matches('/'));
    DB.create_share(&Share {
        id: ShareId::new(),
        root,
        read_only,
        password,
        filen_email: session.filen_email.clone(),
        filen_stringified_client: serde_json::to_string(&session.filen_client.to_stringified())?,
    })
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create share: {}", e))
}

#[post("/api/shares/remove", session: auth::AuthSession)]
pub(crate) async fn remove_share(id: ShareId) -> Result<(), anyhow::Error> {
    DB.get_shares()
        .map_err(|e| anyhow::anyhow!("Failed to get shares from database: {}", e))?
        .into_iter()
        .find(|s| s.id == id && (session.is_admin || s.filen_email == session.filen_email))
        .ok_or_else(|| anyhow::anyhow!("Share not found or unauthorized"))?;
    DB.delete_share(&id)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to remove share: {}", e))
}

#[get("/api/allowedUsers", session: auth::AuthSession)]
pub(crate) async fn get_allowed_users() -> Result<Vec<String>, anyhow::Error> {
    if !session.is_admin {
        return Err(anyhow::anyhow!("Unauthorized"));
    }
    DB.get_allowed_users()
        .map_err(|e| anyhow::anyhow!("Failed to get allowed users: {}", e))
}

#[post("/api/allowedUsers/add", session: auth::AuthSession)]
pub(crate) async fn add_allowed_user(email: String) -> Result<(), anyhow::Error> {
    if !session.is_admin {
        return Err(anyhow::anyhow!("Unauthorized"));
    }
    DB.add_allowed_user(&email)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to add allowed user: {}", e))
}

#[post("/api/allowedUsers/remove", session: auth::AuthSession)]
pub(crate) async fn remove_allowed_user(email: String) -> Result<(), anyhow::Error> {
    if !session.is_admin {
        return Err(anyhow::anyhow!("Unauthorized"));
    }
    DB.remove_allowed_user(&email)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to remove allowed user: {}", e))
}

#[post("/api/allowedUsers/clear", session: auth::AuthSession)]
pub(crate) async fn clear_allowed_users() -> Result<(), anyhow::Error> {
    if !session.is_admin {
        return Err(anyhow::anyhow!("Unauthorized"));
    }
    DB.clear_allowed_users()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to clear allowed users: {}", e))
}
