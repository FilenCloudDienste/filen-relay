mod manage_allowed_users;
mod shares;
use std::ops::Deref;

use dioxus::{
    logger::tracing::{self},
    prelude::*,
};
use dioxus_primitives::checkbox::CheckboxState;

use crate::{
    api::LoginStatus,
    components::{
        button::{Button, ButtonVariant},
        card::{Card, CardContent, CardFooter, CardHeader, CardTitle},
        checkbox::Checkbox,
        input::Input,
        label::Label,
    },
    frontend::{manage_allowed_users::ManageAllowedUsers, shares::Shares},
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

    rsx! {
        div {
            id: "navbar",
            class: "flex gap-4 border-b-1 border-gray-800 p-4 items-center",
            img { class: "size-8", src: "https://filen.io/favicon.ico" }
            Link { to: Route::Home {}, class: "font-bold text-lg", "Filen Relay" }
            div { class: "flex-1" }
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
                        });
                    },
                    "Logout"
                }
            }
        }
        div { class: "p-4",
            if AUTH.read().is_some() {
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

    let login = move || async move {
        loading.set(true);
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
                    tracing::info!("Two-factor authentication required");
                }
                LoginStatus::InvalidCredentials => {
                    tracing::info!("Invalid email or password");
                    // todo: better user feedback
                }
            },
            Err(err) => {
                tracing::error!("Login failed: {}", err);
            }
        };
        loading.set(false);
    };

    rsx! {
        div { class: "flex justify-center",
            Card {
                CardHeader {
                    CardTitle { "Login" }
                }
                CardContent {
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
                                on_checked_change: move |new_state| save_credentials.set(new_state == CheckboxState::Checked),
                            }
                            "Remember me"
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
