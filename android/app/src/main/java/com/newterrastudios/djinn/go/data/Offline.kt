package com.newterrastudios.djinn.go.data

import com.newterrastudios.djinn.go.player.EQ_BAND_HZ
import com.newterrastudios.djinn.go.player.EQ_MAX_BAND_DB
import com.newterrastudios.djinn.go.player.EQ_MAX_OUTPUT_DB
import com.newterrastudios.djinn.go.player.EQ_MIN_OUTPUT_DB
import com.newterrastudios.djinn.go.player.EqSettings
import com.newterrastudios.djinn.go.player.VisualLook
import kotlinx.serialization.encodeToString
import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.webkit.MimeTypeMap
import androidx.core.content.edit
import androidx.core.net.toUri
import androidx.room.ColumnInfo
import androidx.room.Dao
import androidx.room.Database
import androidx.room.Entity
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.PrimaryKey
import androidx.room.Query
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.withContext
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.concurrent.TimeUnit
import com.newterrastudios.djinn.go.player.TalkOver
import com.newterrastudios.djinn.go.i18n.t
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

private const val DAY_MS = 24L * 60 * 60 * 1000

const val KIND_DOWNLOAD = "download"
const val KIND_SMART = "smart"

/** A song kept on this phone, for one server. */
@Entity(tableName = "songs", primaryKeys = ["server", "id"])
data class OfflineSong(
    val server: String,
    val id: Long,
    val title: String,
    val artist: String?,
    val album: String?,
    @ColumnInfo(name = "artist_id") val artistId: Long?,
    @ColumnInfo(name = "album_id") val albumId: Long?,
    @ColumnInfo(name = "track_no") val trackNo: Int?,
    @ColumnInfo(name = "duration_ms") val durationMs: Long?,
    @ColumnInfo(name = "art_hash") val artHash: String?,
    /** [KIND_DOWNLOAD] you chose it, or [KIND_SMART] it was picked for you. */
    val kind: String,
    val path: String,
    val bytes: Long,
    val gain: Float,
    val version: String,
    /** Converted to MP3, or the original file. */
    val converted: Boolean,
    @ColumnInfo(name = "added_at") val addedAt: Long,
) {
    fun toTrack() = Track(id, title, artist, album, artistId, albumId, trackNo, durationMs, artHash)
}

/** A song saved as a system sound. */
@Entity(tableName = "sounds")
data class SavedSound(
    @PrimaryKey val uri: String,
    val server: String,
    @ColumnInfo(name = "track_id") val trackId: Long,
    val title: String,
    val kind: String,
)

/** A play heard while the server couldn't be reached. */
@Entity(tableName = "pending_plays")
data class PendingPlay(
    @PrimaryKey(autoGenerate = true) val rowId: Long = 0,
    val server: String,
    @ColumnInfo(name = "track_id") val trackId: Long,
    @ColumnInfo(name = "ms_played") val msPlayed: Long,
    val at: Long,
    /** Skipped with "Skip, play less". */
    @ColumnInfo(name = "play_less", defaultValue = "0") val playLess: Boolean = false,
)

/** A like changed while the server couldn't be reached. */
@Entity(tableName = "pending_likes", primaryKeys = ["server", "track_id"])
data class PendingLike(
    val server: String,
    @ColumnInfo(name = "track_id") val trackId: Long,
    val liked: Boolean,
)

/** A song added to a playlist while the server couldn't be reached. */
@Entity(tableName = "pending_adds")
data class PendingAdd(
    @PrimaryKey(autoGenerate = true) val rowId: Long = 0,
    val server: String,
    @ColumnInfo(name = "playlist_id") val playlistId: Long,
    @ColumnInfo(name = "track_id") val trackId: Long,
)

@Dao
interface OfflineDao {
    @Query("SELECT * FROM songs WHERE server = :server ORDER BY kind, artist, album, track_no, title")
    fun songsFlow(server: String): Flow<List<OfflineSong>>

    @Query("SELECT * FROM songs WHERE server = :server")
    suspend fun songs(server: String): List<OfflineSong>

    @Query("SELECT * FROM songs")
    suspend fun everySong(): List<OfflineSong>

    @Query("SELECT * FROM songs WHERE server = :server AND id = :id")
    suspend fun song(server: String, id: Long): OfflineSong?

