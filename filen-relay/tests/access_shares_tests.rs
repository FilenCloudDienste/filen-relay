use filen_rclone_wrapper::rclone_installation::{RcloneInstallation, RcloneInstallationConfig};
use predicates::prelude::PredicateBooleanExt;
use serde::Deserialize;
use tempfile::TempDir;
use tokio::process::Command;

struct FilenRelayProcess {
	_process: tokio::process::Child, // kept here so it is removed on drop
	_db_dir: TempDir,                // kept here so it is removed on drop
	url_root: String,
	client: reqwest::Client,
}

#[derive(Deserialize)]
struct Share {
	id: String,
}

impl FilenRelayProcess {
	fn start() -> Self {
		let (email, _, _) = test_utils::RESOURCES.get_credentials();
		let db_dir = TempDir::new().unwrap();
		let port = 8080;
		let process = Command::new("dx")
			.arg("run")
			.env("FILEN_RELAY_ADMIN_EMAIL", email)
			.env("FILEN_RELAY_DB_DIR", db_dir.path())
			.env("PORT", port.to_string())
			.kill_on_drop(true)
			.spawn()
			.expect("Failed to start Filen Relay process");
		Self {
			_process: process,
			_db_dir: db_dir,
			url_root: format!("http://127.0.0.1:{}", port),
			client: reqwest::Client::builder()
				.cookie_store(true) // so auth sessions can be used across requests
				.build()
				.unwrap(),
		}
	}

	async fn wait_for_ready(&self) {
		let mut attempts = 0;
		let max_attempts = 30;
		loop {
			match reqwest::get(&format!("{}/api/ready", self.url_root)).await {
				Ok(res) if res.status().is_success() => break,
				Ok(_) | Err(_) if attempts < max_attempts => {
					attempts += 1;
					tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
				}
				Ok(res) => {
					panic!(
						"Filen Relay is not ready after {} attempts, last response status: {}",
						max_attempts,
						res.status()
					)
				}
				Err(e) => {
					panic!("Failed to connect after {} attempts: {:?}", max_attempts, e)
				}
			}
		}
	}

	async fn login(&self) {
		let (email, password, two_factor_code) = test_utils::RESOURCES.get_credentials();
		self.client
			.post(format!("{}/api/login", self.url_root))
			.json(&serde_json::json!({
				"email": email,
				"password": password,
				"two_factor_code": two_factor_code
			}))
			.send()
			.await
			.expect("Failed to send login request");
	}

	async fn create_test_share_and_get_id(&self, dir: String) -> String {
		// create share
		let res = self
			.client
			.post(format!("{}/api/shares/add", self.url_root))
			.body(
				serde_json::json!({
					"root": dir,
					"read_only": true,
					"password": null,
				})
				.to_string(),
			)
			.send()
			.await
			.expect("Failed to send request to create test share");
		if !res.status().is_success() {
			panic!(
				"Failed to create test share, status: {}, body: {:?}",
				res.status(),
				res.text().await
			);
		}

		// get share uuid
		let res = self
			.client
			.get(format!("{}/api/shares", self.url_root))
			.send()
			.await
			.expect("Failed to send request to get shares");
		if !res.status().is_success() {
			panic!(
				"Failed to get shares, status: {}, body: {:?}",
				res.status(),
				res.text().await
			);
		}

		let shares = res.json::<Vec<Share>>().await.unwrap();
		let id = shares.first().unwrap().id.clone();
		id.split_once('-').unwrap().0.to_string() // extract short id
	}
}

struct FilenRemoteFiles {
	resources: test_utils::TestResources,
}

const SAMPLE_FILE_NAME: &str = "my_test_file.txt";
const SAMPLE_FILE_CONTENT: &str = "Some file content";

impl FilenRemoteFiles {
	async fn setup_with_sample_files() -> Self {
		let test_resources = test_utils::RESOURCES.get_resources().await;
		Self {
			resources: test_resources,
		}
	}

	async fn create_sample_file(&self) {
		let file = self
			.resources
			.client
			.make_file_builder(SAMPLE_FILE_NAME, self.resources.dir.uuid)
			.unwrap()
			.build();
		self.resources
			.client
			.upload_file(file.into(), SAMPLE_FILE_CONTENT.as_bytes())
			.await
			.unwrap();
	}

