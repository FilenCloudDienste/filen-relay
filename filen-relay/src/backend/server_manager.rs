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
use crate::backend::rclone_auth_proxy::get_encoded_rclone_remote_config_for_share;
use crate::backend::rclone_auth_proxy::ACT_AS_RCLONE_AUTH_PROXY_ARG;
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

struct RcloneServerProcess {
    server_type: ServerType,
    _process: tokio::process::Child,
    port: u16,
    base_url: Option<&'static str>,
}

static RCLONE_BASE_URL_TO_SUBSTITUTE: &str = "/RCLONE_BASE_URL_TO_SUBSTITUTE"; // must start with a slash

impl ServerManager {
    pub(crate) async fn start_servers() -> Result<ServerManager> {
        let config_dir = std::env::current_dir()
            .context("Failed to get current directory")?
            .join("rclone_configs");
        let mut servers = vec![];
        for server_type in ServerType::iter() {
            if server_type == ServerType::Http {
                // we don't need a separate http server, since the WebDAV server already handles this
                continue;
            }

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
                ACT_AS_RCLONE_AUTH_PROXY_ARG
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
            let mut args = vec![
                "serve",
                server_type.to_str(),
                "--addr",
                &port_str,
                "--auth-proxy",
                script_file.path().to_str().unwrap(),
                "--max-header-bytes",
                "16384", // arbitrarily larger than default 4096
            ];
            let base_url = if server_type == ServerType::Webdav {
                args.push("--baseurl");
                args.push(RCLONE_BASE_URL_TO_SUBSTITUTE);
                Some(RCLONE_BASE_URL_TO_SUBSTITUTE)
            } else {
                None
            };
            let (mut process, _) =
                RcloneInstallation::initialize_unauthenticated(&RcloneInstallationConfig {
                    rclone_binary_dir: config_dir.clone(),
                    config_dir: config_dir.join(format!("server_{}", server_type.to_str())),
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
        self_port: &u16,
        server_type: &ServerType,
        id: String,
        mut req: Request,
    ) -> Result<Response, anyhow::Error> {
        let shares = DB
            .get_shares()
            .map_err(|e| anyhow::anyhow!("Failed to get shares from database: {}", e))?;
        if let Some(share) = shares.into_iter().find(|s| s.id.short() == id) {
            // check password
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
                    get_encoded_rclone_remote_config_for_share(&share)
                        .context("Failed to contruct rclone remote config for share")?
                )
                .parse()
                .unwrap(),
            );

            // find proxy target
            let server = self
                .processes
                .iter()
                .find(|s| {
                    s.server_type == *server_type
                        || (*server_type == ServerType::Http && s.server_type == ServerType::Webdav)
                    // see above about not needing a separate HTTP server
                })
                .context("No server found for the given type")?;
            let proxy = axum_reverse_proxy::ReverseProxy::new(
                "",
                &format!("http://127.0.0.1:{}", server.port),
            );

            // transform urls (request uri and several headers)
            // this converts between e.g. /webdav/fc127fba/directory/file.txt -> /RCLONE_BASE_URL_TO_SUBSTITUTE/directory/file.txt
            // and back in XML responses from WebDAV
            let (transform_uri_fordward, transform_uri_backward) = {
                let base_path = {
                    let request_path = req.uri().to_string();
                    request_path
                        .find(&id)
                        .map(|id_idx| &request_path[..(id_idx + id.len())])
                        .unwrap()
                        .to_string()
                    // e.g. base_path = "/webdav/fc127fba"
                };
                let self_host = req
                    .headers()
                    .get("host")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or(&format!("127.0.0.1:{}", self_port))
                    .to_string();
                let (base_path_1, self_host_1) = (base_path.clone(), self_host.clone());
                (
                    move |uri: &str| -> String {
                        let uri = uri.replace(&self_host, &format!("127.0.0.1:{}", server.port));
                        if let Some(base_url) = server.base_url {
                            uri.replace(&base_path, base_url)
                        } else {
                            uri
                        }
                    },
                    move |uri: &str| -> String {
                        let uri = uri.replace(&format!("127.0.0.1:{}", server.port), &self_host_1);
                        if let Some(base_url) = server.base_url {
                            uri.replace(base_url, &base_path_1)
                        } else {
                            uri
                        }
                    },
                )
            };

            // transform request url
            *req.uri_mut() =
                Uri::from_str(&transform_uri_fordward(&req.uri().to_string())).unwrap_or_default();

            // transform request url in "Host", "Destination" and "Referer" header (used in WebDAV protocol)
            if let Some(host) = req.headers().get("host") {
                let host = host
                    .to_str()
                    .context("Failed to parse Host header")?
                    .to_string();
                req.headers_mut()
                    .insert("host", transform_uri_fordward(&host).parse().unwrap());
            }
            if let Some(destination) = req.headers().get("destination") {
                let destination = destination
                    .to_str()
                    .context("Failed to parse Destination header")?
                    .to_string();
                req.headers_mut().insert(
                    "destination",
                    transform_uri_fordward(&destination).parse().unwrap(),
                );
            }
            if let Some(referer) = req.headers().get("referer") {
                let referer = referer
                    .to_str()
                    .context("Failed to parse Referer header")?
                    .to_string();
                req.headers_mut()
                    .insert("referer", transform_uri_fordward(&referer).parse().unwrap());
            }

            match proxy.proxy_request(req).await {
                Ok(response) => {
                    if server_type == &ServerType::Webdav
                        && response
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
                        Ok(Body::from(body).into_response())
                    } else {
                        Ok(response)
                    }
                }
                Err(e) => Ok(Response::builder()
                    .status(axum::http::StatusCode::BAD_GATEWAY)
                    .body(Body::from(format!("Failed to proxy request: {}", e)))
                    .unwrap()),
            }
        } else {
            Ok(Response::builder()
                .status(axum::http::StatusCode::NOT_FOUND)
                .body(Body::from(format!("No share found for id: {}", id)))
                .unwrap())
        }
    }
}
