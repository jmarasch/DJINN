package com.newterrastudios.djinn.go.data

import kotlinx.serialization.SerialName
import com.newterrastudios.djinn.go.i18n.t
import kotlinx.serialization.Serializable

@Serializable
data class ServerHello(val name: String, val id: String, val encrypted: Boolean = false)

/** The admin's rules for songs kept here. Used as they were last heard while away. */
@Serializable
data class KeepRules(
    @SerialName("sign_out") val signOut: String = "delete",
    val away: String = "lock",
    @SerialName("away_days") val awayDays: Int = 30,
)

@Serializable
data class Track(
    val id: Long,
    val title: String,
    val artist: String? = null,
    val album: String? = null,
    @SerialName("artist_id") val artistId: Long? = null,
    @SerialName("album_id") val albumId: Long? = null,
    @SerialName("track_no") val trackNo: Int? = null,
    @SerialName("duration_ms") val durationMs: Long? = null,
    @SerialName("art_hash") val artHash: String? = null,
    /** In the library lists: plays (party and GO) and how many people favorited it. */
    val plays: Long = 0,
    val likes: Long = 0,
)

@Serializable
data class Page<T>(val items: List<T>, val total: Long)

@Serializable
data class ArtistSummary(
    val id: Long,
    val name: String,
    @SerialName("art_hash") val artHash: String? = null,
    @SerialName("track_count") val trackCount: Long = 0,
    /** In the library lists: plays (party and GO) and how many people favorited it. */
    val plays: Long = 0,
    val likes: Long = 0,
)

@Serializable
data class Artist(
    val id: Long,
    val name: String,
    @SerialName("art_hash") val artHash: String? = null,
)

@Serializable
data class Album(
    val id: Long,
    val title: String,
    @SerialName("artist_id") val artistId: Long,
    val artist: String,
    val year: Int? = null,
    @SerialName("art_hash") val artHash: String? = null,
    /** Its folder's name, when another album has the same name and artist. */
    val label: String? = null,
    /** In the library lists: plays (party and GO) and how many people favorited it. */
    val plays: Long = 0,
    val likes: Long = 0,
)

@Serializable
data class AlbumDetail(
    val id: Long,
    val title: String,
    @SerialName("artist_id") val artistId: Long,
    val artist: String,
    val year: Int? = null,
    @SerialName("art_hash") val artHash: String? = null,
    val label: String? = null,
    val tracks: List<Track> = emptyList(),
)

@Serializable
data class ArtistDetail(
    val id: Long,
    val name: String,
    @SerialName("art_hash") val artHash: String? = null,
    val bio: String? = null,
    @SerialName("top_tracks") val topTracks: List<Track> = emptyList(),
    val albums: List<Album> = emptyList(),
    @SerialName("appears_on") val appearsOn: List<Album> = emptyList(),
    @SerialName("other_tracks") val otherTracks: List<Track> = emptyList(),
)

@Serializable
data class SearchResults(
    val tracks: List<Track> = emptyList(),
    val albums: List<Album> = emptyList(),
    val artists: List<Artist> = emptyList(),
    val described: Described? = null,
    /** The server's AI reader might understand this better: see [GoApi.ask]. */
    val ask: Boolean = false,
    /** Songs whose lyrics have the words typed, with the line that matched. */
    val lyrics: List<LyricHit> = emptyList(),
)

/** A timed line of lyrics: when it starts, in milliseconds. */
@Serializable
data class LyricLine(val ms: Long, val text: String)

/** A song's lyrics: timed lines when there are some, otherwise plain words. */
@Serializable
data class Lyrics(
    val lines: List<LyricLine>? = null,
    val plain: String? = null,
    val instrumental: Boolean = false,
    val source: String = "",
)

/** A song found by its words. */
@Serializable
data class LyricHit(val track: Track, val line: String)

/** The search read as a description, like "80's hair metal". */
@Serializable
data class Described(
    val summary: List<String> = emptyList(),
    val tracks: List<Track> = emptyList(),
)

/** Whether this server can read YouTube playlist links. */
@Serializable
data class YoutubeLinks(@SerialName("has_key") val hasKey: Boolean = false)

