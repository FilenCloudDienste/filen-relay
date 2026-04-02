mod manage_allowed_users;
mod shares;
mod update_checker;
use std::ops::Deref;

use dioxus::{
	logger::tracing::{self},
	prelude::*,
};
use dioxus_primitives::checkbox::CheckboxState;

use crate::{
	api::{get_admin_email, LoginStatus},
	components::{
		button::{Button, ButtonVariant},
		card::{Card, CardContent, CardFooter, CardHeader, CardTitle},
		checkbox::Checkbox,
		input::Input,
		label::Label,
	},
	frontend::{
		manage_allowed_users::ManageAllowedUsers, shares::Shares, update_checker::UpdateChecker,
	},
};

struct Authentication {
	pub email: String,
	pub is_admin: bool,
}
static AUTH: GlobalSignal<Option<Authentication>> = Signal::global(|| None);
async fn fetch_authentication() {
	match crate::api::get_user().await {
		Ok(user) => {
			tracing::info!("Authenticated as {}", user.email);
			*AUTH.write() = Some(Authentication {
				email: user.email,
				is_admin: user.is_admin,
			});
		}
		Err(err) => {
			tracing::info!("Not authenticated: {}", err);
		}
	}
}

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
pub(crate) enum Route {
    #[layout(NavbarLayout)]
    #[route("/")]
    Home {},
    #[route("/admin")]
    AdminPage {},
}

#[component]
fn NavbarLayout() -> Element {
	use_effect(|| {
		spawn(async move {
			fetch_authentication().await;
		});
	});

	let mut navbar_expanded = use_signal(|| false);
	let logout = || {
		spawn(async move {
			#[cfg(target_arch = "wasm32")]
			{
				wasm_cookies::delete("filen_email");
				wasm_cookies::delete("filen_password");
				wasm_cookies::delete("filen_two_factor_code");
			}
			match crate::api::logout().await {
				Ok(_) => {
					tracing::info!("Logged out successfully");
					*AUTH.write() = None;
				}
				Err(err) => {
					tracing::error!("Logout failed: {}", err);
				}
			}
		})
	};
	rsx! {
		div { id: "navbar", class: "border-b-1 border-gray-800 p-4",
			div { class: "flex items-center gap-4",
				img { class: "size-8", src: "https://filen.io/favicon.ico" }
				Link { to: Route::Home {}, class: "font-bold text-lg", "Filen Relay" }
				div { class: "flex-1" }

				div { class: "hidden md:flex items-center gap-4",
					if let Some(auth) = AUTH.read().deref() {
						span { "{auth.email}" }
						if auth.is_admin {
							Link { to: Route::AdminPage {},
								Button { variant: ButtonVariant::Secondary, "Admin Options" }
							}
						}
						Button {
							variant: ButtonVariant::Secondary,
							onclick: move |_| {
								logout();
							},
							"Logout"
						}
					}
					a {
						href: "https://github.com/FilenCloudDienste/filen-relay",
						target: "_blank",
						img {
							class: "size-8",
							src: asset!("/assets/github-icon.svg"),
						}
					}
				}

				Button {
					variant: ButtonVariant::Secondary,
					class: "md:hidden",
					onclick: move |_| {
						let is_open = *navbar_expanded.read();
						navbar_expanded.set(!is_open);
					},
					if *navbar_expanded.read() {
						"Close"
					} else {
						"Menu"
					}
				}
			}

			if *navbar_expanded.read() {
				div { class: "mt-5 flex flex-col gap-3 md:hidden",
					if let Some(auth) = AUTH.read().deref() {
						span { class: "text-gray-300", "Logged in as: {auth.email}" }
						if auth.is_admin {
							Link { to: Route::AdminPage {},
								Button {
									variant: ButtonVariant::Secondary,
									class: "w-full",
									"Admin Options"
								}
							}
						}
						Button {
							variant: ButtonVariant::Secondary,
							class: "w-full",
							onclick: move |_| {
								logout();
							},
							"Logout"
						}
					}
					a {
						href: "https://github.com/FilenCloudDienste/filen-relay",
						target: "_blank",
						class: "inline-flex w-fit ml-auto",
						img {
							class: "size-8",
							src: asset!("/assets/github-icon.svg"),
						}
					}
				}
			}
		}
		div { class: "p-4",
			if let Some(auth) = AUTH.read().deref() {
				if auth.is_admin {
					UpdateChecker {}
				}
				Outlet::<Route> {}
			} else {
				Login {}
			}
		}
	}
}

