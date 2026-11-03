package com.newterrastudios.djinn.go.data

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

/** Without reaching the server this long, songs kept here are deleted. */
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
    version = 2,
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

        fun open(context: Context) =
            Room.databaseBuilder(context, OfflineDb::class.java, "offline.db")
                .addMigrations(ADD_PENDING_ADDS)
                .build()
    }
}

/** The choices on the Settings screen, kept per device. */
class Prefs(context: Context) {
    private val prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

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
    var wifiOnly: Boolean
        get() = prefs.getBoolean("wifi_only", true)
        set(value) = prefs.edit { putBoolean("wifi_only", value) }
    var lastContact: Long
        get() = prefs.getLong("last_contact", 0)
        set(value) = prefs.edit { putLong("last_contact", value) }
    var likedIds: Set<Long>
        get() = prefs.getStringSet("liked", emptySet()).orEmpty().mapNotNull { it.toLongOrNull() }.toSet()
        set(value) = prefs.edit { putStringSet("liked", value.map { it.toString() }.toSet()) }
    /** The playlists as last seen, for adding to while offline. */
    var playlists: String?
        get() = prefs.getString("playlists", null)
        set(value) = prefs.edit { putString("playlists", value) }
    var visuals: Int
        get() = prefs.getInt("visuals", 0)
        set(value) = prefs.edit { putInt("visuals", value) }
    /** Full screen moves through the styles by itself. */
    var visualsAuto: Boolean
        get() = prefs.getBoolean("visuals_auto", false)
        set(value) = prefs.edit { putBoolean("visuals_auto", value) }
    /** The admin's rules for kept songs, as last heard. */
    var keepRules: KeepRules
        get() = prefs.getString("keep_rules", null)
            ?.let { runCatching { lookJson.decodeFromString<KeepRules>(it) }.getOrNull() }
            ?: KeepRules()
        set(value) = prefs.edit { putString("keep_rules", lookJson.encodeToString(value)) }
    /** Too long away from the server: kept songs wait until it's reached again. */
    var awayLocked: Boolean
        get() = prefs.getBoolean("away_locked", false)
        set(value) = prefs.edit { putBoolean("away_locked", value) }
    /** Whose songs these are: the server and the account. */
    var keptFor: String?
        get() = prefs.getString("kept_for", null)
        set(value) = prefs.edit { putString("kept_for", value) }
    /** Now Playing shows visuals rather than the cover. */
    var visualsOn: Boolean
        get() = prefs.getBoolean("visuals_on", false)
        set(value) = prefs.edit { putBoolean("visuals_on", value) }
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
            "Your server isn't converting songs for this account, so these stay at full size: $size."
        kbps > 0 && canConvert && bytes >= LARGE_DOWNLOAD_BYTES ->
            "Up to $size. FLAC and WAV songs will take less once converted, but converting takes the server a while."
        kbps == 0 && bytes >= LARGE_DOWNLOAD_BYTES -> "Full quality: $size."
        else -> null
    }
}

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

/** Whether kept songs belong to someone else: a different server or account. */
fun keptForSomeoneElse(keptFor: String?, now: String): Boolean = keptFor != null && keptFor != now

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

    /** The songs that can be played from this phone right now. */
    suspend fun playable(server: String): List<OfflineSong> =
        if (isLocked()) emptyList() else dao.songs(server)

    suspend fun searchPlayable(server: String, query: String): List<OfflineSong> =
        if (isLocked()) emptyList() else dao.search(server, "%$query%")

    /** The kept copy, if there is one, it may play and its file is really there. */
    suspend fun local(id: Long): OfflineSong? {
        if (isLocked()) return null
        val server = account.server ?: return null
        return dao.song(server, id)?.takeIf { File(it.path).exists() }
    }

    /**
     * Fetches a copy of a song. A download replaces a smart copy, and stays a
     * download if smart cache asks for it again. Blocks until it's on the phone.
     */
    suspend fun fetch(track: Track, kind: String): OfflineSong = withContext(Dispatchers.IO) {
        val server = account.server ?: throw ApiException(0, "No server chosen")
        val existing = dao.song(server, track.id)?.takeIf { File(it.path).exists() }
        val link = api.offlineLink(track.id, prefs.kbps)
        val effectiveKind = if (existing?.kind == KIND_DOWNLOAD) KIND_DOWNLOAD else kind
        if (existing != null && existing.version == link.version && existing.converted == link.converted) {
            return@withContext existing.copy(kind = effectiveKind).also { dao.putSong(it) }
        }
        val extension = MimeTypeMap.getSingleton().getExtensionFromMimeType(link.mime.substringBefore(';')) ?: "audio"
        val file = File(folder(server), "${track.id}.$extension")
        val part = File(file.path + ".part")
        val request = Request.Builder().url(server + link.url).build()
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
        val server = account.server ?: return
        dao.song(server, id)?.let { File(it.path).delete() }
        dao.deleteSong(server, id)
    }

    /** Everything kept, gone: access ended, or nothing heard for too long. */
    suspend fun deleteEverything() = withContext(Dispatchers.IO) {
        dao.everySong().forEach {
            File(it.path).delete()
            dao.deleteSong(it.server, it.id)
        }
        dao.everySound().forEach { removeSound(it) }
    }

    /**
     * At each contact: whose songs are these, may they stay, and what are the
     * rules now? Without contact: has it been too long, by the rules last heard?
     */
    suspend fun checkAccess() = withContext(Dispatchers.IO) {
        val now = System.currentTimeMillis()
        try {
            val me = api.me()
            prefs.lastContact = now
            prefs.awayLocked = false
            val owner = "${account.server}|${me.username}"
            if (keptForSomeoneElse(prefs.keptFor, owner)) deleteEverything()
            prefs.keptFor = owner
            if (!me.permissions.downloads) deleteEverything()
            try {
                prefs.keepRules = api.keepRules()
            } catch (_: IOException) {
                // An older server, or a role GO refuses: the rules stay as they were.
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
        val server = account.server ?: return@withContext
        for (play in dao.plays().filter { it.server == server }) {
            try {
                api.reportPlay(play.trackId, play.msPlayed, play.at)
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
        val server = account.server ?: return@withContext emptyList()
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
