//! We use the --auth-proxy option of rclone serve commands to generate backends on-the-fly.
//! In our case, that auth proxy is a script that calls the executable itself again, but in a
//! special mode set by a flag, so it behaves as that auth script.
//! To fetch information about shares etc, it communicates with the main process via
//! HTTP calls to endpoints secured by an internal auth token.

use anyhow::{Context, Result};
use base64::{prelude::BASE64_STANDARD, Engine as _};
use filen_sdk_rs::auth::{http::ClientConfig, unauth::UnauthClient, StringifiedClient};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::common::{ServerType, Share};

static ACT_AS_RCLONE_AUTH_PROXY_ARG: &str = "--act-as-rclone-auth-proxy-with-config=";

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct RcloneAuthProxyOptions {
	pub(crate) self_port: u16,
	pub(crate) self_authentication_token: String,
	pub(crate) server_type: ServerType,
}

impl RcloneAuthProxyOptions {
	pub(crate) fn is_rclone_auth_proxy_mode() -> bool {
		std::env::args().any(|arg| arg.starts_with(ACT_AS_RCLONE_AUTH_PROXY_ARG))
	}

	fn read_from_args() -> Result<Self> {
		let str = std::env::args()
			.find_map(|arg| {
				arg.strip_prefix(ACT_AS_RCLONE_AUTH_PROXY_ARG)
					.map(|config_str| config_str.to_owned())
			})
			.context(
				"Failed to find or parse Rclone auth proxy options from command line arguments",
			)?;
		BASE64_STANDARD
			.decode(str)
			.context("Failed to decode base64 Rclone auth proxy options")
			.and_then(|decoded| {
				serde_json::from_slice(&decoded)
					.context("Failed to deserialize Rclone auth proxy options from JSON string")
			})
	}

	pub(crate) fn construct_args(&self) -> String {
		let json =
			serde_json::to_string(self).expect("Failed to serialize Rclone auth proxy options");
		let encoded = BASE64_STANDARD.encode(json);
		format!("{}{}", ACT_AS_RCLONE_AUTH_PROXY_ARG, encoded)
	}
}

mod rclone_auth_proxy_io {
	//! Rclone interfaces with the auth proxy by writing JSON to its stdin and reading JSON from its stdout

	use anyhow::{Context, Result};
	use serde::Deserialize;
	use std::io::Read;

	#[derive(Deserialize)]
	pub(super) struct AuthProxyInput {
		/// For WebDAV, this is the Basic auth username.
		/// For S3, this is a hash of the Access Key ID (like username), and therefore not very useful.
		#[serde(rename = "user")]
		pub username: String,
		/// For S3, this is the Access Key ID (like username), see [`write_output`] for how to verify password.
		/// Otherwise, it's the password.
		#[serde(rename = "pass")]
		pub password: String,
	}

	pub(super) fn read_input() -> Result<AuthProxyInput> {
		let mut input = String::new();
		std::io::stdin().read_to_string(&mut input)?;
		serde_json::from_str(&input)
			.context(format!("Failed to deserialize JSON string: {}", input))
	}

	/// For S3: When you specify the `s3_secret_access_key_to_verify` parameter, a flag will be set
	/// that causes Rclone to verify the S3 Authorization header's signature to match the expected
	/// Secret Access Key (because Rclone can't provide the Secret Access Key to the auth proxy for
	/// technical reasons, only verify it).
	/// This flag is only available with our customizations to filen-rclone. // todo
	pub(super) fn write_output(
		rclone_remote_config: &serde_json::Value,
		s3_secret_access_key_to_verify: Option<&str>,
	) -> Result<()> {
		let mut rclone_remote_config = rclone_remote_config.clone();
		if let Some(s3_secret_access_key) = s3_secret_access_key_to_verify {
			rclone_remote_config["_s3_secret_access_key_to_verify"] = s3_secret_access_key.into();
		}
		log::info!("Writing output: {}", rclone_remote_config);
		println!("{}", rclone_remote_config); // todo check this works
		Ok(())
	}
}

// will be called when executable is called as the rclone auth proxy
pub(crate) async fn rclone_auth_proxy_main() -> Result<()> {
	let options = RcloneAuthProxyOptions::read_from_args()?;
	let input = rclone_auth_proxy_io::read_input()?;
	log::info!(
		"Rclone auth proxy process called with options: {:?} and username: {}",
		options,
		input.username
	);
	let (rclone_remote_config, s3_secret_access_key_to_verify) =
		rclone_auth_proxy_main_inner(options, input).await?;
	rclone_auth_proxy_io::write_output(
		&rclone_remote_config,
		s3_secret_access_key_to_verify.as_deref(),
	)?;
	Ok(())
}

