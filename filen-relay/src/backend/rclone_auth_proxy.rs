use std::{io::Read, sync::OnceLock};

use anyhow::{Context, Result};
use axum_core::{body::Body, extract::Request, response::Response};
use dioxus::fullstack::reqwest;
use filen_sdk_rs::auth::{http::ClientConfig, unauth::UnauthClient, StringifiedClient};
use http::StatusCode;
use rand::distr::SampleString;
use serde::Deserialize;
use serde_json::json;

use crate::{
    backend::db::DB,
    common::{Share, ShareId},
};

static RCLONE_AUTH_PROXY_AUTH_TOKEN: OnceLock<String> = OnceLock::new();

fn get_rclone_auth_proxy_auth_token() -> String {
    RCLONE_AUTH_PROXY_AUTH_TOKEN
        .get_or_init(|| rand::distr::Alphanumeric.sample_string(&mut rand::rng(), 32))
        .to_string()
}

pub(crate) static ACT_AS_RCLONE_AUTH_PROXY_ARG: &str = "--act-as-rclone-auth-proxy";

pub(crate) fn generate_rclone_auth_proxy_args(self_port: u16) -> String {
    format!(
        "{} --self-port={} --self-auth-token={}",
        ACT_AS_RCLONE_AUTH_PROXY_ARG,
        self_port,
        get_rclone_auth_proxy_auth_token()
    )
}

#[derive(Deserialize)]
struct RcloneAuthProxyInput {
    user: String,
}

// will be called when executable is in role of rclone auth proxy
pub(crate) fn rclone_auth_proxy_main() -> Result<()> {
    // extract self port and auth token
    let self_port = std::env::args()
        .find_map(|arg| {
            arg.strip_prefix("--self-port=")
                .and_then(|port| port.parse::<u16>().ok())
        })
        .context("Failed to extract self port from command line arguments")?;
    let self_auth_token = std::env::args()
        .find_map(|arg| {
            arg.strip_prefix("--self-auth-token=")
                .map(|s| s.to_string())
        })
        .context("Failed to extract self auth token from command line arguments")?;
    log::debug!(
        "Rclone auth proxy with self port {} and self auth token {}",
        self_port,
        self_auth_token
    );

    // extract share id
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).ok().unwrap();
    let rclone_auth_proxy_input: RcloneAuthProxyInput = serde_json::from_str(&input)
        .context(format!("Failed to deserialize JSON string: {}", input))?;
    let share_id = rclone_auth_proxy_input.user;
    log::debug!("Request for share id {}", share_id);

    // fetch remote config
    let rclone_remote_config = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async move {
            log::debug!(
                "Start fetching Rclone remote config for share id {}",
                share_id
            );
            match reqwest::Client::new()
                .get(format!(
                    "http://127.0.0.1:{}/rclone-auth-proxy/remote-config/{}",
                    self_port, share_id
                ))
                .bearer_auth(self_auth_token)
                .send()
                .await
            {
                Err(e) => {
                    log::error!("Failed to send request to Filen Relay server: {}", e);
                    Err(anyhow::anyhow!(
                        "Failed to send request to Filen Relay server: {}",
                        e
                    ))
                }
                Ok(response) => {
                    log::debug!(
                        "Received response with status code {} from Filen Relay server",
                        response.status()
                    );
                    match response.text().await {
                        Err(e) => Err(anyhow::anyhow!(
                            "Failed to read response from Filen Relay server: {}",
                            e
                        )),
                        Ok(text) => {
                            log::debug!("Received response from Filen Relay server: {}", text);
                            Ok(text)
                        }
                    }
                }
            }
        })?;
    log::debug!("Rclone remote config for: {}", rclone_remote_config);

    println!("{}", rclone_remote_config);
    Ok(())
}

// will be called when executable is the normal server, HTTP request is sent by the auth proxy
pub(crate) fn handle_rclone_remote_config_request(share_id: String, req: Request) -> Response {
    // auth
    let auth_header = match req.headers().get("Authorization") {
        Some(header) => header.to_str().unwrap_or(""),
        None => "",
    };
    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
    if token != get_rclone_auth_proxy_auth_token() {
        return Response::builder().status(401).body(Body::empty()).unwrap();
    }

    // find share
    let shares = match DB.get_shares() {
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

    // construct response
    match construct_rclone_remote_config_for_share(&share) {
        Ok(config) => Response::new(dioxus::fullstack::body::Body::from(config)),
        Err(e) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(format!(
                "Failed to construct Rclone remote config: {}",
                e
            )))
            .unwrap(),
    }
}

pub(crate) fn construct_rclone_remote_config_for_share(share: &Share) -> Result<String> {
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
