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

/// A setting's value when it was never set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Default {
    Flag(bool),
    Number(i64),
}

/// On/off and number settings the admin pages show, with the defaults
/// their features use (keep in step), for "changed from default".
pub const DEFAULTS: &[(&str, Default)] = &[
    (GUEST_MODE, Default::Flag(false)),
    (crate::tv_api::TV_PAGE, Default::Flag(true)),
    (crate::tv_api::TV_SOUND, Default::Flag(false)),
    (crate::cast_api::PLAY_HERE, Default::Flag(false)),
    (crate::jukebox::MAX_QUEUE, Default::Number(0)),
    (
        crate::jukebox::PER_USER_LIMIT,
        Default::Number(crate::jukebox::DEFAULT_PER_USER_LIMIT),
    ),
    (crate::jukebox::SKIP_VOTES, Default::Number(0)),
    (
        crate::jukebox::HISTORY_WINDOW,
        Default::Number(crate::jukebox::DEFAULT_HISTORY_WINDOW),
    ),
    (crate::jukebox::AVOID_COPIES, Default::Flag(true)),
    (crate::jukebox::SMART_PLAY, Default::Flag(true)),
    (crate::jukebox::SMART_PLAYLISTS, Default::Flag(false)),
    (crate::jukebox::SMART_STRENGTH, Default::Number(100)),
    (crate::jukebox::CROSSFADE, Default::Number(0)),
    (crate::audio_api::SOUND_ANALYSIS, Default::Flag(false)),
    (ONLINE_INFO, Default::Flag(false)),
    (crate::lyrics::LYRICS_LOOKUP, Default::Flag(false)),
    (crate::lyrics::LYRICS_PAGE, Default::Flag(true)),
    (crate::nodes::NODES_JOIN, Default::Flag(false)),
    (crate::library_api::ARTIST_PICTURES, Default::Flag(false)),
    ("show_collaboration_artists", Default::Flag(true)),
    (djinn_library::ORIGINAL_YEARS_KEY, Default::Flag(true)),
    (djinn_store::ALBUM_FOLDERS_KEY, Default::Flag(true)),
    (
        crate::smart::GENRE_PAGE_SIZE,
        Default::Number(crate::smart::GENRE_PAGE_DEFAULT),
    ),
    (
        crate::smart::GENRE_REFRESH_HOURS,
        Default::Number(crate::smart::GENRE_REFRESH_DEFAULT),
    ),
    (crate::scheduled_check::ON, Default::Flag(false)),
    (crate::scheduled_check::HOUR, Default::Number(4)),
    (crate::ai::GENRE_TAGS, Default::Flag(true)),
    (crate::go::GO_ENABLED, Default::Flag(false)),
    (crate::go::GO_ALLOW_REMOTE, Default::Flag(false)),
    ("go_allow_plain_remote", Default::Flag(false)),
    (crate::go::GO_UPNP, Default::Flag(false)),
    (crate::go_announce::GO_DISCOVERABLE, Default::Flag(true)),
    (crate::tls::ENCRYPTION, Default::Flag(true)),
    (crate::two_step::ADMIN_TWO_STEP, Default::Flag(false)),
    (crate::convert::CONVERT_ENABLED, Default::Flag(false)),
    (crate::convert::CONVERT_KBPS, Default::Number(192)),
    (crate::convert::CONVERT_JOBS, Default::Number(1)),
    (crate::convert::CONVERT_CACHE_MB, Default::Number(2048)),
    (crate::submissions::UPLOAD_MAX_MB, Default::Number(15)),
    (crate::wishes_api::SEARCH_HISTORY, Default::Flag(true)),
    (crate::wishes_api::WISHLIST_EVERYONE, Default::Flag(false)),
    ("update_check", Default::Flag(true)),
    ("update_auto", Default::Flag(false)),
];

/// The settings in [`DEFAULTS`] set to something other than their default.
pub async fn changed(store: &Store) -> anyhow::Result<Vec<&'static str>> {
    let mut out = Vec::new();
    for &(key, default) in DEFAULTS {
        let differs = match default {
            Default::Flag(on) => flag(store, key, on).await? != on,
            Default::Number(n) => number(store, key, n).await? != n,
        };
        if differs {
            out.push(key);
        }
    }
    Ok(out)
}