/** Every genre in the library, by name. */
@Serializable
data class GenreList(
    val genres: List<GenreSummary> = emptyList(),
    /** Related genres, as places in [genres]. */
    val links: List<GenreLink> = emptyList(),
    /** Links among the biggest genres only, for the cloud's top level. */
    @SerialName("top_links") val topLinks: List<GenreLink> = emptyList(),
)

@Serializable
data class GenreLink(val a: Int, val b: Int, val strength: Float)

@Serializable
data class GenreSummary(val name: String, val songs: Long = 0)

/** One genre: its most played songs, its artists, and the genres near it. */
@Serializable
data class GenrePage(
    val name: String,
    /** Songs with it, which can be more than are listed. */
    val total: Long = 0,
    @SerialName("artist_total") val artistTotal: Long = 0,
    val tracks: List<Track> = emptyList(),
    val artists: List<ArtistSummary> = emptyList(),
    val related: List<GenreSummary> = emptyList(),
)

/** Where a letter or year starts in a long list. */
@Serializable
data class Jump(val label: String, val offset: Int)

@Serializable
data class GoPlaylist(
    val id: Long,
    val name: String,
    val tracks: Long = 0,
    /** Who made it, for shared ones. */
    val owner: String? = null,
    val mine: Boolean = true,
    val shared: Boolean = false,
)

/** One line of a playlist, in order. Without a song index it can't play. */
@Serializable
data class PlaylistRow(
    @SerialName("entry_id") val entryId: Long,
    /** Its entry is this one song. A song of an album or artist goes with the whole entry. */
    val removable: Boolean = false,
    @SerialName("song_index") val songIndex: Int? = null,
    val title: String,
    val artist: String? = null,
    val problem: String? = null,
) {
    /** Why it can't play, in words. */
    val problemText: String?
        get() = when (problem) {
            null -> null
            "not_in_library" -> t("problem.not_in_library")
            "file_missing" -> t("problem.file_missing")
            else -> t("problem.wont_play")
        }
}

@Serializable
data class ShareTarget(val id: Long, val name: String)

@Serializable
data class ImportRequest(val text: String, val filename: String? = null, val name: String? = null)

@Serializable
data class ImportedPlaylist(val id: Long, val name: String, val found: Int = 0, val missing: Int = 0)

@Serializable
data class ImportResult(val playlists: List<ImportedPlaylist> = emptyList(), val skipped: Int = 0)

@Serializable
data class SharedSetting(val shared: Boolean)

/**
 * A playlist file's text. Apple Music's export is UTF-16; the rest are UTF-8.
 * Only a byte order mark says which, and the server doesn't want the mark.
 */
fun importText(bytes: ByteArray): String {
    fun at(i: Int) = bytes.getOrNull(i)?.toInt()?.and(0xff)
    val charset = when {
        at(0) == 0xff && at(1) == 0xfe -> Charsets.UTF_16LE
        at(0) == 0xfe && at(1) == 0xff -> Charsets.UTF_16BE
        else -> Charsets.UTF_8
    }
    return String(bytes, charset).removePrefix("\uFEFF")
}

@Serializable
data class GoPlaylistDetail(
    val id: Long,
    val name: String,
    /** The account's own: only those can be changed here. */
    val mine: Boolean = true,
    val owner: String? = null,
    val shared: Boolean = false,
    val songs: List<Track> = emptyList(),
    /** Everything in it, in order, flagged where it can't play. */
    val rows: List<PlaylistRow> = emptyList(),
)

@Serializable
data class GoResume(
    val queue: List<Long>,
    val position: Int,
    @SerialName("position_ms") val positionMs: Long,
    val songs: List<Track> = emptyList(),
)

@Serializable
data class GoHome(
    val resume: GoResume? = null,
    val recent: List<Track> = emptyList(),
    val playlists: List<GoPlaylist> = emptyList(),
    val newest: List<Track> = emptyList(),
)

@Serializable
data class LikedIds(
    val tracks: List<Long> = emptyList(),
    val albums: List<Long> = emptyList(),
    val artists: List<Long> = emptyList(),
)

