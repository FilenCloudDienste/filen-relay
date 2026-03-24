use anyhow::{anyhow, Result};
use clap::Parser;
use filen_cli::serialize_auth_config;
use filen_sdk_rs::{auth::Client, ErrorKind};
use filen_types::error::ResponseError;

mod scaleway_api;

#[derive(Parser, Clone)]
#[command()]
struct Args {
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
            deploy_to_scaleway(filen_relay_version, client, args).await?;
        }
        _ => unimplemented!(),
    }

    cliclack::outro("Deployed successfully!")?;
    Ok(())

    // todo: keep open
}

async fn deploy_to_scaleway(filen_relay_version: &str, client: Client, args: Args) -> Result<()> {
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