async fn rclone_auth_proxy_main_inner(
	options: RcloneAuthProxyOptions,
	input: rclone_auth_proxy_io::AuthProxyInput,
) -> Result<(serde_json::Value, Option<String>)> {
	let share_id = match options.server_type {
		ServerType::Http => input.username.clone(),
		ServerType::Webdav => input.username.clone(),
		ServerType::S3 => input.password.clone(),
	};
	let Some(share) = rclone_auth_proxy_ipc::get_share_info_from_server_process(
		options.self_port,
		&options.self_authentication_token,
		&share_id,
	)
	.await?
	else {
		log::error!("No share found for id {}", share_id);
		return Ok((json!({"type": "not_found"}), None));
		// todo: is there a better way to indicate errors?
	};
	let s3_secret_access_key_to_verify = if options.server_type == ServerType::S3 {
		share.password.clone()
	} else {
		if let Some(password) = share.password.as_deref() {
			if input.password != password {
				rclone_auth_proxy_io::write_output(&json!({"type": "wrong_password"}), None)?;
				return Ok((json!({"type": "wrong_password"}), None));
			}
		}
		None
	};
	Ok((
		construct_rclone_remote_config_for_share(&share)?,
		s3_secret_access_key_to_verify,
	))
}

pub(crate) mod rclone_auth_proxy_ipc {
	//! Handles communication between the Rclone auth proxy and the main server process via HTTP.

	use axum_core::{body::Body, extract::Request, response::Response};
	use http::StatusCode;
	use rand::distr::SampleString as _;
	use std::sync::OnceLock;

	use crate::common::{Share, ShareId};
	use anyhow::{Context, Result};
	use dioxus::{
		fullstack::{extract::Path, reqwest},
		server::axum,
	};

	/// This token lives in the server process, and is passed to the auth proxy via the [RcloneAuthProxyOptions].
	pub(crate) static RCLONE_AUTH_PROXY_SELF_AUTH_TOKEN: OnceLock<String> = OnceLock::new();

	pub(crate) fn get_rclone_auth_proxy_self_auth_token() -> String {
		RCLONE_AUTH_PROXY_SELF_AUTH_TOKEN
			.get_or_init(|| rand::distr::Alphanumeric.sample_string(&mut rand::rng(), 32))
			.to_string()
	}

	/// (called from auth proxy process)
	pub(crate) async fn get_share_info_from_server_process(
		self_port: u16,
		self_auth_token: &str,
		share_id: &str,
	) -> Result<Option<Share>> {
		match reqwest::Client::new()
			.get(format!(
				"http://127.0.0.1:{}/rclone-auth-proxy-ipc/share/{}",
				self_port, share_id
			))
			.bearer_auth(self_auth_token)
			.send()
			.await
		{
			Err(e) => {
				log::error!("Failed to send request to server process: {}", e);
				Err(anyhow::anyhow!(
					"Failed to send request to server process: {}",
					e
				))
			}
			Ok(response) => match response.status() {
				StatusCode::OK => response
					.json::<Share>()
					.await
					.context("Failed to parse JSON response from server process")
					.map(Some),
				StatusCode::NOT_FOUND => Ok(None),
				_ => Err(anyhow::anyhow!(
					"Received error status code {} from server process",
					response.status()
				)),
			},
		}
	}

	/// (called from server process)
	pub(crate) fn register_ipc_endpoint(router: axum::Router) -> axum::Router {
		router.route(
			"/rclone-auth-proxy-ipc/share/{share_id}",
			axum::routing::get(|Path(share_id): Path<String>, req: Request| async move {
				// auth
				let auth_header = match req.headers().get("Authorization") {
					Some(header) => header.to_str().unwrap_or(""),
					None => "",
				};
				let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
				if token != get_rclone_auth_proxy_self_auth_token() {
					return Response::builder().status(401).body(Body::empty()).unwrap();
				}

				// find share
				let shares = match crate::backend::db::DB.get_shares() {
					Ok(shares) => shares,
					Err(e) => {
						return Response::builder()
							.status(StatusCode::INTERNAL_SERVER_ERROR)
							.body(Body::from(format!(
								"Failed to get shares from database: {}",
								e
							)))
							.unwrap();
					}
				};
				let share_id: ShareId = share_id.into();
				let share = match shares.into_iter().find(|s| s.id == share_id) {
					Some(share) => share,
					None => {
						return Response::builder()
							.status(StatusCode::NOT_FOUND)
							.body(Body::from("Share not found"))
							.unwrap();
					}
				};

				Response::new(dioxus::fullstack::body::Body::from(
					serde_json::to_string(&share).unwrap(),
				))
			}),
		)
	}
}

fn construct_rclone_remote_config_for_share(share: &Share) -> Result<serde_json::Value> {
	let client = UnauthClient::from_config(ClientConfig::default())?
		.from_stringified(
			serde_json::from_str::<StringifiedClient>(&share.filen_stringified_client)
				.context("Failed to deserialize stringified client from share")?,
		)
		.context("Failed to create Filen client from share")?;
	// ref: filen-rclone-wrapper::rclone_installation::write_rclone_config
	let client = client.to_sdk_config();
	Ok(json!({
		"type": "filen",
		"_root": share.root,
		"_obscure": "password,api_key",
		"password": "INTERNAL",
		"email": client.email,
		"master_keys": client.master_keys.join("|"),
		"api_key": client.api_key,
		"public_key": client.public_key,
		"private_key": client.private_key,
		"auth_version": (client.auth_version as u8).to_string(),
		"base_folder_uuid": client.base_folder_uuid,
		"read_only": share.read_only.to_string(),
	}))
}
