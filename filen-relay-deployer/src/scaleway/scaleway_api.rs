use anyhow::Result;
use serde::{Deserialize, Serialize};

pub(crate) struct ScalewayApi {
	client: reqwest::Client,
	organization_id: String,
	region: String,
}

#[derive(Deserialize)]
pub struct ListProjectsResponse {
	pub projects: Vec<ListProjectsResponseItem>,
}

#[derive(Deserialize)]
pub struct ListProjectsResponseItem {
	pub id: String,
	pub name: String,
}

#[derive(Deserialize)]
pub struct ContainersNamespacesListResponse {
	pub namespaces: Vec<ContainersNamespacesListResponseItem>,
}

#[derive(Deserialize)]
pub struct ContainersNamespacesListResponseItem {
	pub id: String,
	pub name: String,
	pub status: String,
}

#[derive(Deserialize)]
pub struct ContainersListResponseItem {
	pub id: String,
	pub domain_name: String,
}

impl ScalewayApi {
	pub fn new(api_key: &str, organization_id: &str, region: &str) -> Self {
		let client = reqwest::Client::builder()
			.user_agent("filen-relay-deployer")
			.default_headers({
				let mut headers = reqwest::header::HeaderMap::new();
				headers.insert(
					"X-Auth-Token",
					reqwest::header::HeaderValue::from_str(api_key).unwrap(),
				);
				headers
			})
			.build()
			.unwrap();
		Self {
			client,
			organization_id: organization_id.to_string(),
			region: region.to_string(),
		}
	}

	async fn get<T: for<'a> Deserialize<'a>>(&self, endpoint: &str) -> Result<T> {
		let url = format!(
			"https://api.scaleway.com/{}",
			endpoint.trim_start_matches('/')
		);
		log::debug!("Scaleway API: GET {}", url);
		let response = self.client.get(url).send().await?;
		let text = response.text().await?;
		log::debug!("Response from {}: {}", endpoint, text);
		let result = serde_json::from_str::<T>(&text)?;
		Ok(result)
	}

	async fn post<T: for<'a> Deserialize<'a>, B: Serialize>(
		&self,
		endpoint: &str,
		body: &B,
	) -> Result<T> {
		let url = format!(
			"https://api.scaleway.com/{}",
			endpoint.trim_start_matches('/')
		);
		log::debug!("Scaleway API: POST {}", url);
		log::debug!("Request body: {}", serde_json::to_string(body)?);
		let response = self.client.post(url).json(body).send().await?;
		let text = response.text().await?;
		log::debug!("Response from {}: {}", endpoint, text);
		let result = serde_json::from_str::<T>(&text)?;
		Ok(result)
	}

	pub async fn list_projects(&self) -> Result<Vec<ListProjectsResponseItem>> {
		let response = self
			.get::<ListProjectsResponse>(&format!(
				"account/v3/projects?organization_id={}",
				self.organization_id
			))
			.await?;
		Ok(response.projects)
	}

	pub async fn list_containers_namespaces(
		&self,
	) -> Result<Vec<ContainersNamespacesListResponseItem>> {
		let response = self
			.get::<ContainersNamespacesListResponse>(&format!(
				"containers/v1beta1/regions/{}/namespaces",
				self.region
			))
			.await?;
		Ok(response.namespaces)
	}

	pub async fn create_containers_namespace(
		&self,
		name: &str,
		project_id: &str,
	) -> Result<ContainersNamespacesListResponseItem> {
		let response = self
			.post::<ContainersNamespacesListResponseItem, _>(
				&format!("containers/v1beta1/regions/{}/namespaces", self.region),
				&serde_json::json!({
					"name": name,
					"project_id": project_id,
				}),
			)
			.await?;
		Ok(response)
	}

	pub async fn get_containers_namespace(
		&self,
		namespace_id: &str,
	) -> Result<ContainersNamespacesListResponseItem> {
		let response = self
			.get::<ContainersNamespacesListResponseItem>(&format!(
				"containers/v1beta1/regions/{}/namespaces/{}",
				self.region, namespace_id
			))
			.await?;
		Ok(response)
	}

	pub async fn create_container(
		&self,
		body: &serde_json::Value,
	) -> Result<ContainersListResponseItem> {
		let response = self
			.post::<ContainersListResponseItem, _>(
				&format!("containers/v1beta1/regions/{}/containers", self.region),
				body,
			)
			.await?;
		Ok(response)
	}

	pub async fn deploy_container(&self, container_id: &str) -> Result<()> {
		self.post::<serde_json::Value, _>(
			&format!(
				"containers/v1beta1/regions/{}/containers/{}/deploy",
				self.region, container_id
			),
			&serde_json::json!({}),
		)
		.await?;
		Ok(())
	}
}
