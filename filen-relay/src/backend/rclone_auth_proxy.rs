use std::io::Read;

use anyhow::{Context, Result};
use base64::{prelude::BASE64_STANDARD, Engine};
use filen_sdk_rs::auth::{http::ClientConfig, unauth::UnauthClient, StringifiedClient};
use serde::Deserialize;
use serde_json::json;

use crate::common::Share;

pub(crate) static ACT_AS_RCLONE_AUTH_PROXY_ARG: &str = "--act-as-rclone-auth-proxy";

#[derive(Deserialize)]
struct RcloneAuthProxyInput {
	user: String,
}

// will be called when executable is in role of rclone auth proxy
pub(crate) fn rclone_auth_proxy_main() -> Result<()> {
	// extract share id
	let mut input = String::new();
	std::io::stdin().read_to_string(&mut input).ok().unwrap();
	let rclone_auth_proxy_input: RcloneAuthProxyInput = serde_json::from_str(&input)
		.context(format!("Failed to deserialize JSON string: {}", input))?;

	// decode rclone remote config for share
	let rclone_remote_config = decode_rclone_remote_config(&rclone_auth_proxy_input.user)?;
	log::debug!("Rclone remote config for: {}", rclone_remote_config);

	println!("{}", rclone_remote_config);
	Ok(())
}

pub(crate) fn get_encoded_rclone_remote_config_for_share(share: &Share) -> Result<String> {
	let config = construct_rclone_remote_config_for_share(share)?;
	Ok(BASE64_STANDARD.encode(config))
}

fn decode_rclone_remote_config(encoded_config: &str) -> Result<String> {
	let decoded_bytes = BASE64_STANDARD
		.decode(encoded_config)
		.context("Failed to decode base64 encoded Rclone remote config")?;
	let decoded_str = String::from_utf8(decoded_bytes)
		.context("Failed to convert decoded Rclone remote config bytes to string")?;
	Ok(decoded_str)
}

fn construct_rclone_remote_config_for_share(share: &Share) -> Result<String> {
	let client = UnauthClient::from_config(ClientConfig::default())?
		.from_stringified(
			serde_json::from_str::<StringifiedClient>(&share.filen_stringified_client)
				.context("Failed to deserialize stringified client from share")?,
		)
		.context("Failed to create Filen client from share")?;
	// ref: filen-rclone-wrapper::rclone_installation::write_rclone_config
	let client = client.to_sdk_config();
	let str = serde_json::to_string(&json!({
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
	.context("Failed to serialize Rclone remote config")?;
	Ok(str)
}
