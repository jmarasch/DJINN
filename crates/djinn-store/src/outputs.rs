//! Audio outputs the admin has set up.

use serde::Serialize;
use sqlx::FromRow;

use crate::{Result, Store};

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct OutputRow {
    pub id: i64,
    /// The engine's device id, or empty for the system default device.
    #[serde(rename = "device_id")]
    pub device_name: String,
    pub label: String,
    pub enabled: bool,
    pub volume_trim: f64,
    pub delay_ms: i64,
}

impl OutputRow {
    pub fn is_system_default(&self) -> bool {
        self.device_name.is_empty()
    }
}

macro_rules! output_columns {
    () => {
        "id, device_name, label, enabled, volume_trim, delay_ms"
    };
}

impl Store {
    /// All outputs, the system default first, then in the order added.
    pub async fn outputs(&self) -> Result<Vec<OutputRow>> {
        let rows = sqlx::query_as(concat!(
            "SELECT ",
            output_columns!(),
            " FROM outputs ORDER BY device_name != '', position, id"
        ))
        .fetch_all(self.pool())
        .await?;
        Ok(rows)
    }

    pub async fn output(&self, id: i64) -> Result<Option<OutputRow>> {
        let row = sqlx::query_as(concat!(
            "SELECT ",
            output_columns!(),
            " FROM outputs WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row)
    }

    /// Adds a device, or returns the existing output for it.
    pub async fn add_output(&self, device_id: &str, label: &str) -> Result<OutputRow> {
        sqlx::query(
            "INSERT INTO outputs (device_name, label, position) \
             VALUES (?, ?, (SELECT coalesce(max(position), 0) + 1 FROM outputs)) \
             ON CONFLICT (device_name) DO NOTHING",
        )
        .bind(device_id)
        .bind(label)
        .execute(self.pool())
        .await?;
        let row = sqlx::query_as(concat!(
            "SELECT ",
            output_columns!(),
            " FROM outputs WHERE device_name = ?"
        ))
        .bind(device_id)
        .fetch_one(self.pool())
        .await?;
        Ok(row)
    }

    pub async fn update_output(
        &self,
        id: i64,
        enabled: bool,
        volume_trim: f64,
        delay_ms: i64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE outputs SET enabled = ?, volume_trim = ?, delay_ms = ? WHERE id = ?",
        )
        .bind(enabled)
        .bind(volume_trim)
        .bind(delay_ms)
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Removes an output. The system default can't be removed.
    pub async fn delete_output(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM outputs WHERE id = ? AND device_name != ''")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn outputs_keep_the_system_default() {
        let store = Store::open_in_memory().await.unwrap();
        let list = store.outputs().await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].is_system_default());
        assert!(!store.delete_output(list[0].id).await.unwrap());

        let speaker = store
            .add_output("pulseaudio:bluez_output.AA", "Speaker")
            .await
            .unwrap();
        let again = store
            .add_output("pulseaudio:bluez_output.AA", "Other name")
            .await
            .unwrap();
        assert_eq!(again, speaker, "adding a device twice keeps one output");
        assert!(store
            .update_output(speaker.id, true, 0.8, 250)
            .await
            .unwrap());
        let list = store.outputs().await.unwrap();
        assert_eq!(
            list.iter()
                .map(|o| (o.label.as_str(), o.delay_ms))
                .collect::<Vec<_>>(),
            [("System default", 0), ("Speaker", 250)]
        );
        assert!(store.delete_output(speaker.id).await.unwrap());
        assert_eq!(store.outputs().await.unwrap().len(), 1);
    }
}
