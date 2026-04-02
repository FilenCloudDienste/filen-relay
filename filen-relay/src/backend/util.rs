use std::{ops::Deref, sync::OnceLock};

use anyhow::{Context, Result};
use filen_sdk_rs::{
	auth::Client,
	fs::HasUUID,
	io::{RemoteDirectory, RemoteFile},
};
use filen_types::fs::UuidStr;

/// A wrapper around OnceLock that panics if accessed before initialization.
/// This is useful for when you know the value will be initialized and want to avoid
/// explicitly calling unwrap() everywhere.
pub struct UnwrapOnceLock<T>(OnceLock<T>);

impl<T> UnwrapOnceLock<T> {
	pub const fn new() -> Self {
		UnwrapOnceLock(OnceLock::new())
	}
}

impl<T> UnwrapOnceLock<T> {
	pub fn init(&self, val: T) {
		let _ = self.0.set(val);
	}
}

impl<T> Deref for UnwrapOnceLock<T> {
	type Target = T;

	fn deref(&self) -> &Self::Target {
		self.0.get().expect("OnceLock not initialized")
	}
}

pub(crate) async fn find_path_for_dir(client: &Client, dir: RemoteDirectory) -> Result<String> {
	dbg!("Finding path for dir", dir.uuid());
	dbg!("Root dir", client.root().uuid());
	dbg!("Parent uuid", dir.parent);
	if dir.uuid() == client.root().uuid() {
		return Ok(String::new());
	}
	let parent_uuid = UuidStr::try_from(dir.parent)
		.context("Failed to get parent UUID for directory not inside regular file tree")?;
	let parent_path = if &parent_uuid == client.root().uuid() {
		String::new()
	} else {
		let parent = client.get_dir(parent_uuid).await.with_context(|| {
			format!("Failed to find parent {parent_uuid} while traversing path")
		})?;
		Box::pin(find_path_for_dir(client, parent)).await?
	};
	Ok(format!(
		"{}/{}",
		parent_path,
		dir.meta.name().unwrap_or("INVALID_NAME")
	))
}

pub(crate) async fn find_path_for_file(client: &Client, file: RemoteFile) -> Result<String> {
	let parent_uuid = UuidStr::try_from(file.parent)
		.context("Failed to get parent UUID for file not inside regular file tree")?;
	let parent = client
		.get_dir(parent_uuid)
		.await
		.with_context(|| format!("Failed to find parent {parent_uuid} while traversing path"))?;
	let parent_path = find_path_for_dir(client, parent).await?;
	Ok(format!(
		"{}/{}",
		parent_path,
		file.meta.name().unwrap_or("INVALID_NAME")
	))
}
