use anyhow::{anyhow, Result};
use clap::Parser;
use filen_sdk_rs::{auth::Client, ErrorKind};
use filen_types::error::ResponseError;

mod scaleway;

#[derive(Parser, Clone)]
#[command()]
pub(crate) struct Args {
	#[arg(long, help = "Ignore update check")]
	ignore_updates: bool,
	#[arg(
		long,
		env = "FILEN_RELAY_ADMIN_EMAIL",
		help = "Email of the Filen account with admin privileges"
	)]
	admin_email: Option<String>,
	#[arg(
		long,
		env = "FILEN_RELAY_ADMIN_PASSWORD",
		help = "Password of the Filen account with admin privileges"
	)]
	admin_password: Option<String>,
	#[arg(
		long,
		env = "FILEN_RELAY_ADMIN_2FA_CODE",
		help = "2FA code of the Filen account with admin privileges (if 2FA is enabled)"
	)]
	admin_2fa_code: Option<String>,
	#[arg(
		long,
		env = "FILEN_RELAY_SCALEWAY_API_KEY_SECRET",
		help = "Scaleway API Secret Key"
	)]
	scaleway_api_key_secret: Option<String>,
	#[arg(
		long,
		env = "FILEN_RELAY_SCALEWAY_ORGANIZATION_ID",
		help = "Scaleway Organization ID"
	)]
	scaleway_organization_id: Option<String>,
	#[arg(
		long,
		env = "FILEN_RELAY_SCALEWAY_REGION",
		help = "Scaleway region to deploy to (e.g. fr-par, nl-ams, pl-waw)"
	)]
	scaleway_region: Option<String>,
	#[arg(
		long,
		env = "FILEN_RELAY_SCALEWAY_PROJECT_ID",
		help = "Scaleway Project ID to deploy to"
	)]
	scaleway_project_id: Option<String>,
	#[arg(
		long,
		env = "FILEN_RELAY_SCALEWAY_NAMESPACE_ID",
		help = "Scaleway Containers Namespace ID to deploy to, or 'create_new' to create a new namespace"
	)]
	scaleway_namespace_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
	env_logger::init();
	if let Err(e) = main_().await {
		log::error!("Error: {}", e);
		std::process::exit(1);
	}
	Ok(())
}

async fn main_() -> Result<()> {
	let args = Args::parse();
	let filen_relay_version = option_env!("FILEN_RELAY_VERSION").unwrap_or("0.0.0");

	// check if there's an update for filen-relay-deployer
	if !args.ignore_updates {
		let response = reqwest::Client::new()
			.get("https://api.github.com/repos/FilenCloudDienste/filen-relay/releases/latest")
			.header(reqwest::header::USER_AGENT, "filen-relay-deployer")
			.send()
			.await?;
		let latest_version = response
			.json::<serde_json::Value>()
			.await?
			.get("tag_name")
			.and_then(|t| t.as_str())
			.map(|s| s.to_string())
			.ok_or_else(|| anyhow!("Failed to get tag_name from GitHub API response"))?;
		if latest_version.trim_start_matches('v') != filen_relay_version.trim_start_matches('v') {
			cliclack::log::info(format!(
				"A new version of filen-relay-deployer is available: {} (current: {})",
				latest_version, filen_relay_version
			))?;
			return Ok(());
		}
	}

	cliclack::intro(format!("Filen Relay v{} Deployer", filen_relay_version,))?;

	// login to admin Filen account, export auth config
	let admin_email: String = match args.admin_email {
		Some(ref admin_email) => admin_email.clone(),
		None => cliclack::input("Admin Filen account email: ").interact()?,
	};
	let admin_password: String = match args.admin_password {
		Some(ref admin_password) => admin_password.clone(),
		None => cliclack::password("Admin Filen account password: ").interact()?,
	};
	let login_spinner = cliclack::spinner();
	login_spinner.start("Logging in to admin Filen account...");
	let client = match Client::login(
		admin_email.clone(),
		&admin_password,
		args.admin_2fa_code.as_deref().unwrap_or("XXXXXX"),
	)
	.await
	{
		Err(e) if e.kind() == ErrorKind::Server => match e.downcast::<ResponseError>() {
			Ok(ResponseError::ApiError { code, .. }) => {
				if code.as_deref() == Some("enter_2fa") {
					let two_factor_code: String =
						cliclack::input("Admin Filen two-factor code: ").interact()?;
					Client::login(admin_email, &admin_password, &two_factor_code).await?
				} else if code.as_deref() == Some("email_or_password_wrong") {
					cliclack::log::error("Email or password is wrong!")?;
					return Ok(());
				} else {
					return Err(anyhow!(
						"Failed to log in (code {})",
						code.as_deref().unwrap_or("")
					));
				}
			}
			Err(e) => return Err(anyhow!("Failed to log in: {}", e)),
		},
		Err(e) => return Err(anyhow!("Failed to log in: {}", e)),
		Ok(client) => client,
	};
	login_spinner.stop(format!("Logged in to admin account {}!", client.email()));

	// choose backend
	match cliclack::select("Pick a backend to deploy Filen Relay on:")
		.item("scaleway", "Scaleway", "")
		.interact()?
	{
		"scaleway" => {
			scaleway::deploy_to_scaleway(filen_relay_version, client, args).await?;
		}
		_ => unimplemented!(),
	}

	cliclack::outro("Deployed successfully!")?;
	Ok(())

	// todo: keep open
}
