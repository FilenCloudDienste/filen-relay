use dioxus::prelude::*;

#[component]
pub(crate) fn UpdateChecker() -> Element {
	let available_update =
		use_resource(move || async move { crate::api::check_for_updates().await });
	if let Some(response) = &*available_update.value().read() {
		match response {
			Ok(Some(update)) => rsx! {
				div { class: "card border-orange-300! py-3! px-4! block! mb-5!",
					"A new version of Filen Relay is available: {update.current_version} -> "
					a {
						class: "underline",
						href: "https://github.com/FilenCloudDienste/filen-relay/releases/tag/{update.latest_version}",
						target: "_blank",
						"{update.latest_version}"
					}
				}
			},
			Ok(None) => rsx! {},
			Err(_) => rsx! {
				div { class: "card bg-red-200", "Failed to check for updates." }
			},
		}
	} else {
		rsx! {}
	}
}
