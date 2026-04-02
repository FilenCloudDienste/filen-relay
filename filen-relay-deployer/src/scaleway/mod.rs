use anyhow::Result;
use filen_cli::serialize_auth_config;
use filen_sdk_rs::auth::Client;

use crate::Args;

mod scaleway_api;

pub(crate) async fn deploy_to_scaleway(
	filen_relay_version: &str,
	client: Client,
	args: Args,
) -> Result<()> {
	// enter api key, organization id, region
	let api_key: String = match args.scaleway_api_key_secret {
		Some(ref api_key) => api_key.clone(),
		None => cliclack::password("Enter your Scaleway API Secret Key:").interact()?,
	};
	let organization_id: String = match args.scaleway_organization_id {
		Some(ref organization_id) => organization_id.clone(),
		None => cliclack::input("Enter your Scaleway Organization ID:").interact()?,
	};
	let region = match args.scaleway_region {
		Some(ref region) => region,
		None => cliclack::select("Enter the region to deploy to")
			.item("fr-par", "Paris (fr-par)", "")
			.item("nl-ams", "Amsterdam (nl-ams)", "")
			.item("pl-waw", "Warsaw (pl-waw)", "")
			.interact()?,
	};
	let scaleway = scaleway_api::ScalewayApi::new(&api_key, &organization_id, region);

	// choose project
	let projects = scaleway.list_projects().await?;
	let project_id = match args.scaleway_project_id {
		Some(ref project_id) => project_id,
		None => cliclack::select("Choose a project to deploy to:")
			.items(
				projects
					.iter()
					.map(|p| (p.id.as_str(), p.name.as_str(), ""))
					.collect::<Vec<_>>()
					.as_slice(),
			)
			.interact()?,
	};

	// choose "filen-relay" namespace or create it
	let namespaces = scaleway.list_containers_namespaces().await?;
	let namespace_id = match args.scaleway_namespace_id {
		Some(ref namespace_id) => namespace_id,
		None => cliclack::select("Choose a namespace to deploy to:")
			.item("create_new", "Create a new namespace", "")
			.items(
				namespaces
					.iter()
					.map(|ns| (ns.id.as_str(), ns.name.as_str(), ""))
					.collect::<Vec<_>>()
					.as_slice(),
			)
			.interact()?,
	};
	let namespace = if namespace_id == "create_new" {
		// create a new namespace named "filen-relay-<random-suffix>"
		let random_suffix: String = uuid::Uuid::new_v4().as_simple().to_string()[..8].to_string();
		let namespace_name = format!("filen-relay-{}", random_suffix);
		scaleway
			.create_containers_namespace(&namespace_name, project_id)
			.await?
	} else {
		let namespace_id = namespace_id.to_string();
		namespaces
			.into_iter()
			.find(|ns| ns.id == namespace_id)
			.unwrap()
	};

	// wait for namespace to be ready
	let namespace_ready_spinner = cliclack::spinner();
	let mut i = 0;
	loop {
		let namespace = scaleway.get_containers_namespace(&namespace.id).await?;
		if namespace.status == "ready" {
			break;
		}
		if i == 1 {
			namespace_ready_spinner.start("Waiting for namespace to be ready...");
		}
		tokio::time::sleep(std::time::Duration::from_secs(1)).await;
		i += 1;
	}
	namespace_ready_spinner.stop("Namespace is ready!");

	// create container and deploy it
	let container_name = format!(
		"filen-relay-{}",
		&uuid::Uuid::new_v4().as_simple().to_string()[..8]
	);
	let container = scaleway
		.create_container(&serde_json::json!({
			"namespace_id": namespace.id,
			"name": container_name,
			"registry_image": format!("ghcr.io/FilenCloudDienste/filen-relay:{}", filen_relay_version),
			"min_scale": 0,
			"max_scale": 1,
			"port": 80,
			"cpu_limit": 250,
			"memory_limit": 256,
			"secret_environment_variables": [
				{
					"key": "FILEN_RELAY_ADMIN_AUTH_CONFIG",
					"value": serialize_auth_config(&client)?,
				},
			],
			"health_check": {
				"http": {
					"path": "/api/ready",
				},
				"failure_threshold": 24,
				"interval": "5s"
			},
		}))
		.await?;
	scaleway.deploy_container(&container.id).await?;
	let console_url = format!(
		"https://console.scaleway.com/containers/namespaces/{}/{}/containers/{}",
		region, namespace.id, container.id
	);
	cliclack::log::success(format!(
        "Deployed Filen Relay to Scaleway!\nView it in the Scaleway Console: {}\nFilen Relay soon available at: https://{}",
        console_url,
        container.domain_name
    ))?;

	Ok(())
}
