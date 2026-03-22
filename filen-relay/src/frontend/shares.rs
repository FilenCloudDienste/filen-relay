use dioxus::prelude::*;
use dioxus_primitives::checkbox::CheckboxState;

use crate::{
    common::Share,
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
            div { class: "flex flex-col gap-2",
                for share in shares {
                    ShareCard {
                        share: share.clone(),
                        on_remove: move |_| fetch_shares(),
                    }
                }
                CreateShareCard { on_create: move |_| fetch_shares() }
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
    let url = format!("/s/{}/", share.id.short());
    let mut removing = use_signal(|| false);
    rsx! {
        div { class: "card p-3! pl-4!",
            div { class: "flex items-center gap-2",
                a {
                    class: "font-semibold flex items-center gap-2 cursor-pointer",
                    href: url,
                    target: "_blank",
                    "{share.root}"
                    img { src: open_external_icon, style: "color: #ffffff" }
                }
                div { class: "flex-1" }
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
                        let share_id = share.clone().id.clone();
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
                div { class: "flex items-center gap-2",
                    div { class: "flex items-center gap-4",
                        Input {
                            id: "root",
                            placeholder: "/path/to/share",
                            value: "{root}",
                            oninput: move |e: Event<FormData>| root.set(e.value().clone()),
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
                    // todo: options
                    div { class: "flex-1" }
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
