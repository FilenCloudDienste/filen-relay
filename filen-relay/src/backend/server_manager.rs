use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result};
use dioxus::logger::tracing;
use filen_rclone_wrapper::rclone_installation::RcloneInstallation;
use filen_rclone_wrapper::rclone_installation::RcloneInstallationConfig;
use serde::Deserialize;
use serde::Serialize;
use strum::IntoEnumIterator as _;
use strum_macros::EnumIter;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

use crate::backend::rclone_auth_proxy::generate_rclone_auth_proxy_args;

#[derive(EnumIter, PartialEq)]
pub(crate) enum ServerType {
    Http,
    Webdav,
    S3,
    Ftp,
    Sftp,
}

impl Serialize for ServerType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            ServerType::Http => "s",
            ServerType::Webdav => "webdav",
            ServerType::S3 => "s3",
            ServerType::Ftp => "ftp",
            ServerType::Sftp => "sftp",
        };
        serializer.serialize_str(s)
    }
}

impl ServerType {
    fn to_rclone_serve_arg(&self) -> &'static str {
        match self {
            ServerType::Http => "http",
            ServerType::Webdav => "webdav",
            ServerType::S3 => "s3",
            ServerType::Ftp => "ftp",
            ServerType::Sftp => "sftp",
        }
    }
}

impl<'de> Deserialize<'de> for ServerType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "s" => Ok(ServerType::Http),
            "webdav" => Ok(ServerType::Webdav),
            "s3" => Ok(ServerType::S3),
            "ftp" => Ok(ServerType::Ftp),
            "sftp" => Ok(ServerType::Sftp),
            _ => Err(serde::de::Error::custom(format!(
                "Unknown server type: {}",
                s
            ))),
        }
    }
}

pub(crate) struct ServerManager {
    processes: Vec<RcloneServerProcess>,
}

struct RcloneServerProcess {
    pub server_type: ServerType,
    pub _process: tokio::process::Child,
    pub port: u16,
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
                generate_rclone_auth_proxy_args(self_port)
            );
            let mut script_file = tempfile::NamedTempFile::new()?;
            script_file.disable_cleanup(true); // todo
            script_file.as_file().write_all(script.as_bytes())?;
            let mut perms = script_file.as_file().metadata()?.permissions();
            perms.set_mode(0o755);
            script_file.as_file().set_permissions(perms)?;
            dbg!(script_file.path());
            // todo: can we avoid creating a platform-dependent shell script?

            // spawn rclone process
            let (mut process, _) =
                RcloneInstallation::initialize_unauthenticated(&RcloneInstallationConfig {
                    rclone_binary_dir: config_dir.clone(),
                    config_dir: config_dir
                        .join(format!("server_{}", server_type.to_rclone_serve_arg())),
                })
                .await
                .context("Failed to initialize Rclone installation")?
                .execute_in_background(&[
                    "serve",
                    server_type.to_rclone_serve_arg(),
                    "--addr",
                    &format!(":{}", port),
                    "--auth-proxy",
                    script_file.path().to_str().unwrap(),
                    "--verbose",
                ])
                .await
                .context("Failed to start Rclone server")?;

            // todo: handle process termination (health checks?) and restarts

            // handle logs
            let server_type_str = server_type.to_rclone_serve_arg();
            {
                let process_stdout = process.stdout.take().unwrap();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(process_stdout).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        tracing::debug!("Rclone server {} stdout: {}", server_type_str, line);
                    }
                });
            }
            {
                let process_stderr = process.stderr.take().unwrap();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(process_stderr).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        tracing::debug!("Rclone server {} stderr: {}", server_type_str, line);
                    }
                });
            }

            servers.push(RcloneServerProcess {
                server_type,
                _process: process,
                port,
            });
        }

        Ok(ServerManager { processes: servers })
    }

    pub(crate) async fn get_port_for_forwarded_request(
        &self,
        server_type: &ServerType,
    ) -> Result<u16> {
        let server = self
            .processes
            .iter()
            .find(|s| s.server_type == *server_type)
            .context("No server found for the given type")?;
        Ok(server.port)
    }
}
