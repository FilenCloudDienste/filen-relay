use anyhow::Result;
use filen_sdk_rs::auth::Client;
use google_cloud_gax::paginator::ItemPaginator as _;
use google_cloud_secretmanager_v1::client::SecretManagerService;

use crate::Args;

pub(crate) async fn deploy_to_gcloud(
	filen_relay_version: &str,
	client: Client,
	args: Args,
) -> Result<()> {
	let client = SecretManagerService::builder().build().await?;
	let mut secrets = client
		.list_secrets()
		.set_parent("projects/my-project")
		.by_item();
	while let Some(secret) = secrets.next().await {
		let secret = secret?;
		println!("Found secret: {}", secret.name);
	}
	Ok(())
}
