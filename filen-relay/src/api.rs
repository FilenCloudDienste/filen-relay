use std::sync::{Mutex, OnceLock};

use crate::common::{Share, ShareId};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
use crate::backend::{auth, db::DB};

#[derive(Serialize, Deserialize)]
pub(crate) struct User {
	pub email: String,
	pub is_admin: bool,
}

#[get("/api/admin")]
pub(crate) async fn get_admin_email() -> Result<String, anyhow::Error> {
	crate::backend::auth::ADMIN_EMAIL
		.get()
		.cloned()
		.ok_or_else(|| anyhow::anyhow!("Admin email not set"))
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
	UserNotAllowed { only_admin_allowed: bool },
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

#[post("/api/logout", session: tower_sessions::Session)]
pub(crate) async fn logout() -> Result<(), anyhow::Error> {
	auth::logout(session).await?;
	Ok(())
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CheckedShareRoot {
	pub path: String,
	pub item_type: ShareRootType,
}

#[derive(Serialize, Deserialize)]
pub(crate) enum ShareRootType {
	Root,
	File,
	Dir,
}

#[post("/api/checkRoot", session: auth::AuthSession)]
pub(crate) async fn check_and_transform_root(
	root: String,
) -> Result<CheckedShareRoot, anyhow::Error> {
	use super::backend::util::{find_path_for_dir, find_path_for_file};
	use filen_sdk_rs::fs::categories::NonRootFileType;
	use filen_types::fs::UuidStr;

	// check if it is a uuid
	if let Ok(uuid) = uuid::Uuid::try_parse(&root) {
		dioxus::logger::tracing::info!("Checking root as UUID: {}", uuid);
		// try to find a dir with the uuid
		match session.filen_client.get_dir(UuidStr::from(&uuid)).await {
			Ok(dir) => {
				let path = find_path_for_dir(session.filen_client.as_ref(), dir).await?;
				Ok(CheckedShareRoot {
					path,
					item_type: ShareRootType::Dir,
				})
			}
			Err(e1) => {
				// try to find a file with the uuid
				match session.filen_client.get_file(UuidStr::from(&uuid)).await {
					Ok(file) => {
						let path = find_path_for_file(session.filen_client.as_ref(), file).await?;
						Ok(CheckedShareRoot {
							path,
							item_type: ShareRootType::File,
						})
					}
					Err(e2) => Err(anyhow::anyhow!(
						"Failed to find dir ({}), also failed to find file ({}), for provided UUID",
						e1,
						e2
					)),
				}
			}
		}
	} else {
		let root = format!("/{}", root.trim_start_matches('/').trim_end_matches('/'));
		// not a uuid, try to find a dir with the path
		match session.filen_client.find_item_at_path(&root).await {
            Ok(item) => match item {
                Some(item) => match item {
                    NonRootFileType::Root(_) => Ok(CheckedShareRoot {
                        path: root,
                        item_type: ShareRootType::Root,
                    }),
                    NonRootFileType::File(_) => Ok(CheckedShareRoot {
                        path: root,
                        item_type: ShareRootType::File,
                    }),
                    NonRootFileType::Dir(_) => Ok(CheckedShareRoot {
                        path: root,
                        item_type: ShareRootType::Dir,
                    }),
                }
                None => Err(anyhow::anyhow!("No item found at provided path")),
            },
            Err(e) => Err(anyhow::anyhow!(
                "Failed to find dir at path ({}), also failed to find file at path ({}), for provided path",
                e,
                e
            )),
        }
	}
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
	let root = check_and_transform_root(root)
		.await
		.context("Failed to check root")?
		.path;
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

pub(crate) static SKIP_UPDATE_CHECKER: OnceLock<bool> = OnceLock::new();

#[derive(Clone)]
struct LastUpdateCheck {
	timestamp: std::time::Instant,
	latest_version: String,
}
static LAST_UPDATE_CHECK: Mutex<Option<LastUpdateCheck>> = Mutex::new(None);

#[cfg(feature = "server")]
async fn check_for_updates_cached() -> Result<String, anyhow::Error> {
	let cached_last_check = LAST_UPDATE_CHECK
		.lock()
		.unwrap()
		.clone()
		.and_then(|last_check| {
			if last_check.timestamp.elapsed() < std::time::Duration::from_mins(10) {
				Some(last_check.clone())
			} else {
				None
			}
		});
	if let Some(last_check) = cached_last_check {
		Ok(last_check.latest_version)
	} else {
		let latest_version = get_latest_version().await?;
		*LAST_UPDATE_CHECK.lock().unwrap() = Some(LastUpdateCheck {
			timestamp: std::time::Instant::now(),
			latest_version: latest_version.clone(),
		});
		Ok(latest_version)
	}
}

#[cfg(feature = "server")]
async fn get_latest_version() -> Result<String, anyhow::Error> {
	use dioxus::fullstack::reqwest;

	let response = reqwest::Client::new()
		.get("https://api.github.com/repos/FilenCloudDienste/filen-relay/releases/latest")
		.header(reqwest::header::USER_AGENT, "filen-relay-deployer")
		.send()
		.await?;
	let latest_version = response
		.json::<serde_json::Value>()
		.await?
		.get("tag_name")
		.and_then(|t| t.as_str())
		.map(|s| s.to_string())
		.ok_or_else(|| anyhow::anyhow!("Failed to get tag_name from GitHub API response"))?;
	Ok(latest_version)
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct AvailableUpdate {
	pub current_version: String,
	pub latest_version: String,
}

#[get("/api/checkForUpdates")]
pub(crate) async fn check_for_updates() -> Result<Option<AvailableUpdate>, anyhow::Error> {
	if SKIP_UPDATE_CHECKER.get().cloned().unwrap_or_default() {
		return Ok(None);
	}
	let latest_version = check_for_updates_cached()
		.await
		.context("Failed to check for updates")?;
	let current_version = env!("CARGO_PKG_VERSION").to_string();
	if latest_version != current_version {
		Ok(Some(AvailableUpdate {
			current_version,
			latest_version,
		}))
	} else {
		Ok(None)
	}
}