    @Query(
        "SELECT * FROM songs WHERE server = :server AND " +
            "(title LIKE :like OR artist LIKE :like OR album LIKE :like) ORDER BY title LIMIT 100",
    )
    suspend fun search(server: String, like: String): List<OfflineSong>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putSong(song: OfflineSong)

    @Query("DELETE FROM songs WHERE server = :server AND id = :id")
    suspend fun deleteSong(server: String, id: Long)

    @Query("SELECT * FROM sounds WHERE server = :server")
    fun soundsFlow(server: String): Flow<List<SavedSound>>

    @Query("SELECT * FROM sounds")
    suspend fun everySound(): List<SavedSound>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putSound(sound: SavedSound)

    @Query("DELETE FROM sounds WHERE uri = :uri")
    suspend fun deleteSound(uri: String)

    @Insert
    suspend fun addPlay(play: PendingPlay)

    @Query("SELECT * FROM pending_plays ORDER BY at")
    suspend fun plays(): List<PendingPlay>

    @Query("DELETE FROM pending_plays WHERE rowId = :rowId")
    suspend fun deletePlay(rowId: Long)

    @Insert
    suspend fun addToPlaylist(add: PendingAdd)

    @Query("SELECT * FROM pending_adds ORDER BY rowId")
    suspend fun adds(): List<PendingAdd>

    @Query("DELETE FROM pending_adds WHERE rowId = :rowId")
    suspend fun deleteAdd(rowId: Long)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun putLike(like: PendingLike)

    @Query("SELECT * FROM pending_likes")
    suspend fun likes(): List<PendingLike>

    @Query("DELETE FROM pending_likes WHERE server = :server AND track_id = :trackId")
    suspend fun deleteLike(server: String, trackId: Long)
}

@Database(
    entities = [OfflineSong::class, SavedSound::class, PendingPlay::class, PendingLike::class, PendingAdd::class],
    version = 3,
    exportSchema = false,
)
abstract class OfflineDb : RoomDatabase() {
    abstract fun dao(): OfflineDao

    companion object {
        private val ADD_PENDING_ADDS = object : Migration(1, 2) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    "CREATE TABLE IF NOT EXISTS pending_adds (" +
                        "rowId INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, server TEXT NOT NULL, " +
                        "playlist_id INTEGER NOT NULL, track_id INTEGER NOT NULL)",
                )
            }
        }

        private val ADD_PLAY_LESS = object : Migration(2, 3) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE pending_plays ADD COLUMN play_less INTEGER NOT NULL DEFAULT 0")
            }
        }

        fun open(context: Context) =
            Room.databaseBuilder(context, OfflineDb::class.java, "offline.db")
                .addMigrations(ADD_PENDING_ADDS, ADD_PLAY_LESS)
                .build()
    }
}

/** The choices on the Settings screen, kept per device. */
/** Where Prefs keeps the language choices; `Strings.load` reads them too. */
const val SETTINGS_FILE = "settings"
const val LANGUAGE_KEY = "language"
const val HOUSE_LANGUAGE_KEY = "house_language"

class Prefs(context: Context, private val scope: () -> String = { "" }) {
    /** A setting belonging to one account on one server, not to the phone. */
    private fun own(name: String) = "$name@${scope()}"

    /** Those settings for a server other than the one in use. */
    fun lastContactFor(scope: String) = prefs.getLong("last_contact@$scope", 0)

    fun awayLockedFor(scope: String) = prefs.getBoolean("away_locked@$scope", false)

    fun keepRulesFor(scope: String): KeepRules =
        prefs.getString("keep_rules@$scope", null)
            ?.let { runCatching { lookJson.decodeFromString<KeepRules>(it) }.getOrNull() }
            ?: KeepRules()

    private val prefs = context.getSharedPreferences(SETTINGS_FILE, Context.MODE_PRIVATE)

    private val talkOverNow = MutableStateFlow(TalkOver.named(prefs.getString("talk_over", null)))

    /** What the music does when another app needs to talk. */
    val talkOverChoice: StateFlow<TalkOver> = talkOverNow

    fun chooseTalkOver(choice: TalkOver) {
        prefs.edit { putString("talk_over", choice.name) }
        talkOverNow.value = choice
    }

    /** Your own equalizer, for this phone (not per server). */
    var equalizer: EqSettings
        get() = parseEqualizer(prefs.getString("equalizer", null))
        set(value) = prefs.edit {
            putString(
                "equalizer",
                listOf(if (value.enabled) "1" else "0", value.outputGainDb.toString(), value.bandsDb.joinToString(","))
                    .joinToString(";"),
            )
        }