/** Everything you like, newest first. */
@Serializable
data class Liked(
    val tracks: List<Track> = emptyList(),
    val albums: List<Album> = emptyList(),
    val artists: List<ArtistSummary> = emptyList(),
)

@Serializable
data class PlayLinks(
    val url: String,
    val mime: String = "audio/mpeg",
    val gain: Float = 1f,
    /** The server would convert for this account. */
    @SerialName("can_convert") val canConvert: Boolean = false,
)

/** What tapping a song does, on every device: "play", "queue" (next) or "end". */
@Serializable
data class GoPrefs(val tap: String = "play")

@Serializable
data class VersionQuery(val ids: List<Long>)

/** Changes when a song's file does. */
@Serializable
data class SongVersion(val id: Long, val version: String, val size: Long = 0)

@Serializable
data class NewPlaylist(val name: String, val songs: List<Long> = emptyList())

@Serializable
data class PlaylistName(val name: String)

@Serializable
data class Permissions(
    val downloads: Boolean = false,
    val convert: Boolean = false,
    @SerialName("share_playlists") val sharePlaylists: Boolean = false,
)

@Serializable
data class Me(val id: Long = 0, val username: String, val permissions: Permissions = Permissions())

/** A link to a copy to keep on this phone. */
@Serializable
data class OfflineLink(
    val url: String,
    val mime: String,
    val gain: Float = 1f,
    val converted: Boolean = false,
    @SerialName("convert_refused") val convertRefused: Boolean = false,
    val version: String,
)

@Serializable
data class SongIds(val songs: List<Long>)

/** A finished sign-in, or the password was right and a code is needed. */
@Serializable
data class SignInReply(
    val token: String? = null,
    val me: Me? = null,
    @SerialName("two_step") val twoStep: Boolean = false,
    val ticket: String? = null,
)

@Serializable
data class Login(
    val username: String,
    val password: String,
    @SerialName("device_token") val deviceToken: Boolean = true,
)

@Serializable
data class TwoStep(
    val ticket: String,
    val code: String,
    @SerialName("device_token") val deviceToken: Boolean = true,
)

@Serializable
data class PlayReport(
    @SerialName("track_id") val trackId: Long,
    @SerialName("ms_played") val msPlayed: Long,
    /** When it was heard, for a play reported later. */
    val at: Long? = null,
)

/** Your year on GO, from `/api/go/year`. */
@Serializable
data class GoYear(
    val year: Int,
    val years: List<Int> = emptyList(),
    val plays: Long = 0,
    val minutes: Long = 0,
    val songs: Long = 0,
    val requests: Long = 0,
    @SerialName("top_songs") val topSongs: List<YearSong> = emptyList(),
    @SerialName("top_artists") val topArtists: List<YearArtist> = emptyList(),
    val months: List<Long> = emptyList(),
    @SerialName("busiest_month") val busiestMonth: Int? = null,
    val first: Track? = null,
)

@Serializable
data class YearSong(
    @SerialName("track_id") val trackId: Long,
    val title: String,
    val artist: String? = null,
    @SerialName("album_id") val albumId: Long? = null,
    val plays: Long = 0,
)

@Serializable
data class YearArtist(
    @SerialName("artist_id") val artistId: Long,
    val name: String,
    val plays: Long = 0,
)

/** A page of an artist's songs, with how many there are in all. */
@Serializable
data class ArtistSongs(val total: Long = 0, val tracks: List<Track> = emptyList())

/** Asking the admin for a song the library lacks. */
@Serializable
data class NewWish(val artist: String, val title: String)

/** A listener saying a song needs fixing; `reason` is one of [REPORT_REASONS]. */
@Serializable
data class SongReport(val reason: String, val note: String? = null)

/** Why a song needs fixing, as the server names them, with what to show. */
val REPORT_REASONS = listOf(
    "wrong_info" to "The title, artist or album is wrong",
    "wrong_cover" to "The cover is wrong",
    "sounds_bad" to "It sounds bad, skips or cuts off",
    "wrong_song" to "It plays a different song",
    "other" to "Something else",
)

@Serializable
data class ResumeSave(
    val queue: List<Long>,
    val position: Int,
    @SerialName("position_ms") val positionMs: Long,
)
