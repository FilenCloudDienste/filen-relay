use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result};
use dioxus::logger::tracing;
use filen_rclone_wrapper::rclone_installation::RcloneInstallation;
use filen_rclone_wrapper::rclone_installation::RcloneInstallationConfig;
use strum::IntoEnumIterator as _;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

use crate::backend::db::DB;
use crate::backend::rclone_auth_proxy::rclone_auth_proxy_ipc::get_rclone_auth_proxy_self_auth_token;
use crate::backend::rclone_auth_proxy::RcloneAuthProxyOptions;
use crate::common::ServerType;

use std::str::FromStr;

use axum_core::body::Body;
use axum_core::response::IntoResponse;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use dioxus::server::axum;
use dioxus::server::axum::{extract::Request, response::Response};
use http::Uri;

pub(crate) struct ServerManager {
	processes: Vec<RcloneServerProcess>,
}

#[derive(Debug)]
struct RcloneServerProcess {
	server_type: ServerType,
	_process: tokio::process::Child,
	port: u16,
	base_url: String,
}

impl ServerManager {
	pub(crate) async fn start_servers(self_port: u16) -> Result<ServerManager> {
		let config_dir = std::env::current_dir()
			.context("Failed to get current directory")?
			.join("rclone_configs");
		let mut servers = vec![];
		for server_type in ServerType::iter() {
			// start server process
			let port =
				port_check::free_local_ipv4_port().context("Failed to find free local port")?;

			// create temporary shell script to start the executable with the right args
			if cfg!(not(unix)) {
				panic!("Rclone auth proxy is currently only supported on Unix systems because it relies on shell scripts to start the executable with the right arguments. Contributions to make it work on Windows are welcome!");
			}
			let script = &format!(
				"#!/bin/sh\n{} {}\n",
				std::env::current_exe()
					.context("Failed to get current executable path")?
					.display(),
				RcloneAuthProxyOptions {
					self_port,
					self_authentication_token: get_rclone_auth_proxy_self_auth_token(),
					server_type: server_type.clone(),
				}
				.construct_args()
			);
			let mut script_file = tempfile::NamedTempFile::new()?;
			script_file.disable_cleanup(true); // todo
			script_file.as_file().write_all(script.as_bytes())?;
			let mut perms = script_file.as_file().metadata()?.permissions();
			perms.set_mode(0o755);
			script_file.as_file().set_permissions(perms)?;
			// todo: can we avoid creating a platform-dependent shell script?
			// maybe this is actually solved since we don't need to pass dynamic args anymore?

			// spawn rclone process
			let port_str = format!(":{}", port);
			let server_type_str = format!("{}", server_type).to_lowercase();
			let base_url = format!(
				"/{}",
				serde_json::to_string(&server_type)
					.unwrap()
					.trim_matches('"')
			);
			let args = vec![
				"serve",
				&server_type_str,
				"--addr",
				&port_str,
				"--auth-proxy",
				script_file.path().to_str().unwrap(),
				"--baseurl",
				&base_url,
				"--max-header-bytes",
				"16384", // arbitrarily larger than default 4096
				"--dir-cache-time",
				"1s", // so changes made to the Filen Drive are reflected immediately
			];
			let (mut process, _) =
				RcloneInstallation::initialize_unauthenticated(&RcloneInstallationConfig {
					rclone_binary_dir: config_dir.clone(),
					config_dir: config_dir.join(format!("server_{}", server_type_str)),
				})
				.await
				.context("Failed to initialize Rclone installation")?
				.execute_in_background(&args)
				.await
				.context("Failed to start Rclone server")?;

			// todo: handle process termination (health checks?) and restarts

			// handle logs
			{
				let process_stdout = process.stdout.take().unwrap();
				let server_type = server_type.clone();
				tokio::spawn(async move {
					let mut reader = BufReader::new(process_stdout).lines();
					while let Ok(Some(line)) = reader.next_line().await {
						tracing::debug!("Rclone server {} stdout: {}", server_type, line);
					}
				});
			}
			{
				let process_stderr = process.stderr.take().unwrap();
				let server_type = server_type.clone();
				tokio::spawn(async move {
					let mut reader = BufReader::new(process_stderr).lines();
					while let Ok(Some(line)) = reader.next_line().await {
						tracing::debug!("Rclone server {} stderr: {}", server_type, line);
					}
				});
			}

			servers.push(RcloneServerProcess {
				server_type,
				_process: process,
				port,
				base_url,
			});
		}

		Ok(ServerManager { processes: servers })
	}