    /** Ask when the server has a newer version of the app. */
    var askAboutUpdates: Boolean
        get() = prefs.getBoolean("ask_about_updates", true)
        set(value) = prefs.edit { putBoolean("ask_about_updates", value) }
    /** The version answered "Not now", which isn't asked about again. */
    var updateDeclined: Long
        get() = prefs.getLong("update_declined", 0)
        set(value) = prefs.edit { putLong("update_declined", value) }

    /** Genres as a cloud rather than a list, and how much of it to show. */
    var genreCloud: Boolean
        get() = prefs.getBoolean("genre_cloud", false)
        set(value) = prefs.edit { putBoolean("genre_cloud", value) }
    var genreTop: Int
        get() = prefs.getInt("genre_top", 100)
        set(value) = prefs.edit { putInt("genre_top", value) }
    var genreLines: Int
        get() = prefs.getInt("genre_lines", 4)
        set(value) = prefs.edit { putInt("genre_lines", value) }

    /** Bit rate to convert lossless songs to; 0 keeps full quality. */
    var kbps: Int
        get() = prefs.getInt("kbps", DEFAULT_KBPS)
        set(value) = prefs.edit { putInt("kbps", value) }
    var smartCache: Boolean
        get() = prefs.getBoolean("smart_cache", true)
        set(value) = prefs.edit { putBoolean("smart_cache", value) }
    var smartLimitMb: Int
        get() = prefs.getInt("smart_limit_mb", 2048)
        set(value) = prefs.edit { putInt("smart_limit_mb", value) }
    /** Songs fetched ahead from the queue into the recent cache; 0 is off. */
    var cacheAhead: Int
        get() = prefs.getInt("cache_ahead", DEFAULT_CACHE_AHEAD)
        set(value) = prefs.edit { putInt("cache_ahead", value) }
    var wifiOnly: Boolean
        get() = prefs.getBoolean("wifi_only", true)
        set(value) = prefs.edit { putBoolean("wifi_only", value) }
    var lastContact: Long
        get() = prefs.getLong(own("last_contact"), 0)
        set(value) = prefs.edit { putLong(own("last_contact"), value) }
    var likedIds: Set<Long>
        get() = prefs.getStringSet(own("liked"), emptySet()).orEmpty().mapNotNull { it.toLongOrNull() }.toSet()
        set(value) = prefs.edit { putStringSet(own("liked"), value.map { it.toString() }.toSet()) }
    /** The playlists as last seen, for adding to while offline. */
    var playlists: String?
        get() = prefs.getString(own("playlists"), null)
        set(value) = prefs.edit { putString(own("playlists"), value) }
    var visuals: Int
        get() = prefs.getInt("visuals", 0)
        set(value) = prefs.edit { putInt("visuals", value) }
    /** Full screen moves through the styles by itself. */
    var visualsAuto: Boolean
        get() = prefs.getBoolean("visuals_auto", false)
        set(value) = prefs.edit { putBoolean("visuals_auto", value) }
    /** The language picked on this phone (Settings); empty to follow the server's default. */
    var language: String
        get() = prefs.getString(LANGUAGE_KEY, "").orEmpty()
        set(value) = prefs.edit { putString(LANGUAGE_KEY, value) }
    /** The server's default language as last heard; empty for the phone's own. */
    var houseLanguage: String
        get() = prefs.getString(HOUSE_LANGUAGE_KEY, "").orEmpty()
        set(value) = prefs.edit { putString(HOUSE_LANGUAGE_KEY, value) }
    /** The admin's rules for kept songs, as last heard. */
    var keepRules: KeepRules
        get() = prefs.getString(own("keep_rules"), null)
            ?.let { runCatching { lookJson.decodeFromString<KeepRules>(it) }.getOrNull() }
            ?: KeepRules()
        set(value) = prefs.edit { putString(own("keep_rules"), lookJson.encodeToString(value)) }
    /** Too long away from the server: kept songs wait until it's reached again. */
    var awayLocked: Boolean
        get() = prefs.getBoolean(own("away_locked"), false)
        set(value) = prefs.edit { putBoolean(own("away_locked"), value) }
    /** Now Playing shows visuals rather than the cover. */
    var visualsOn: Boolean
        get() = prefs.getBoolean("visuals_on", false)
        set(value) = prefs.edit { putBoolean("visuals_on", value) }
    /** Lyrics open under the player's controls. */
    var lyricsOn: Boolean
        get() = prefs.getBoolean("lyrics_on", false)
        set(value) = prefs.edit { putBoolean("lyrics_on", value) }
    /** Styles this device turned out not to keep up with. */
    var visualsTooSlow: Set<String>
        get() = prefs.getStringSet("visuals_too_slow", emptySet()).orEmpty()
        set(value) = prefs.edit { putStringSet("visuals_too_slow", value) }
    var visualLook: VisualLook
        get() = prefs.getString("visual_look", null)
            ?.let { runCatching { lookJson.decodeFromString<VisualLook>(it) }.getOrNull() }
            ?: VisualLook()
        set(value) = prefs.edit { putString("visual_look", lookJson.encodeToString(value)) }
    /** Lines the visuals up with the speaker, in milliseconds. */
    var visualsLeadMs: Int
        get() = prefs.getInt("visuals_lead_ms", DEFAULT_VISUALS_LEAD_MS)
        set(value) = prefs.edit { putInt("visuals_lead_ms", value) }
    /** How strongly they react, 1 being as the sound is. */
    var visualsIntensity: Float
        get() = prefs.getFloat("visuals_intensity", 1f)
        set(value) = prefs.edit { putFloat("visuals_intensity", value) }

