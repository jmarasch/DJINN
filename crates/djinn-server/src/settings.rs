//! Reading and writing the admin's settings. Each setting's name lives with
//! the feature that uses it; the ones several features share live here.

use djinn_store::Store;

/// 1 when guests may join with just a name.
pub const GUEST_MODE: &str = "guest_mode";
/// 1 to look up artist photos and bios online.
pub const ONLINE_INFO: &str = "online_artist_info";
/// Contact (email or website) sent to MusicBrainz, which it requires.
pub const MUSICBRAINZ_CONTACT: &str = "musicbrainz_contact";
/// 1 to look up album covers, genres and exact artist matches on
/// MusicBrainz. Before this switch existed a contact turned it on, so
/// that's what it defaults to.
pub const MUSICBRAINZ: &str = "musicbrainz";

/// The contact to send MusicBrainz, when MusicBrainz lookups are on.
pub async fn musicbrainz(store: &Store) -> anyhow::Result<Option<String>> {
    let contact = store
        .get_setting(MUSICBRAINZ_CONTACT)
        .await?
        .unwrap_or_default()
        .trim()
        .to_owned();
    let on = flag(store, MUSICBRAINZ, !contact.is_empty()).await?;
    Ok((on && !contact.is_empty()).then_some(contact))
}

/// Whether any online lookup is on: artist photos and bios, or MusicBrainz.
pub async fn lookups_on(store: &Store) -> bool {
    flag(store, ONLINE_INFO, false).await.unwrap_or(false)
        || musicbrainz(store).await.ok().flatten().is_some()
}

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
