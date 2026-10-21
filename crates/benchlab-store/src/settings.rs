//! Small key/value settings repository.

use benchlab_core::Id;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;

use crate::Store;

pub const LAST_PROJECT_KEY: &str = "last_project_id";

impl Store {
    pub async fn get_setting(&self, key: &str) -> Result<Option<Value>, crate::StoreError> {
        let key = key.to_owned();
        self.call(move |conn| {
            let value: Option<String> = conn
                .query_row(
                    "SELECT value_json FROM app_settings WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()?;
            match value {
                Some(json) => serde_json::from_str(&json)
                    .map(Some)
                    .map_err(crate::StoreError::from),
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn set_setting(&self, key: &str, value: Value) -> Result<(), crate::StoreError> {
        let key = key.to_owned();
        let value = serde_json::to_string(&value)?;
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO app_settings (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn remove_setting(&self, key: &str) -> Result<(), crate::StoreError> {
        let key = key.to_owned();
        self.call(move |conn| {
            conn.execute("DELETE FROM app_settings WHERE key = ?1", params![key])?;
            Ok(())
        })
        .await
    }

    pub async fn set_last_project(&self, id: Option<Id>) -> Result<(), crate::StoreError> {
        match id {
            Some(id) => {
                self.set_setting(LAST_PROJECT_KEY, Value::String(id.to_string()))
                    .await
            }
            None => self.remove_setting(LAST_PROJECT_KEY).await,
        }
    }

    pub async fn last_project_id(&self) -> Result<Option<Id>, crate::StoreError> {
        match self.get_setting(LAST_PROJECT_KEY).await? {
            Some(Value::String(value)) => Id::parse_str(&value).map(Some).map_err(|error| {
                crate::StoreError::Invalid(format!("invalid last project id: {error}"))
            }),
            Some(_) => Err(crate::StoreError::Invalid(
                "last project setting has the wrong type".to_owned(),
            )),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_project_round_trips() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let id = Id::now_v7();
            store.set_last_project(Some(id)).await.unwrap();
            assert_eq!(store.last_project_id().await.unwrap(), Some(id));
            store.set_last_project(None).await.unwrap();
            assert_eq!(store.last_project_id().await.unwrap(), None);
        });
    }
}