    companion object {
        private val lookJson = kotlinx.serialization.json.Json { ignoreUnknownKeys = true }
        const val DEFAULT_KBPS = 128
        const val DEFAULT_CACHE_AHEAD = 3
        const val DEFAULT_VISUALS_LEAD_MS = 250
    }
}

/** At or over this, a download asks first. */
const val LARGE_DOWNLOAD_BYTES = 500L * 1000 * 1000
/** Lower when the songs will stay at full size though smaller was asked for. */
const val LARGE_UNCONVERTED_BYTES = 100L * 1000 * 1000

/**
 * What to say before a large download, or null to just start it. Only lossless
 * songs convert, so with converting on this is the most it could take.
 */
fun largeDownloadWarning(bytes: Long, kbps: Int, canConvert: Boolean): String? {
    val size = if (bytes >= 1_000_000_000) "%.1f GB".format(bytes / 1e9) else "${bytes / 1_000_000} MB"
    return when {
        kbps > 0 && !canConvert && bytes >= LARGE_UNCONVERTED_BYTES ->
            t("download.not_converting", "size" to size)
        kbps > 0 && canConvert && bytes >= LARGE_DOWNLOAD_BYTES ->
            t("download.up_to", "size" to size)
        kbps == 0 && bytes >= LARGE_DOWNLOAD_BYTES -> t("download.full_quality", "size" to size)
        else -> null
    }
}

/** A song kept for one account on one server, and which server to credit it to. */
data class KeptEntry(val song: OfflineSong, val serverName: String?)

private const val KEPT_PREFIX = "kept|"

/**
 * How a song from another server is known to the player. A song's number only
 * means something on its own server, so it can't go by number alone.
 */
fun keptMediaId(scope: String, id: Long) = "$KEPT_PREFIX$scope|$id"

/** The account and song number in a `keptMediaId`, or null for an ordinary song. */
fun parseKeptMediaId(mediaId: String): Pair<String, Long>? {
    if (!mediaId.startsWith(KEPT_PREFIX)) return null
    val rest = mediaId.removePrefix(KEPT_PREFIX)
    val split = rest.lastIndexOf('|')
    if (split <= 0) return null
    val id = rest.substring(split + 1).toLongOrNull() ?: return null
    return rest.substring(0, split) to id
}

/**
 * Whether songs from a server other than the one in use may play. By the rules
 * last heard from it, and only with a sign-in: a server this phone no longer
 * knows, or has signed out of, stays locked.
 */
fun lockedElsewhere(signedIn: Boolean, awayLocked: Boolean, away: WhenAway): Boolean =
    !signedIn || awayLocked || away != WhenAway.Nothing

enum class WhenAway { Nothing, Lock, Delete }

