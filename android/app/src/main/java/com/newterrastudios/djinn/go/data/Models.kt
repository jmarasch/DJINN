package com.newterrastudios.djinn.go.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

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
)

@Serializable
data class Page<T>(val items: List<T>, val total: Long)

@Serializable
data class ArtistSummary(
    val id: Long,
    val name: String,
    @SerialName("art_hash") val artHash: String? = null,
    @SerialName("track_count") val trackCount: Long = 0,
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
)

@Serializable
data class AlbumDetail(
    val id: Long,
    val title: String,
    @SerialName("artist_id") val artistId: Long,
    val artist: String,
    val year: Int? = null,
    @SerialName("art_hash") val artHash: String? = null,
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
)

/** The search read as a description, like "80's hair metal". */
@Serializable
data class Described(
    val summary: List<String> = emptyList(),
    val tracks: List<Track> = emptyList(),
)

@Serializable
data class GoPlaylist(
    val id: Long,
    val name: String,
    val tracks: Long = 0,
)

@Serializable
data class GoPlaylistDetail(
    val id: Long,
    val name: String,
    /** The account's own: only those can be changed here. */
    @SerialName("user_id") val userId: Long = 0,
    val songs: List<Track> = emptyList(),
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
data class LikedIds(val tracks: List<Long> = emptyList())

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
data class Permissions(val downloads: Boolean = false, val convert: Boolean = false)

@Serializable
data class Me(val username: String, val permissions: Permissions = Permissions())

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

@Serializable
data class ResumeSave(
    val queue: List<Long>,
    val position: Int,
    @SerialName("position_ms") val positionMs: Long,
)
