use crate::components::{
    label::Label,
    select::{SelectGroupLabel, SelectOption, SelectTrigger},
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
        select::{Select, SelectGroup, SelectItemIndicator, SelectList, SelectValue},
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

    let mut open_as = use_signal(|| Some(Some(ServerType::Http)));
    let open_as_options = ServerType::iter().enumerate().map(|(i, server_type)| {
        rsx! {
            SelectOption::<ServerType> {
                index: i,
                value: server_type.clone(),
                text_value: format!("{}", server_type),
                {format!("{}", server_type)}
                SelectItemIndicator {}
            }
        }
    });

    match (*shares)() {
        Some(shares) => rsx! {
            div { class: "grid gap-2 my-4",
                Label { html_for: "open-as-select", "Open shares as:" }
                Select::<ServerType> {
                    id: "open-as-select",
                    value: open_as,
                    on_value_change: move |new_open_as: Option<ServerType>| {
                        open_as.set(Some(Some(new_open_as.unwrap_or_default())))
                    },
                    SelectTrigger { width: "12rem", SelectValue {} }
                    SelectList {
                        SelectGroup {
                            SelectGroupLabel { "Server Type" }
                            {open_as_options}
                        }
                    }
                }
            }
            div { class: "flex flex-col gap-2",
                for share in shares {
                    ShareCard {
                        share: share.clone(),
                        open_as: open_as.cloned().unwrap().unwrap_or_default(),
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
pub(crate) fn ShareCard(share: Share, open_as: ServerType, on_remove: EventHandler<()>) -> Element {
    let open_external_icon = asset!("/assets/open-external-icon.svg");
    let url = format!("/{}/{}", open_as.to_url_segment(), share.id.short());
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
                div { class: "flex items-center gap-2 justify-between flex-wrap",
                    div { class: "flex items-center gap-4 flex-wrap",
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
