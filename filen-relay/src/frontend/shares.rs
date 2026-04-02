use crate::{
	api::ShareRootType,
	components::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle},
};
use dioxus::prelude::*;
use dioxus_primitives::checkbox::CheckboxState;
use strum::IntoEnumIterator as _;

use crate::{
	common::{ServerType, Share},
	components::{
		badge::{Badge, BadgeVariant},
		button::{Button, ButtonVariant},
		checkbox::Checkbox,
		input::Input,
	},
};

#[component]
pub(crate) fn Shares() -> Element {
	let mut shares = use_signal(|| None::<Vec<Share>>);
	let fetch_shares = move || {
		spawn(async move {
			match crate::api::get_shares().await {
				Ok(fetched_shares) => shares.set(Some(fetched_shares)),
				Err(e) => dioxus::logger::tracing::error!("Failed to fetch shares: {}", e),
			}
		});
	};
	use_effect(move || {
		fetch_shares();
	});

	match (*shares)() {
		Some(shares) => rsx! {
			div { class: "flex flex-col gap-2 mt-3",
				CreateShareCard { on_create: move |_| fetch_shares() }
				for share in shares.iter().rev() {
					ShareCard {
						key: "{share.id}",
						share: share.clone(),
						on_remove: move |_| fetch_shares(),
					}
				}
			}
		},
		None => rsx! {
			div { "Loading shares..." }
		},
	}
}

#[component]
pub(crate) fn ShareCard(share: Share, on_remove: EventHandler<()>) -> Element {
	let open_external_icon = asset!("/assets/open-external-icon.svg");
	let ellipsis_icon = asset!("/assets/ellipsis-icon.svg");
	let url = format!("/s/{}", share.id.short());
	let mut removing = use_signal(|| false);
	let mut open_external_dialog_open = use_signal(|| false);
	let share_id = share.id.clone();

	rsx! {
		div { class: "card p-3! pl-4!",
			div { class: "flex items-center gap-2 flex-wrap",
				a {
					class: "font-semibold flex items-center gap-2 cursor-pointer",
					href: url,
					target: "_blank",
					"{share.root}"
					img { src: open_external_icon }
								// todo: display copy icon instead when server type is not web
				}
				a {
					href: "",
					onclick: move |e| {
						e.prevent_default();
						open_external_dialog_open.set(true);
					},
					img { src: ellipsis_icon, class: "size-4" }
				}
				div { class: "flex-1" }
				div { class: "flex items-center gap-2",
					if share.read_only {
						Badge { variant: BadgeVariant::Secondary, "Read-Only" }
					} else {
						Badge { variant: BadgeVariant::Primary, "Read-Write" }
					}
					if share.password.is_some() {
						Badge { variant: BadgeVariant::Primary, "Password-Protected" }
					}
					Button {
						class: "ml-2",
						variant: ButtonVariant::Destructive,
						disabled: *removing.read(),
						onclick: move |_| {
							removing.set(true);
							let share_id = share_id.clone();
							async move {
								match crate::api::remove_share(share_id).await {
									Ok(_) => on_remove.call(()),
									Err(e) => {
										dioxus::logger::tracing::error!("Failed to remove share: {}", e)
									}
								}
							}
						},
						"Remove"
					}
				}
			}
		}
		DialogRoot {
			open: open_external_dialog_open(),
			on_open_change: move |v| open_external_dialog_open.set(v),
			DialogContent {
				button {
					class: "dialog-close",
					r#type: "button",
					aria_label: "Close",
					tabindex: if open_external_dialog_open() { "0" } else { "-1" },
					onclick: move |_| open_external_dialog_open.set(false),
					"×"
				}
				OpenAsDialog { share: share.clone() }
			}
		}
	}
}

