//! Reading and writing the admin's settings. Each setting's name lives with
//! the feature that uses it; the ones several features share live here.

use djinn_store::Store;

/// 1 when guests may join with just a name.
pub const GUEST_MODE: &str = "guest_mode";
/// 1 to look up artist photos and bios online.
pub const ONLINE_INFO: &str = "online_artist_info";
/// Contact (email or website) sent to MusicBrainz; empty turns it off.
pub const MUSICBRAINZ_CONTACT: &str = "musicbrainz_contact";

/// A number setting, or `default` when it's missing or not a number.
pub async fn number(store: &Store, key: &str, default: i64) -> anyhow::Result<i64> {
    Ok(store
        .get_setting(key)
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(default))
}

/// An on/off setting, saved as 1 or 0.
pub async fn flag(store: &Store, key: &str, default: bool) -> anyhow::Result<bool> {
    Ok(number(store, key, i64::from(default)).await? != 0)
}

pub async fn set_flag(store: &Store, key: &str, on: bool) -> anyhow::Result<()> {
    store.set_setting(key, if on { "1" } else { "0" }).await?;
    Ok(())
}
