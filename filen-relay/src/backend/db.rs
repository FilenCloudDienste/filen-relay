use std::sync::Mutex;

use dioxus::prelude::*;
use filen_sdk_rs::{
    auth::{Client, http::ClientConfig, unauth::UnauthClient},
    fs::categories::{DirType, NonRootFileType}, io::{RemoteDirectory, client_impl::IoSharedClientExt},
};
use rusqlite::Connection;

use crate::{
    common::{ShareId, Share},
    util::UnwrapOnceLock,
};

// todo: is it good (or safe) that this needs to be .lock().unwrap() everywhere?
pub(crate) static DB: UnwrapOnceLock<DbViaOfflineOrRemoteFile> = UnwrapOnceLock::new();

const DB_FILE_NAME: &str = "filen-relay.db";
pub(crate) struct DbViaOfflineOrRemoteFile {
    conn: Mutex<rusqlite::Connection>,
    filen_client: Option<Client>,
    remote_db_dir: Option<RemoteDirectory>,
}

impl DbViaOfflineOrRemoteFile {
    pub(crate) async fn new_from_email_and_password(
        filen_email: String,
        filen_password: &str,
        filen_two_factor_code: Option<&str>,
    ) -> Result<Self> {
        let client = UnauthClient::from_config(ClientConfig::default())?.login(
            filen_email,
            filen_password,
            filen_two_factor_code.unwrap_or("XXXXXX"),
        )
        .await
        .context("Failed to log in to admin Filen")?;
        let remote_db_dir = Self::initialize_from_filen(&client).await?;
        let db = Self {
            conn: Mutex::new(Self::init(None)),
            filen_client: Some(client),
            remote_db_dir: Some(remote_db_dir),
        };
        Ok(db)
    }

    pub(crate) async fn new_from_auth_config(filen_auth_config: String) -> Result<(String, Self)> {
        let client = filen_cli::deserialize_auth_config(&filen_auth_config)
            .context("Failed to deserialize admin Filen auth config")?;
        let admin_email = client.email().to_string();
        let remote_db_dir = Self::initialize_from_filen(&client).await?;
        let db = Self {
            conn: Mutex::new(Self::init(None)),
            filen_client: Some(client),
            remote_db_dir: Some(remote_db_dir),
        };
        Ok((admin_email, db))
    }

    pub(crate) async fn new_from_offline_location(db_dir: Option<&str>) -> Result<Self> {
        Ok(Self {
            conn: Mutex::new(Self::init(db_dir)),
            filen_client: None,
            remote_db_dir: None,
        })
    }

    fn init(db_dir: Option<&str>) -> Connection {
        let db_dir = db_dir.unwrap_or(".").trim_end_matches('/').to_string();
        let conn = rusqlite::Connection::open(format!("{}/{}", db_dir, DB_FILE_NAME))
            .expect("Failed to open database");
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS allowed_users (
                id INTEGER PRIMARY KEY,
                email TEXT NOT NULL UNIQUE
            );
            CREATE TABLE IF NOT EXISTS shares (
                id TEXT PRIMARY KEY,
                root TEXT NOT NULL,
                read_only BOOLEAN NOT NULL,
                password TEXT,
                filen_email TEXT NOT NULL,
                filen_stringified_client TEXT NOT NULL
            );
            ",
        )
        .unwrap();
        // todo: verify schema when loading database?
    conn
    }

    async fn initialize_from_filen(client: &Client) -> anyhow::Result<RemoteDirectory> {
        let local_db_file = std::env::current_dir()?.join(DB_FILE_NAME);
        if tokio::fs::try_exists(&local_db_file).await.context("Failed to check if local database file exists")? {
            tokio::fs::remove_file(&local_db_file).await.context("Failed to remove existing local database file")?;
        }
        match client
            .find_item_at_path(&format!("/.filen-relay/{}", DB_FILE_NAME))
            .await?
        {
            Some(NonRootFileType::File(db_file)) => {
                client
                    .download_file_to_path(
                        db_file.as_ref(),
                        local_db_file,
                        None,
                    )
                    .await?;
            }
            _ => {
                dioxus::logger::tracing::warn!(
                    "Filen relay database not found at /.filen-relay/{} in admin Filen account, starting with empty database",
                    DB_FILE_NAME
                );
            }
        };
        if let DirType::Dir(remote_db_dir) = client
            .find_or_create_dir(".filen-relay")
            .await
            .context("Failed to create .filen-relay dir in admin Filen account")?
        {
            Ok(remote_db_dir.into_owned())
        } else {
            Err(anyhow::anyhow!("Failed to find or create .filen-relay dir in admin Filen account"))
        }
    }

    // todo: make this more async so that other things can be resumed until the upload is done (can be done at call site probably)
    async fn write_to_filen(&self) -> anyhow::Result<()> {
        let Some(client) = &self.filen_client else {
            return Ok(()); // it is not needed
        };
        client
            .upload_file_from_path(
                &DirType::Dir(std::borrow::Cow::Borrowed(self.remote_db_dir.as_ref().unwrap())),
                std::env::current_dir()?.join(DB_FILE_NAME),
                None,
            )
            .await
            .context("Failed to upload database file to admin Filen account")?;
        Ok(())
    }

    pub(crate) fn get_allowed_users(&self) -> Result<Vec<String>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = db.prepare("SELECT email FROM allowed_users")?;
        let user_iter = stmt.query_map([], |row| row.get(0))?;
        let mut users = Vec::new();
        for user in user_iter {
            users.push(user?);
        }
        Ok(users)
    }

    pub(crate) async fn add_allowed_user(&self, email: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO allowed_users (email) VALUES (?1)",
            rusqlite::params![email],
        )?;
        self.write_to_filen().await?;
        Ok(())
    }

    pub(crate) async fn remove_allowed_user(&self, email: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "DELETE FROM allowed_users WHERE email = ?1",
            rusqlite::params![email],
        )?;
        self.write_to_filen().await?;
        Ok(())
    }

    pub(crate) async fn clear_allowed_users(&self) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM allowed_users", [])?;
        self.write_to_filen().await?;
        Ok(())
    }

    pub(crate) fn get_shares(&self) -> Result<Vec<Share>> {
        let db = self.conn.lock().unwrap();
        let mut stmt = 
            db.prepare("SELECT id, root, read_only, password, filen_email, filen_stringified_client FROM shares")?;
        let server_iter = stmt.query_map([], |row| {
            Ok(Share {
                id: row.get(0)?,
                root: row.get(1)?,
                read_only: row.get(2)?,
                password: row.get(3)?,
                filen_email: row.get(4)?,
                filen_stringified_client: row.get(5)?,
            })
        })?;
        let mut servers = Vec::new();
        for server in server_iter {
            servers.push(server?);
        }
        Ok(servers)
    }

    pub(crate) async fn create_share(&self, share: &Share) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO shares (id, root, read_only, password, filen_email, filen_stringified_client) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![share.id, share.root, share.read_only, share.password, share.filen_email, share.filen_stringified_client],
        )?;
        self.write_to_filen().await?;
        Ok(())
    }

    pub(crate) async fn delete_share(&self, id: &ShareId) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM shares WHERE id = ?1", rusqlite::params![id])?;
        self.write_to_filen().await?;
        Ok(())
    }
}