	pub(crate) async fn handle_rclone_request(
		&self,
		server_type: &ServerType,
		id: Option<String>,
		mut req: Request,
	) -> Result<Response, anyhow::Error> {
		// find proxy target
		let server = self
			.processes
			.iter()
			.find(|s| s.server_type == *server_type)
			.context("No server found for the given type")?;
		let proxy =
			axum_reverse_proxy::ReverseProxy::new("", &format!("http://127.0.0.1:{}", server.port));

		// handle HTTP servers a special way // todo explain
		let transform_uri_backward = if server_type == &ServerType::Http {
			let id = id.expect("HTTP server requests will always have an id");

			let shares = DB
				.get_shares()
				.map_err(|e| anyhow::anyhow!("Failed to get shares from database: {}", e))?;
			if let Some(share) = shares.into_iter().find(|s| s.id.0 == id) {
				// check password if needed
				if let Some(password) = &share.password {
					let auth_header = req
						.headers()
						.get("authorization")
						.and_then(|h| h.to_str().ok())
						.unwrap_or("");
					let auth_header = auth_header.strip_prefix("Basic ").unwrap_or("");
					let auth_header = BASE64_STANDARD.decode(auth_header).unwrap_or_default();
					let auth_header = String::from_utf8_lossy(&auth_header);
					if !auth_header.ends_with(password) {
						return Ok(Response::builder()
							.status(axum::http::StatusCode::UNAUTHORIZED)
							.header(
								"WWW-Authenticate",
								format!("Basic realm=\"share {}\"", share.id),
							)
							.body(Body::from(
								"This share needs a password to access. No username is needed.",
							))
							.unwrap());
					}
				}

				// insert share id for auth proxy
				req.headers_mut().insert(
					"Authorization",
					format!(
						"Basic {}",
						BASE64_STANDARD.encode(format!(
							"{}:{}",
							share.id,
							share.password.as_ref().unwrap_or(&"".into())
						))
					)
					.parse()
					.unwrap(),
				);
			}

			// transform urls
			// this converts between e.g. /s/fc127fba/directory/file.txt -> /s/directory/file.txt
			// and back in XML responses from HTTP server
			// todo: explain reason
			let (transform_uri_fordward, transform_uri_backward) = {
				let base_path = {
					let request_path = req.uri().to_string();
					request_path
						.find(&id)
						.map(|id_idx| &request_path[..(id_idx + id.len())])
						.unwrap()
						.to_string()
					// e.g. base_path = "/s/fc127fba"
				};
				let base_path_1 = base_path.clone();
				(
					move |uri: &str| -> String { uri.replace(&base_path, &server.base_url) },
					move |uri: &str| -> String { uri.replace(&server.base_url, &base_path_1) },
				)
			};

			// transform request url
			*req.uri_mut() =
				Uri::from_str(&transform_uri_fordward(&req.uri().to_string())).unwrap_or_default();

			Some(transform_uri_backward)
		} else {
			None
		};

		let response = proxy.proxy_request(req).await.unwrap();
		if let Some(transform_uri_backward) = transform_uri_backward {
			if response
				.headers()
				.get("content-type")
				.and_then(|h| h.to_str().ok())
				.is_some_and(|h| h.contains("xml"))
			{
				// replace base url in response body if needed (e.g. for WebDAV directory listings)
				let (mut parts, body) = response.into_parts();
				let body = axum::body::to_bytes(body, usize::MAX)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to read response body for transformation (it might be too long; this might be a bug worth looking at): {}", e))?;
				let body = String::from_utf8_lossy(&body).to_string();
				let body = transform_uri_backward(&body);
				parts.headers.insert("content-length", body.len().into());
				return Ok(Body::from(body).into_response());
			}
		}
		Ok(response)
	}
}