/** What the admin's rules say to do with kept songs, given the last word from the server. */
fun whenAway(lastContact: Long, now: Long, rules: KeepRules): WhenAway {
    // Never having reached the server isn't being away.
    if (lastContact == 0L || now - lastContact <= rules.awayDays * DAY_MS) return WhenAway.Nothing
    return when (rules.away) {
        "off" -> WhenAway.Nothing
        "delete" -> WhenAway.Delete
        else -> WhenAway.Lock
    }
}

/** Keeps songs on this phone, and tells the server what happened while it couldn't. */
class OfflineStore(
    private val context: Context,
    private val account: Account,
    private val api: GoApi,
    private val prefs: Prefs,
) {
    val dao = OfflineDb.open(context).dao()

    /** Conversion happens while the server answers, so allow it time. */
    private val slow = api.http.newBuilder().readTimeout(10, TimeUnit.MINUTES).build()

    private fun folder(server: String): File {
        val hash = MessageDigest.getInstance("MD5").digest(server.toByteArray())
            .joinToString("") { "%02x".format(it) }
        return File(context.filesDir, "offline/$hash").apply { mkdirs() }
    }

    /** Kept songs don't play for someone signed out, or too long away from the server. */
    fun isLocked(): Boolean = account.token == null || prefs.awayLocked

    /** Whether songs kept for this account on this server may play right now. */
    fun isLockedFor(scope: String): Boolean {
        if (scope == account.scope) return isLocked()
        val saved = account.servers.value.firstOrNull { scopeOf(it) == scope }
        return lockedElsewhere(
            signedIn = saved?.token != null,
            awayLocked = prefs.awayLockedFor(scope),
            away = whenAway(prefs.lastContactFor(scope), System.currentTimeMillis(), prefs.keepRulesFor(scope)),
        )
    }

    /** Everything on the phone that may play now, from any server, and how many may not. */
    suspend fun allKept(): Pair<List<KeptEntry>, Int> = withContext(Dispatchers.IO) {
        val (open, locked) = dao.everySong().filter { File(it.path).exists() }.partition { !isLockedFor(it.server) }
        val names = account.servers.value.associate { scopeOf(it) to it.name }
        open.map { KeptEntry(it, names[it.server]) }.sortedBy { it.song.title.lowercase() } to locked.size
    }

    /** A song from any server, if it may play now and its file is really there. */
    suspend fun keptElsewhere(scope: String, id: Long): OfflineSong? {
        if (isLockedFor(scope)) return null
        return dao.song(scope, id)?.takeIf { File(it.path).exists() }
    }

    /** The songs that can be played from this phone right now. */
    suspend fun playable(server: String): List<OfflineSong> =
        if (isLocked()) emptyList() else dao.songs(server)

    suspend fun searchPlayable(server: String, query: String): List<OfflineSong> =
        if (isLocked()) emptyList() else dao.search(server, "%$query%")

    /** The kept copy, if there is one, it may play and its file is really there. */
    suspend fun local(id: Long): OfflineSong? {
        if (isLocked()) return null
        val server = account.scope ?: return null
        return dao.song(server, id)?.takeIf { File(it.path).exists() }
    }

    /**
     * Fetches a copy of a song. A download replaces a smart copy, and stays a
     * download if smart cache asks for it again. Blocks until it's on the phone.
     */
    suspend fun fetch(track: Track, kind: String): OfflineSong = withContext(Dispatchers.IO) {
        val server = account.scope ?: throw ApiException(0, "No server chosen")
        val address = account.server ?: throw ApiException(0, "No server chosen")
        val existing = dao.song(server, track.id)?.takeIf { File(it.path).exists() }
        val link = api.offlineLink(track.id, prefs.kbps)
        val effectiveKind = if (existing?.kind == KIND_DOWNLOAD) KIND_DOWNLOAD else kind
        if (existing != null && existing.version == link.version && existing.converted == link.converted) {
            return@withContext existing.copy(kind = effectiveKind).also { dao.putSong(it) }
        }
        val extension = MimeTypeMap.getSingleton().getExtensionFromMimeType(link.mime.substringBefore(';')) ?: "audio"
        val file = File(folder(server), "${track.id}.$extension")
        val part = File(file.path + ".part")
        val request = Request.Builder().url(address + link.url).build()
        slow.newCall(request).execute().use { response ->
            if (!response.isSuccessful) throw ApiException(response.code, "The server answered ${response.code}")
            part.outputStream().use { out -> response.body.byteStream().copyTo(out) }
        }
        if (existing != null && existing.path != file.path) File(existing.path).delete()
        if (!part.renameTo(file)) throw IOException("Couldn't save the song")
        val song = OfflineSong(
            server = server,
            id = track.id,
            title = track.title,
            artist = track.artist,
            album = track.album,
            artistId = track.artistId,
            albumId = track.albumId,
            trackNo = track.trackNo,
            durationMs = track.durationMs,
            artHash = track.artHash,
            kind = effectiveKind,
            path = file.path,
            bytes = file.length(),
            gain = link.gain,
            version = link.version,
            converted = link.converted,
            addedAt = System.currentTimeMillis(),
        )
        dao.putSong(song)
        song
    }

    suspend fun remove(id: Long) {
        val server = account.scope ?: return
        dao.song(server, id)?.let { File(it.path).delete() }
        dao.deleteSong(server, id)
    }

    /** Deletes what's kept for the account in use. Other servers and accounts aren't touched. */
    suspend fun deleteEverything() {
        val scope = account.scope ?: return
        deleteWhere { it == scope }
    }

    /** Deletes what's kept for every account on a server, when it's forgotten. */
    suspend fun deleteForServer(server: SavedServer) {
        val scope = serverScope(server)
        deleteWhere { it.startsWith(scope) }
    }

    private suspend fun deleteWhere(belongs: (String) -> Boolean) = withContext(Dispatchers.IO) {
        dao.everySong().filter { belongs(it.server) }.forEach {
            File(it.path).delete()
            dao.deleteSong(it.server, it.id)
        }
        dao.everySound().filter { belongs(it.server) }.forEach { removeSound(it) }
    }

    /** Clears out what's kept for other servers and accounts, when someone asks. */
    suspend fun deleteElsewhere() {
        val scope = account.scope ?: return
        deleteWhere { it != scope }
    }

    /** What's kept for other servers and accounts. Being elsewhere leaves it alone. */
    suspend fun keptElsewhere(): List<OfflineSong> = withContext(Dispatchers.IO) {
        val scope = account.scope
        dao.everySong().filter { it.server != scope }
    }

    /**
     * At each contact with the server in use: may its songs stay, and what are
     * its rules now? Without contact: has it been too long, by the rules last
     * heard? Being on another server is normal, so other servers' songs wait
     * until they're next used, and time away is counted for each server.
     */
    suspend fun checkAccess() = withContext(Dispatchers.IO) {
        if (account.scope == null) return@withContext
        val now = System.currentTimeMillis()
        try {
            val me = api.me()
            prefs.lastContact = now
            prefs.awayLocked = false
            if (!me.permissions.downloads) deleteEverything()
            try {
                prefs.keepRules = api.keepRules()
            } catch (_: IOException) {
                // An older server, or a role GO refuses: the rules stay as they were.
            }
            try {
                val house = api.language().default
                if (house != prefs.houseLanguage) {
                    prefs.houseLanguage = house
                    // Screens already shown change when they're next opened.
                    com.newterrastudios.djinn.go.i18n.Strings.load(context)
                }
            } catch (_: IOException) {
                // An older server: the language stays as it was.
            }
        } catch (_: IOException) {
            when (whenAway(prefs.lastContact, now, prefs.keepRules)) {
                WhenAway.Nothing -> prefs.awayLocked = false
                WhenAway.Lock -> prefs.awayLocked = true
                WhenAway.Delete -> deleteEverything()
            }
        }
    }

    /** Sends what happened offline. What the server refuses isn't kept. */
    suspend fun sync() = withContext(Dispatchers.IO) {
        val server = account.scope ?: return@withContext
        for (play in dao.plays().filter { it.server == server }) {
            try {
                api.reportPlay(play.trackId, play.msPlayed, play.at, play.playLess)
            } catch (e: ApiException) {
                if (e.status == 401) return@withContext
            }
            dao.deletePlay(play.rowId)
        }
        for (add in dao.adds().filter { it.server == server }) {
            try {
                api.addToPlaylist(add.playlistId, add.trackId)
            } catch (e: ApiException) {
                if (e.status == 401) return@withContext
            }
            dao.deleteAdd(add.rowId)
        }
        for (like in dao.likes().filter { it.server == server }) {
            try {
                api.setLike(like.trackId, like.liked)
            } catch (e: ApiException) {
                if (e.status == 401) return@withContext
            }
            dao.deleteLike(like.server, like.trackId)
        }
    }

    /**
     * Songs whose file changed on the server, to fetch again. A song missing
     * from the answer is left alone: it may only be a library drive that's
     * unplugged, and the copy still plays.
     */
    suspend fun changed(): List<Track> = withContext(Dispatchers.IO) {
        val server = account.scope ?: return@withContext emptyList()
        val kept = dao.songs(server)
        if (kept.isEmpty()) return@withContext emptyList()
        val current = kept.map { it.id }.chunked(500).flatMap { api.versions(it) }.associateBy { it.id }
        kept.filter { song -> current[song.id]?.let { it.version != song.version } == true }.map { it.toTrack() }
    }

    /**
     * Saves a song as a system sound: a real file other apps can use, kept in
     * the phone's Alarms, Ringtones or Notifications folder.
     */
    suspend fun saveSound(song: OfflineSong, kind: SoundKind): SavedSound = withContext(Dispatchers.IO) {
        val source = File(song.path)
        val extension = source.extension
        val name = "${song.title} - ${song.id}.$extension".replace(Regex("""[\\/:*?"<>|]"""), "_")
        val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension) ?: "audio/mpeg"
        val values = ContentValues().apply {
            put(MediaStore.Audio.Media.DISPLAY_NAME, name)
            put(MediaStore.Audio.Media.TITLE, song.title)
            put(MediaStore.Audio.Media.ARTIST, song.artist)
            put(MediaStore.Audio.Media.MIME_TYPE, mime)
            put(MediaStore.Audio.Media.IS_ALARM, kind == SoundKind.Alarm)
            put(MediaStore.Audio.Media.IS_RINGTONE, kind == SoundKind.Ringtone)
            put(MediaStore.Audio.Media.IS_NOTIFICATION, kind == SoundKind.Notification)
            put(MediaStore.Audio.Media.IS_MUSIC, false)
        }
        val resolver = context.contentResolver
        val uri: Uri
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            values.put(MediaStore.Audio.Media.RELATIVE_PATH, kind.folder + "/")
            values.put(MediaStore.Audio.Media.IS_PENDING, 1)
            uri = resolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values)
                ?: throw IOException("The phone wouldn't make the file")
            resolver.openOutputStream(uri)?.use { out -> source.inputStream().use { it.copyTo(out) } }
                ?: throw IOException("The phone wouldn't open the file")
            resolver.update(uri, ContentValues().apply { put(MediaStore.Audio.Media.IS_PENDING, 0) }, null, null)
        } else {
            @Suppress("DEPRECATION")
            val folder = Environment.getExternalStoragePublicDirectory(kind.folder).apply { mkdirs() }
            val target = File(folder, name)
            source.copyTo(target, overwrite = true)
            @Suppress("DEPRECATION")
            values.put(MediaStore.Audio.Media.DATA, target.path)
            uri = resolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values)
                ?: throw IOException("The phone wouldn't list the file")
        }
        SavedSound(uri.toString(), song.server, song.id, song.title, kind.name).also { dao.putSound(it) }
    }

    suspend fun removeSound(sound: SavedSound) = withContext(Dispatchers.IO) {
        runCatching { context.contentResolver.delete(sound.uri.toUri(), null, null) }
        dao.deleteSound(sound.uri)
    }
}

enum class SoundKind(val label: String, val folder: String) {
    Alarm("alarm", Environment.DIRECTORY_ALARMS),
    Ringtone("ringtone", Environment.DIRECTORY_RINGTONES),
    Notification("notification", Environment.DIRECTORY_NOTIFICATIONS),
}

/** "1;-2.0;4.0,5.0,..." (on, output level, bands) back to settings; flat if unreadable. */
internal fun parseEqualizer(saved: String?): EqSettings {
    val parts = saved?.split(";") ?: return EqSettings()
    val bands = parts.getOrNull(2)?.split(",")?.mapNotNull { it.toFloatOrNull() }
    val output = parts.getOrNull(1)?.toFloatOrNull()
    if (parts.size != 3 || output == null || bands == null || bands.size != EQ_BAND_HZ.size) return EqSettings()
    return EqSettings(
        enabled = parts[0] == "1",
        bandsDb = bands.map { it.coerceIn(-EQ_MAX_BAND_DB, EQ_MAX_BAND_DB) },
        outputGainDb = output.coerceIn(EQ_MIN_OUTPUT_DB, EQ_MAX_OUTPUT_DB),
    )
}
