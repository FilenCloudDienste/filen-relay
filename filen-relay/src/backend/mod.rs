// todo: better technical documentation

use axum_core::body::Body;
use dioxus::fullstack::Redirect;
use dioxus::prelude::*;
use dioxus::server::axum;
use dioxus::server::axum::middleware::Next;
use dioxus::server::axum::{
    extract::{Path, Request},
    response::Response,
};

use crate::api::SKIP_UPDATE_CHECKER;
use crate::backend::{
    rclone_auth_proxy::handle_rclone_remote_config_request, server_manager::ServerManager,
};
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
pub(crate) mod util;

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
            SKIP_UPDATE_CHECKER.set(args.skip_update_checker).unwrap();

            let self_port = std::env::var("PORT")
                .map(|port| port.parse::<u16>().unwrap_or(8080))
                .context("Failed to parse content of PORT env var")?;
            let server_manager = std::sync::Arc::new(
                ServerManager::start_servers(self_port)
                    .await
                    .context("Failed to start Rclone servers")
                    .unwrap(),
            );

            let (server_manager_1, server_manager_2) =
                (server_manager.clone(), server_manager.clone());
            let router = dioxus::server::router(crate::frontend::App);
            let router = auth::initialize_session_manager(router);
            Ok(
                router
                    .route(
                        "/{server_type}",
                        axum::routing::any(
                            |Path(_server_type): Path<ServerType>, _req: Request| async move {
                                Response::builder()
                                    .status(axum::http::StatusCode::NOT_FOUND)
                                    .body(Body::from("No share id provided"))
                                    .unwrap()
                                // WebDAV clients such as Windows Explorer might otherwise call routes such as /webdav, get assigned a session and be confused somehow
                            },
                        ),
                    )
                    .route(
                        "/{server_type}/{id}",
                        axum::routing::any(
                            |Path((_server_type, _id)): Path<(ServerType, String)>,
                             req: Request| async move {
                                Redirect::permanent(&format!("{}/", req.uri()))
                            },
                        ),
                    )
                    .route(
                        "/{server_type}/{id}/",
                        axum::routing::any(
                            move |Path((server_type, id)): Path<(ServerType, String)>,
                                  req: Request| async move {
                                handle_rclone_request(
                                    &self_port,
                                    server_manager_1,
                                    &server_type,
                                    id,
                                    req,
                                )
                                .await
                            },
                        ),
                    )
                    .route(
                        "/{server_type}/{id}/{*rest}",
                        axum::routing::any(
                            move |Path((server_type, id, _rest)): Path<(
                                ServerType,
                                String,
                                String,
                            )>,
                                  req: Request| async move {
                                handle_rclone_request(
                                    &self_port,
                                    server_manager_2,
                                    &server_type,
                                    id,
                                    req,
                                )
                                .await
                            },
                        ),
                    )
                    .route(
                        "/rclone-auth-proxy/remote-config/{share_id}",
                        axum::routing::get(
                            |Path(share_id): Path<String>, req: Request| async move {
                                handle_rclone_remote_config_request(share_id, req)
                            },
                        ),
                    )
                    .layer(axum::middleware::from_fn(
                        move |req: Request, next: Next| async move {
                            dioxus::logger::tracing::trace!(
                                "Received request: {} {}",
                                req.method(),
                                req.uri()
                            );
                            next.run(req).await
                        },
                    )),
            )
            // todo: add info somewhere that these routes exist
        }
    });
}

async fn handle_rclone_request(
    self_port: &u16,
    server_manager: std::sync::Arc<server_manager::ServerManager>,
    server_type: &ServerType,
    id: String,
    req: Request,
) -> Response {
    server_manager
        .handle_rclone_request(self_port, server_type, id, req)
        .await
        .unwrap_or_else(|e| {
            Response::builder()
                .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("Failed to process request: {}", e)))
                .unwrap()
        })
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