#[component]
fn OpenAsDialog(share: Share) -> Element {
	let mut open_as = use_signal(|| None::<ServerType>);
	let root = use_resource(move || async move {
		let mut eval = document::eval("dioxus.send(window.location.origin);");
		match eval.recv::<String>().await {
			Ok(origin) => Ok(origin),
			Err(e) => {
				dioxus::logger::tracing::error!("Failed to get server URL: {}", e);
				Err(e)
			}
		}
	});
	let share_url = match &*root.read() {
		Some(Ok(url)) => Some(format!(
			"{}/{}/{}",
			url,
			open_as().unwrap_or_default().to_url_segment(),
			share.id.short()
		)),
		Some(Err(e)) => {
			dioxus::logger::tracing::error!("Failed to get server URL: {}", e);
			None
		}
		_ => None,
	};

	rsx! {
		DialogTitle { "Open Share" }
		DialogDescription {
			"You can open this share via different protocols:"
			div { class: "flex gap-2 mt-2 mb-4 flex-wrap justify-center sm:justify-start",
				for (server_type_1 , server_type_2) in ServerType::iter().map(|s| (s.clone(), s.clone())) {
					Button {
						variant: if open_as() == Some(server_type_2.clone()) { ButtonVariant::Secondary } else { ButtonVariant::Outline },
						onclick: move |_| open_as.set(Some(server_type_2.clone())),
						"{server_type_1}"
					}
				}
			}

			match open_as() {
				Some(ServerType::Http) => {
					rsx! {
						"You can open this share in your web browser by visiting:\n"
						if let Some(share_url) = share_url {
							a {
								class: "text-blue-400 hover:underline",
								href: "{share_url}",
								target: "_blank",
								"{share_url}"
							}
						} else {
							"Loading share URL..."
						}
					}
				}
				Some(ServerType::Webdav) => {
					rsx! {
						"You can open this share in a WebDAV client by connecting to:\n" // todo: add option to show the password
						if let Some(share_url) = share_url {
							a {
								class: "text-blue-400 hover:underline",
								href: "{share_url}/",
								target: "_blank",
								"{share_url}/"
							}
						} else {
							"Loading share URL..."
						}
						if share.password.is_some() {
							div { class: "mt-2",
								"This share needs a password to access. Use Basic authentication with any username (or empty) and the share password."
								// todo: add option to show the password
							}
						}
					}
				}
				Some(ServerType::S3) => {
					rsx! {
						div { class: "text-yellow-400",
							"This protocol, as well as FTP/SFTP, is not supported yet. Please check back later!" // todo: check that these protocols work properly and add instructions
						}
					}
				}
				_ => rsx! {},
			}
		}
	}
}

#[component]
pub(crate) fn CreateShareCard(on_create: EventHandler<()>) -> Element {
	let mut root = use_signal(String::new);
	let mut read_only = use_signal(|| false);
	let mut password = use_signal(|| None::<String>);
	let mut creating = use_signal(|| false);
	let is_valid = root.len() > 0;
	let create = move || async move {
		creating.set(true);
		match crate::api::add_share(root.cloned(), read_only.cloned(), password()).await {
			Ok(_) => {
				creating.set(false);
				on_create.call(());
				root.set(String::new());
				read_only.set(false);
				password.set(None);
			}
			Err(e) => dioxus::logger::tracing::error!("Failed to create share: {}", e),
		}
	};

	rsx! {
		div { class: "card p-3!",
			form {
				onsubmit: move |e| async move {
					e.prevent_default();
					create().await;
				},
				div { class: "flex items-center gap-2 justify-between flex-wrap",
					div { class: "flex items-center gap-4 flex-wrap",
						div { class: "flex flex-col gap-1",
							Input {
								id: "root",
								placeholder: "/path/to/share or ID",
								value: "{root}",
								oninput: move |e: Event<FormData>| root.set(e.value().clone()),
							}
							if root.len() > 0 {
								ShareRootChecker { root }
							}
						}
						div { class: "flex gap-2 items-center",
							Checkbox {
								id: "read_only",
								checked: if *read_only.read() { CheckboxState::Checked } else { CheckboxState::Unchecked },
								on_checked_change: move |checked| read_only.set(checked == CheckboxState::Checked),
							}
							label { r#for: "read_only", "Read-Only" }
						}
						Input {
							id: "password",
							placeholder: "Password (empty for none)",
							value: format!("{}", password().as_deref().unwrap_or("")),
							oninput: move |e: Event<FormData>| {
								let val = e.value().clone();
								if val.is_empty() {
									password.set(None);
								} else {
									password.set(Some(val));
								}
							},
						}
					}
					div { class: "ml-auto",
						Button {
							r#type: "submit",
							variant: ButtonVariant::Primary,
							disabled: !is_valid || *creating.read(),
							"Create Share"
						}
					}
				}
			}
		}
	}
}

#[component]
fn ShareRootChecker(root: ReadSignal<String>) -> Element {
	let mut checked_root = use_action(move |root: String| async move {
		match crate::api::check_and_transform_root(root).await {
			Ok(checked_root) => Ok(checked_root),
			Err(e) => {
				dioxus::logger::tracing::error!("Failed to check root: {}", e);
				Err(e)
			}
		}
	});
	use_effect(move || {
		checked_root.call(root());
	});
	match checked_root.value() {
		Some(Ok(checked_root)) => {
			let item_type = match checked_root.read().item_type {
				ShareRootType::File => "file",
				ShareRootType::Dir => "directory",
				ShareRootType::Root => "root",
			};
			let path = checked_root.read().path.clone();
			rsx! {
				div { class: "text-green-400 text-sm flex gap-1",
					"Sharing "
					span { class: "font-semibold", "{item_type}" }
					" at "
					span { class: "font-semibold", "{path}" }
				}
			}
		}
		Some(Err(_)) => rsx! {
			span { class: "text-red-400 text-sm", "Must be a valid path or ID" }
		},
		None => rsx! {},
	}
}
