use std::fmt::Display;

use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Share {
    pub id: ShareId,
    pub root: String,
    pub read_only: bool,
    pub password: Option<String>,
    pub filen_email: String,
    pub filen_stringified_client: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub(crate) struct ShareId(String);

impl ShareId {
    pub fn new() -> Self {
        ShareId(uuid::Uuid::new_v4().to_string())
    }

    pub fn short(&self) -> &str {
        self.0.split_once('-').unwrap().0
    }
}

impl From<String> for ShareId {
    fn from(value: String) -> Self {
        ShareId(value)
    }
}

impl Display for ShareId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "server")]
impl rusqlite::types::FromSql for ShareId {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        Ok(ShareId(s))
    }
}

#[cfg(feature = "server")]
impl rusqlite::ToSql for ShareId {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.0.clone()),
        ))
    }
}

#[derive(EnumIter, PartialEq, Clone, Default)]
pub(crate) enum ServerType {
    #[default]
    Http,
    Webdav,
    S3,
    Ftp,
    Sftp,
}

impl ServerType {
    pub(crate) fn to_url_segment(&self) -> &'static str {
        match self {
            ServerType::Http => "s",
            ServerType::Webdav => "webdav",
            ServerType::S3 => "s3",
            ServerType::Ftp => "ftp",
            ServerType::Sftp => "sftp",
        }
    }

    pub(crate) fn to_str(&self) -> &'static str {
        match self {
            ServerType::Http => "http",
            ServerType::Webdav => "webdav",
            ServerType::S3 => "s3",
            ServerType::Ftp => "ftp",
            ServerType::Sftp => "sftp",
        }
    }
}

impl Serialize for ServerType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_url_segment())
    }
}

impl<'de> Deserialize<'de> for ServerType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "s" => Ok(ServerType::Http),
            "webdav" => Ok(ServerType::Webdav),
            "s3" => Ok(ServerType::S3),
            "ftp" => Ok(ServerType::Ftp),
            "sftp" => Ok(ServerType::Sftp),
            _ => Err(serde::de::Error::custom(format!(
                "Unknown server type: {}",
                s
            ))),
        }
    }
}

impl Display for ServerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ServerType::Http => "Web",
                ServerType::Webdav => "WebDAV",
                ServerType::S3 => "S3",
                ServerType::Ftp => "FTP",
                ServerType::Sftp => "SFTP",
            }
        )
    }
}
