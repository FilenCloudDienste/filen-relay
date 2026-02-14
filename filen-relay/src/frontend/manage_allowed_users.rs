use dioxus::{core::Element, hooks::use_signal, prelude::component};
use dioxus::{
    logger::tracing::{self},
    prelude::*,
};

#[component]
pub(crate) fn ManageAllowedUsers() -> Element {
    let mut allowed_users = use_signal(|| None::<Vec<String>>);
    let mut loading = use_signal(|| false);
    let mut new_user_email = use_signal(|| "".to_string());

    let fetch_users = move || {
        spawn(async move {
            loading.set(true);
            match crate::api::get_allowed_users().await {
                Ok(users) => {
                    allowed_users.set(Some(users));
                }
                Err(err) => {
                    tracing::error!("Failed to fetch allowed users: {}", err);
                }
            }
            loading.set(false);
        });
    };
    use_effect(move || {
        fetch_users();
    });

    rsx! {
        div { class: "flex flex-col gap-4 border p-4 rounded-lg",
            h2 { class: "font-bold text-lg", "Manage Allowed Users" }
            form {
                class: "flex gap-2 items-center",
                onsubmit: move |e| async move {
                    e.prevent_default();
                    let email = new_user_email.read().clone();
                    if email.is_empty() {
                        tracing::error!("Email cannot be empty");
                        return;
                    }
                    match crate::api::add_allowed_user(email).await {
                        Ok(_) => {
                            tracing::info!("User added successfully");
                            new_user_email.set("".to_string());
                            fetch_users();
                        }
                        Err(err) => {
                            tracing::error!("Failed to add user: {}", err);
                        }
                    }
                },
                input {
                    class: "_input flex-1",
                    r#type: "email",
                    placeholder: "user@example.com",
                    value: "{new_user_email}",
                    oninput: move |e| new_user_email.set(e.value().clone()),
                }
                button {
                    class: "_button",
                    r#type: "submit",
                    disabled: new_user_email.read().is_empty(),
                    "Add User"
                }
            }
            if *loading.read() {
                div { class: "text-gray-500", "Loading..." }
            } else {
                if let Some(allowed_users) = allowed_users.read().as_ref() {
                    if !allowed_users.is_empty() {
                        div { class: "flex flex-col gap-2",
                            for user in allowed_users.iter().cloned() {
                                div { class: "flex items-center gap-2 p-2 border rounded",
                                    span { class: "flex-1 font-mono",
                                        "{user}"
                                        if user == "*" {
                                            span { class: "text-red-400 font-sans",
                                                " (this is a wildcard that allows anyone to access the system)"
                                            }
                                        }
                                    }
                                    button {
                                        class: "_button px-2 py-1 text-sm bg-red-500 hover:bg-red-600",
                                        onclick: move |_| {
                                            let user = user.clone();
                                            async move {
                                                match crate::api::remove_allowed_user(user.clone()).await {
                                                    Ok(_) => {
                                                        tracing::info!("User removed successfully");
                                                        fetch_users();
                                                    }
                                                    Err(err) => {
                                                        tracing::error!("Failed to remove user: {}", err);
                                                    }
                                                }
                                            }
                                        },
                                        "✕"
                                    }
                                }
                            }
                        }
                        p { class: "text-gray-500",
                            "If you want to stop allowing other users to access the system, "
                            a {
                                class: "text-blue-400 cursor-pointer",
                                onclick: move |_| {
                                    spawn(async move {
                                        match crate::api::clear_allowed_users().await {
                                            Ok(_) => {
                                                tracing::info!("Allowed users cleared successfully");
                                                fetch_users();
                                            }
                                            Err(err) => {
                                                tracing::error!("Failed to clear allowed users: {}", err);
                                            }
                                        }
                                    });
                                },
                                "clear the allowed users list"
                            }
                            " and remove their servers as needed."
                        }
                    } else {
                        div { class: "text-gray-500",
                            p {
                                "No allowed users configured. This means that only you are allowed to access the system and create servers."
                            }
                            p {
                                "To allow other users to access the system, add their email addresses to the allowed users list."
                            }
                        }
                    }
                    if !allowed_users.contains(&"*".to_string()) {
                        p { class: "text-gray-500",
                            "If you want to allow anyone to access the system, "
                            a {
                                class: "text-blue-400 cursor-pointer",
                                onclick: move |_| {
                                    spawn(async move {
                                        match crate::api::add_allowed_user("*".to_string()).await {
                                            Ok(_) => {
                                                tracing::info!("Wildcard user added successfully");
                                                fetch_users();
                                            }
                                            Err(err) => {
                                                tracing::error!("Failed to add wildcard user: {}", err);
                                            }
                                        }
                                    });
                                },
                                "add the wildcard (*) email"
                            }
                            " to the allowed users list."
                        }
                    }
                } else {
                    div { class: "text-gray-500", "Failed to load users." }
                }
            }
        }
    }
}
