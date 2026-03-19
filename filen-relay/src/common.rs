use std::fmt::Display;

use serde::{Deserialize, Serialize};

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
