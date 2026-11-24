// todo: better technical documentation

use std::collections::HashMap;

use axum_core::body::Body;
use axum_core::response::IntoResponse;
use dioxus::fullstack::Redirect;
use dioxus::prelude::*;
use dioxus::server::axum;
use dioxus::server::axum::{
	extract::{Path, Query, Request},
	response::Response,
};

use crate::api::SKIP_UPDATE_CHECKER;
use crate::backend::rclone_auth_proxy::rclone_auth_proxy_ipc;
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

			let (
				server_manager_1,
				server_manager_2,
				server_manager_3,
				server_manager_4,
				server_manager_5,
			) = (
				server_manager.clone(),
				server_manager.clone(),
				server_manager.clone(),
				server_manager.clone(),
				server_manager.clone(),
			);
			let router = dioxus::server::router(crate::frontend::App);
			let router = rclone_auth_proxy_ipc::register_ipc_endpoint(router);
			let router = auth::initialize_session_manager(router);
			Ok(router
				// append missing trailing slash
				.route(
					"/s/{id}",
					axum::routing::any(
						move |Path(_id): Path<String>,
						      Query(_query): Query<HashMap<String, String>>,
						      req: Request| async move {
							Redirect::permanent(&format!("{}/", req.uri())).into_response()
						},
					),
				)
				// serve http shares with or without rest
				.route(
					"/s/{id}/",
					axum::routing::any(
						move |Path(id): Path<String>,
						      Query(_query): Query<HashMap<String, String>>,
						      req: Request| async move {
							handle_rclone_request(
								server_manager_1,
								&ServerType::Http,
								Some(id),
								req,
							)
							.await
						},
					),
				)
				.route(
					"/s/{id}/{*rest}",
					axum::routing::any(
						move |Path((id, _rest)): Path<(String, String)>,
						      Query(_query): Query<HashMap<String, String>>,
						      req: Request| async move {
							handle_rclone_request(
								server_manager_2,
								&ServerType::Http,
								Some(id),
								req,
							)
							.await
						},
					),
				)
				// serve other servers (with or without rest and regardless of trailing slash)
				.route(
					"/{server_type}",
					axum::routing::any(
						|Path(server_type): Path<ServerType>, req: Request| async move {
							if server_type == ServerType::Webdav {
								// append missing trailing slash
								Redirect::permanent(&format!("{}/", req.uri())).into_response()
							} else {
								handle_rclone_request(server_manager_3, &server_type, None, req)
									.await
							}
						},
					),
				)
				.route(
					"/{server_type}/",
					axum::routing::any(
						move |Path(server_type): Path<ServerType>,
						      Query(_query): Query<HashMap<String, String>>,
						      req: Request| async move {
							handle_rclone_request(server_manager_4, &server_type, None, req).await
						},
					),
				)
				.route(
					"/{server_type}/{*rest}",
					axum::routing::any(
						move |Path((server_type, _rest)): Path<(ServerType, String)>,
						      Query(_query): Query<HashMap<String, String>>,
						      req: Request| async move {
							handle_rclone_request(server_manager_5, &server_type, None, req).await
						},
					),
				))
		}
	});
}

async fn handle_rclone_request(
	server_manager: std::sync::Arc<server_manager::ServerManager>,
	server_type: &ServerType,
	id: Option<String>,
	req: Request,
) -> Response {
	if server_type == &ServerType::Http && id.is_none() {
		return Response::builder()
			.status(axum::http::StatusCode::BAD_REQUEST)
			.body(Body::from("Share id is required under /s"))
			.unwrap();
	}
	server_manager
		.handle_rclone_request(server_type, id, req)
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