#[component]
fn Login() -> Element {
	let mut email = use_signal(|| "".to_string());
	let mut password = use_signal(|| "".to_string());
	let mut two_factor_code = use_signal(|| None::<String>);

	let mut loading = use_signal(|| false);

	let mut saved_credentials_pending = use_signal(|| true);
	let mut save_credentials = use_signal(|| false);
	use_effect(move || {
		#[cfg(target_arch = "wasm32")]
		{
			if let Some(Ok(email_val)) = wasm_cookies::get("filen_email") {
				email.set(email_val);
				save_credentials.set(true);
			}
			if let Some(Ok(password_val)) = wasm_cookies::get("filen_password") {
				password.set(password_val);
				save_credentials.set(true);
			}
			if let Some(Ok(code_val)) = wasm_cookies::get("filen_two_factor_code") {
				two_factor_code.set(Some(code_val));
				save_credentials.set(true);
			}
		}
		saved_credentials_pending.set(false);
	});

	let mut login_error_message = use_signal(|| None::<String>);

	let login = move || async move {
		loading.set(true);
		login_error_message.set(None);
		match crate::api::login(email.cloned(), password.cloned(), two_factor_code.cloned()).await {
			Ok(login_status) => match login_status {
				LoginStatus::LoggedIn => {
					tracing::info!("Login successful");
					#[cfg(target_arch = "wasm32")]
					{
						if *save_credentials.read() {
							let options = wasm_cookies::cookies::CookieOptions::default()
								.with_path("/")
								.secure()
								.with_same_site(wasm_cookies::cookies::SameSite::Strict)
								.expires_after(std::time::Duration::from_hours(24 * 30));
							wasm_cookies::set("filen_email", &email(), &options);
							wasm_cookies::set("filen_password", &password(), &options);
							if let Some(code) = two_factor_code().as_deref() {
								wasm_cookies::set("filen_two_factor_code", code, &options);
							} else {
								wasm_cookies::delete("filen_two_factor_code");
							}
						}
					}
					fetch_authentication().await;
					email.set("".to_string());
					password.set("".to_string());
					two_factor_code.set(None);
				}
				LoginStatus::TwoFactorRequired => {
					login_error_message.set(Some("2FA code required".into()));
				}
				LoginStatus::InvalidCredentials => {
					login_error_message.set(Some("Invalid credentials".into()));
				}
				LoginStatus::UserNotAllowed { only_admin_allowed } => {
					if only_admin_allowed {
						login_error_message.set(Some(
							"This instance is configured to only allow its host".into(),
						));
					} else {
						login_error_message.set(Some(
							"Your account is not allowed to use this instance".into(),
						));
					}
				}
			},
			Err(err) => {
				tracing::error!("Login failed: {}", err);
				login_error_message.set(Some("Login failed unexpectedly".into()));
			}
		};
		loading.set(false);
	};
	// todo: better user feedback for login errors, 2fa required, etc.

	let admin_email = use_resource(move || async move { get_admin_email().await });

	rsx! {
		div { class: "flex justify-center",
			Card {
				CardHeader {
					CardTitle { "Login" }
				}
				CardContent {
					p { class: "max-w-md -mt-3 mb-5 text-sm",
						"Enter your Filen credentials below. "
						"Please note that the host of this instance of Filen Relay"
						if let Some(Ok(admin_email)) = &*admin_email.read() {
							", {admin_email},"
						}
						" will have access to your Filen credentials in order to decrypt and relay your files, "
						"and thus also have access to your Filen account. Make sure you trust the host before logging in."
					}
					form {
						id: "login-form",
						class: "grid gap-4",
						onsubmit: move |e| async move {
							e.prevent_default();
							login().await;
						},
						if *saved_credentials_pending.read() {
							div { class: "text-gray-500", "Loading saved credentials..." }
						}
						div { class: "grid gap-2",
							Label { html_for: "email", "Filen Email:" }
							Input {
								r#type: "email",
								id: "email",
								value: "{email}",
								oninput: move |e: Event<FormData>| email.set(e.value().clone()),
							}
						}
						div { class: "grid gap-2",
							Label { html_for: "password", "Filen Password:" }
							Input {
								r#type: "password",
								id: "password",
								value: "{password}",
								oninput: move |e: Event<FormData>| password.set(e.value().clone()),
							}
						}
						div { class: "grid gap-2",
							Label { html_for: "2fa-code", "2FA Code (if needed):" }
							Input {
								r#type: "text",
								id: "2fa-code",
								value: format!("{}", two_factor_code().as_deref().unwrap_or("")),
								oninput: move |e: Event<FormData>| {
									let val = e.value().clone();
									if val.is_empty() {
										two_factor_code.set(None);
									} else {
										two_factor_code.set(Some(val));
									}
								},
							}
						}
						div { class: "flex gap-2 items-center",
							Checkbox {
								id: "save_credentials",
								checked: if *save_credentials.read() { CheckboxState::Checked } else { CheckboxState::Unchecked },
								on_checked_change: move |new_state| save_credentials.set(new_state == CheckboxState::Checked),
							}
							label {
								r#for: "save_credentials",
								class: "cursor-pointer",
								"Remember me"
							}
						}
						if let Some(error_message) = &*login_error_message.read() {
							div { class: "text-red-400", "{error_message}" }
						}
					}
				}
				CardFooter {
					Button {
						r#type: "submit",
						form: "login-form",
						disabled: *loading.read(),
						"Login"
					}
				}
			}
		}
	}
}

#[component]
pub(crate) fn App() -> Element {
	rsx! {
		document::Title { "Filen Relay" }
		document::Link { rel: "icon", href: "https://filen.io/favicon.ico" }
		document::Link { rel: "stylesheet", href: asset!("/assets/tailwind.css") }
		document::Stylesheet { href: asset!("/assets/dx-components-theme.css") }
		Router::<Route> {}
	}
}

#[component]
fn Home() -> Element {
	rsx! {
		h2 { "Your Shares" }
		Shares {}
	}
}

#[component]
fn AdminPage() -> Element {
	rsx! {
		ManageAllowedUsers {}
	}
}
