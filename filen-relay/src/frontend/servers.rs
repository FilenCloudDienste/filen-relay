use chrono::Local;
use dioxus::{
    logger::tracing::{self},
    prelude::*,
};
use strum::IntoEnumIterator as _;

use crate::{
    common::{LogLine, LogLineContent, ServerState, ServerStatus, ServerType},
    frontend::{Route, AUTH},
};

#[component]
pub(crate) fn Servers() -> Element {
    let mut servers = use_signal(|| None::<Vec<ServerState>>);
    use_future(move || async move {
        match crate::api::get_servers().await {
            Ok(mut servers_stream) => loop {
                match servers_stream.next().await {
                    Some(Ok(new_servers)) => {
                        servers.set(Some(new_servers));
                    }
                    Some(Err(err)) => {
                        tracing::error!("Error receiving server states: {}", err);
                        break;
                    }
                    None => {
                        tracing::info!("Server states stream ended");
                        break;
                    }
                }
            },
            Err(err) => {
                tracing::error!("Failed to fetch servers: {}", err);
            }
        }
    });
    let servers = &*servers;

    let is_admin = AUTH.read().as_ref().is_some_and(|auth| auth.is_admin);

    match servers() {
        Some(servers) if !servers.is_empty() => {
            rsx! {
                div { class: "flex flex-wrap gap-4",
                    for server in servers {
                        div { class: "border p-4 inline-flex flex-col w-64 rounded-lg",
                            h2 { class: "font-bold text-lg", "{server.spec.name}" }
                            p {
                                "ID: "
                                span { class: "font-mono", "#{server.spec.id.short()}" }
                            }
                            p { "Type: {server.spec.server_type}" }
                            if is_admin {
                                p {
                                    span { class: "font-mono", "{server.spec.filen_email}" }
                                }
                            }
                            p { "Root: {server.spec.root}" }
                            if server.spec.read_only {
                                p { "Mode: Read-Only" }
                            } else {
                                p { "Mode: Read-Write" }
                            }
                            if server.spec.password.is_some() {
                                p { "Password protection" }
                            } else {
                                p { "No password protection" }
                            }
                            match server.status.clone() {
                                ServerStatus::Starting => rsx! {
                                    p { class: "text-gray-500", "Status: Starting..." }
                                },
                                ServerStatus::Running { .. } => rsx! {
                                    p { class: "text-green-500", "Online" }
                                    p {
                                        "Connect: "
                                        a {
                                            class: "font-mono text-blue-400",
                                            href: "/s/{server.spec.id.short()}/",
                                            target: "_blank",
                                            "/s/{server.spec.id.short()}/"
                                        }
                                    }
                                },
                                ServerStatus::Error => rsx! {
                                    p { class: "text-red-500", "Status: Error" }
                                },
                            }
                            Link {
                                to: Route::LogsPage {
                                    logs_id: server.logs_id.clone(),
                                },
                                class: "flex _button mt-2",
                                "View Logs"
                            }
                            button {
                                class: "_button mt-2",
                                onclick: move |_| {
                                    let server = server.clone();
                                    async move {
                                        match crate::api::remove_server(server.spec.id.clone()).await {
                                            Ok(_) => {
                                                tracing::info!("Server removed successfully");
                                            }
                                            Err(err) => {
                                                tracing::error!("Failed to remove server: {}", err);
                                            }
                                        };
                                    }
                                },
                                "Remove Server"
                            }
                        }
                    }
                }
            }
        }
        Some(_) => {
            rsx! {
                div { class: "text-gray-500", "No servers available." }
            }
        }
        None => rsx! {
            div { class: "text-gray-500", "Loading servers..." }
        },
    }
}

