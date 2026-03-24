// todo: better technical documentation

use std::str::FromStr;

use axum_core::body::Body;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
use dioxus::prelude::*;
use dioxus::server::axum;
use dioxus::server::axum::{
    extract::{Path, Request},
    response::Response,
};
use http::Uri;

use crate::backend::rclone_auth_proxy::handle_rclone_remote_config_request;
use crate::backend::server_manager::ServerManager;
use crate::common::ServerType;
use crate::{
    backend::{
        auth::ADMIN_EMAIL,
        db::{DbViaOfflineOrRemoteFile, DB},
    },
    Args,
};

pub(crate) mod auth;
pub(crate) mod db;
pub(crate) mod rclone_auth_proxy;
pub(crate) mod server_manager;

pub(crate) fn serve(args: Args) {
    dioxus::serve(move || {
        let args = args.clone();
        async move {
            let (admin_email, db) = match (
                    args.admin_email,
                    args.admin_password,
                    args.admin_2fa_code,
                    args.admin_auth_config,
                    args.db_dir,
                ) {
                    (Some(email), _, _, _, Some(db_dir)) => {
                        let db = DbViaOfflineOrRemoteFile::new_from_offline_location(Some(&db_dir)).await;
                        db.map(|db| (email, db))
                    }
                    (_, _, _, Some(auth_config), _) => {
                        DbViaOfflineOrRemoteFile::new_from_auth_config(auth_config).await
                    }
                    (Some(email), Some(password), two_fa_code, _, _) => {
                        let db = DbViaOfflineOrRemoteFile::new_from_email_and_password(
                            email.clone(),
                            &password,
                            two_fa_code.as_deref(),
                        )
                        .await;
                        db.map(|db| (email, db))
                    }
                    _ => panic!(
                        "Either admin email and local db dir, email/password or auth config must be provided"
                    ),
                }.expect("Failed to initialize database");
            ADMIN_EMAIL.set(admin_email).unwrap();
            DB.init(db);

            let self_port = std::env::var("PORT")
                .map(|port| port.parse::<u16>().unwrap_or(8080))
                .context("Failed to parse content of PORT env var")?;
            let server_manager = std::sync::Arc::new(
                ServerManager::start_servers(self_port)
                    .await
                    .context("Failed to start Rclone servers")
                    .unwrap(),
            );

            let (server_manager_1, server_manager_2, server_manager_3) = (
                server_manager.clone(),
                server_manager.clone(),
                server_manager.clone(),
            );
            let router = dioxus::server::router(crate::frontend::App);
            let router = auth::initialize_session_manager(router);
            Ok(router
                .route(
                    "/{server_type}/{id}",
                    axum::routing::any(
                        |Path((server_type, id)): Path<(ServerType, String)>, req: Request| async move {
                            handle_rclone_request(server_manager_1, &server_type, id, req).await
                        },
                    ),
                )
                .route(
                    "/{server_type}/{id}/",
                    axum::routing::any(
                        |Path((server_type, id)): Path<(ServerType, String)>, req: Request| async move {
                            handle_rclone_request(server_manager_2, &server_type, id, req).await
                        },
                    ),
                )
                .route(
                    "/{server_type}/{id}/{*rest}",
                    axum::routing::any(
                        |Path((server_type, id, _rest)): Path<(ServerType, String, String)>,
                         req: Request| async move {
                            handle_rclone_request(server_manager_3, &server_type, id, req).await
                        },
                    ),
                )
                .route(
                    "/rclone-auth-proxy/remote-config/{share_id}",
                    axum::routing::get(|Path(share_id): Path<String>, req: Request| async move {
                        handle_rclone_remote_config_request(share_id, req)
                    }),
                ))
            // todo: add info somewhere that these routes exist
        }
    });
}

async fn handle_rclone_request(
    server_manager: std::sync::Arc<server_manager::ServerManager>,
    server_type: &ServerType,
    id: String,
    mut req: Request,
) -> Response {
    let shares = match DB
        .get_shares()
        .map_err(|e| anyhow::anyhow!("Failed to get shares from database: {}", e))
    {
        Ok(shares) => shares,
        Err(e) => {
            return Response::builder()
                .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!(
                    "Failed to get shares from database: {}",
                    e
                )))
                .unwrap();
        }
    };
    let request_path = req.uri().path().to_string();
    let base_path = request_path
        .find(&id)
        .map(|pos| &request_path[..(pos + id.len())])
        .unwrap();
    if let Some(share) = shares.into_iter().find(|s| s.id.short() == id) {
        match server_manager
            .get_port_for_forwarded_request(server_type)
            .await
        {
            Ok(port) => {
                let proxy = axum_reverse_proxy::ReverseProxy::new(
                    base_path,
                    &format!("http://127.0.0.1:{}", port),
                );
                req.headers_mut().insert(
                    "Authorization",
                    format!("Basic {}", BASE64_STANDARD.encode(format!("{}:", share.id)))
                        .parse()
                        .unwrap(),
                );
                *req.uri_mut() = Uri::from_str(&req.uri().to_string().replace(base_path, ""))
                    .unwrap_or(Uri::default());
                proxy.proxy_request(req).await.unwrap_or_else(|e| {
                    Response::builder()
                        .status(axum::http::StatusCode::BAD_GATEWAY)
                        .body(Body::from(format!("Failed to proxy request: {}", e)))
                        .unwrap()
                })
            }
            Err(e) => Response::builder()
                .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("Failed to process request: {}", e)))
                .unwrap(),
        }
    } else {
        Response::builder()
            .status(axum::http::StatusCode::NOT_FOUND)
            .body(Body::from(format!("No share found for id: {}", id)))
            .unwrap()
    }
}

#[get("/api/ready")]
pub(crate) async fn ready() -> Result<(), axum::http::StatusCode> {
    let ready = true; // todo: check if all servers are ready?
    if ready {
        Ok(())
    } else {
        Err(axum::http::StatusCode::SERVICE_UNAVAILABLE)
    }
}