	fn get_dir(&self) -> String {
		self.resources.dir.meta.name().unwrap().to_string()
	}
}

struct GeneralRcloneClient {
	_config_dir: TempDir, // kept here so it is removed on drop
	rclone_executable_path: std::path::PathBuf,
}

impl GeneralRcloneClient {
	async fn init() -> Self {
		let rclone_config_dir = TempDir::new().unwrap();
		let _ = RcloneInstallation::initialize_unauthenticated(&RcloneInstallationConfig::new(
			rclone_config_dir.path(),
		))
		.await
		.expect("Failed to initialize rclone installation");
		let rclone_executable_path = {
			let mut path = None::<std::path::PathBuf>;
			for entry in rclone_config_dir.path().read_dir().unwrap() {
				let entry = entry.unwrap();
				let file_name = entry.file_name().into_string().unwrap();
				if file_name.contains("rclone") {
					// the actual file name is not known in advance
					path = Some(entry.path().to_path_buf());
					break;
				}
			}
			path
		};
		Self {
			_config_dir: rclone_config_dir,
			rclone_executable_path: rclone_executable_path
				.expect("Failed to find rclone executable"),
		}
	}

	fn execute_command(&self, backend: &RcloneBackend, args: &str) -> assert_cmd::assert::Assert {
		let transformed_args = format!(
			"{} {}",
			backend.backend_args,
			args.replace("backend", backend.backend_type)
		);
		assert_cmd::Command::new(self.rclone_executable_path.as_path())
			.args(transformed_args.split_whitespace())
			.assert()
			.success()
	}
}

struct RcloneBackend {
	backend_type: &'static str,
	backend_args: String,
}

#[tokio::test]
async fn access_shares() {
	// setup remote directory with sample files
	let remote_files = FilenRemoteFiles::setup_with_sample_files().await;

	// start server
	let filen_relay = FilenRelayProcess::start();
	filen_relay.wait_for_ready().await;
	println!("Filen Relay is ready");

	// create share
	filen_relay.login().await;
	let id = filen_relay
		.create_test_share_and_get_id(remote_files.get_dir())
		.await;
	println!("Created share with id: {}", id);

	let rclone = GeneralRcloneClient::init().await;
	let backends = &[RcloneBackend {
		backend_type: "webdav",
		backend_args: format!("--webdav-url {}/webdav/{}", filen_relay.url_root, id),
	}]; // todo: add more backends (S3, FTP, SFTP) here when they are implemented
	for backend in backends {
		println!("Testing operations on backend: {}", backend.backend_type);

		// do various rclone operations to verify the share can be accessed correctly on all backends

		// list files (but none are there)
		rclone.execute_command(backend, "ls :backend:/");

		// create file with content and upload it
		let temp_dir = tempfile::tempdir().unwrap();
		let uploaded_file = temp_dir.path().join(SAMPLE_FILE_NAME);
		std::fs::write(&uploaded_file, SAMPLE_FILE_CONTENT).unwrap();
		rclone
			.execute_command(
				backend,
				&format!("copy {} :backend:/", uploaded_file.to_string_lossy(),),
			)
			.success();

		// verify file is accessible
		rclone
			.execute_command(backend, "ls :backend:/")
			.stdout(predicates::str::contains(SAMPLE_FILE_NAME));

		// verify file content
		rclone
			.execute_command(backend, &format!("cat :backend:/{}", SAMPLE_FILE_NAME))
			.stdout(predicates::str::contains(SAMPLE_FILE_CONTENT));

		// delete file
		rclone
			.execute_command(backend, &format!("delete :backend:/{}", SAMPLE_FILE_NAME))
			.stdout(predicates::str::contains(""));

		// verify file is no longer accessible
		rclone
			.execute_command(backend, "ls :backend:/")
			.stdout(predicates::str::contains(SAMPLE_FILE_NAME).not());

		// todo: add more edge-case operations here?
	}

	// access share via HTTP
	{
		println!("Testing special operations on HTTP UI");
		remote_files.create_sample_file().await;

		// access share via HTTP UI
		let url = format!("{}/s/{}", filen_relay.url_root, id);
		let res = reqwest::Client::new()
			.get(url)
			.send()
			.await
			.expect("Failed to access share via HTTP");
		assert!(res.status().is_success());
		let body = res.text().await.unwrap();
		assert!(body.contains(SAMPLE_FILE_NAME));
	}
}