#[component]
pub(crate) fn CreateServerForm() -> Element {
    let mut name = use_signal(|| "".to_string());
    let mut server_type = use_signal(|| ServerType::Http);
    let mut root = use_signal(|| "/".to_string());
    let mut read_only = use_signal(|| false);
    let mut password = use_signal(|| None::<String>);
    let password_str = password.read().as_deref().unwrap_or("").to_string();

    rsx! {
        form {
            class: "flex flex-col gap-2 border p-4 rounded-lg max-w-80",
            onsubmit: move |e| async move {
                e.prevent_default();
                let name_ = name.read().clone();
                if name_.is_empty() {
                    tracing::error!("Server name cannot be empty");
                    return;
                }
                let server_type_ = server_type.read().clone();
                let root_ = root.read().clone();
                let read_only_ = *read_only.read();
                let password_ = password.read().clone();
                match crate::api::add_server(
                        name_.to_string(),
                        server_type_.clone(),
                        root_,
                        read_only_,
                        password_,
                    )
                    .await
                {
                    Ok(_) => {
                        tracing::info!("Server created successfully");
                        name.set("".to_string());
                        server_type.set(ServerType::Http);
                        root.set("/".to_string());
                        read_only.set(false);
                        password.set(None);
                    }
                    Err(err) => {
                        tracing::error!("Failed to create server: {}", err);
                    }
                };
            },
            div { class: "flex flex-col gap-2",
                div {
                    label { "Server Name:" }
                    input {
                        class: "mt-1 _input",
                        r#type: "text",
                        placeholder: "My Server",
                        value: "{name}",
                        oninput: move |e| name.set(e.value().clone()),
                    }
                }
                div {
                    label { "Server Type:" }
                    select {
                        class: "mt-1 _input w-full",
                        onchange: move |e| {
                            server_type.set(ServerType::from(e.value().as_str()));
                        },
                        for server_type in ServerType::iter() {
                            option { value: server_type.to_string(), "{server_type.to_string()}" }
                        }
                    }
                }
                div {
                    label { "Root Path:" }
                    input {
                        class: "mt-1 _input",
                        r#type: "text",
                        placeholder: "/",
                        value: "{root}",
                        oninput: move |e| root.set(e.value().clone()),
                    }
                }
                div {
                    label { class: "flex items-center gap-2",
                        "Read-Only"
                        input {
                            r#type: "checkbox",
                            checked: *read_only.read(),
                            onchange: move |e| read_only.set(e.value() == "true"),
                        }
                    }

                }
                div {
                    label { "Password:" }
                    input {
                        class: "mt-1 _input",
                        r#type: "password",
                        placeholder: "Password",
                        value: "{password_str}",
                        oninput: move |e| password.set(Some(e.value().clone())),
                    }
                }
            }
            button {
                class: "_button",
                r#type: "submit",
                disabled: name.read().is_empty(),
                "Create Server"
            }
        }
    }
}

#[component]
pub(crate) fn Logs(logs_id: String) -> Element {
    let mut logs = use_signal(Vec::<LogLine>::new);
    use_future(move || {
        let logs_id = logs_id.clone();
        async move {
            match crate::api::get_logs(logs_id.clone()).await {
                Ok(mut logs_stream) => loop {
                    match logs_stream.next().await {
                        Some(Ok(new_log)) => {
                            logs.write().push(new_log);
                        }
                        Some(Err(err)) => {
                            tracing::error!("Error receiving logs: {}", err);
                            break;
                        }
                        None => {
                            tracing::info!("Logs stream ended");
                            break;
                        }
                    }
                },
                Err(err) => {
                    tracing::error!("Failed to fetch logs: {}", err);
                }
            }
        }
    });
    rsx! {
        div { class: "flex flex-col gap-1 p-2 rounded-lg overflow-y-auto font-mono text-gray-200",
            for (log , timestamp) in logs.read()
                .iter()
                .map(|log| (
                    log,
                    log.clone().timestamp.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S"),
                ))
            {
                div {
                    span { class: "text-gray-500 mr-2", "[{timestamp}] " }
                    match &log.content {
                        LogLineContent::ServerProcess(content) => rsx! {
                            span { "{content}" }
                        },
                        LogLineContent::Event(content) => rsx! {
                            span { class: "text-blue-400", "{content}" }
                        },
                    }
                }
            }
        }
    }
}
