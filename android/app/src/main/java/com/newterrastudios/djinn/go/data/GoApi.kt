package com.newterrastudios.djinn.go.data

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import java.net.URLEncoder
import java.util.concurrent.TimeUnit

/** The server said no, with something to show the person. */
class ApiException(val status: Int, message: String) : IOException(message)

@Serializable
private data class ErrorBody(val error: String? = null)

/** A known server isn't presenting its certificate. Nothing was sent to it. */
class CertificateChanged : IOException(
    "This server's certificate has changed, so nothing was sent to it. Settings > Other servers can check it.",
)

/** What a first look at a server found. */
data class Probed(
    /** Where to reach it: encrypted when it can be. */
    val address: String,
    /** The certificate it presented, unless a real authority vouches for it. */
    val pin: String?,
    val hello: ServerHello?,
    val encrypted: Boolean,
)

/** A genre's address. In a path a "+" stays a plus, so spaces have to be %20. */
internal fun genrePath(name: String) =
    "/api/genres/" + URLEncoder.encode(name, "UTF-8").replace("+", "%20")

/** DJINN GO's API on the server this phone is connected to. */
class GoApi(private val account: Account) {
    val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }
    private val jsonType = "application/json".toMediaType()

    private val pins = { account.servers.value.mapNotNull { it.pin }.toSet() }
    private val trust = PinnedTrust(pins)

    /** Shared with the player and images, so they carry the sign-in too. */
    val http: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .sslSocketFactory(
            SSLContext.getInstance("TLS").apply { init(null, arrayOf(trust), null) }.socketFactory,
            trust,
        )
        .hostnameVerifier(PinnedHostnames(pins))
        // After the connection opens and before anything is sent, sign-ins included.
        .addNetworkInterceptor(Interceptor { chain ->
            val url = chain.request().url
            val expected = serverAt(account.servers.value, url.host, url.port)?.pin
            if (expected != null) {
                // Not handshake().peerCertificates, which is empty for a certificate no authority signed.
                val socket = chain.connection()?.socket() as? SSLSocket
                val presented = socket?.session?.peerCertificates?.firstOrNull()
                if (presented == null || fingerprint(presented) != expected) {
                    throw CertificateChanged()
                }
            }
            chain.proceed(chain.request())
        })
        .addInterceptor(Interceptor { chain ->
            val request = chain.request()
            val token = account.token
            // The token only ever goes to the server it came from.
            val ours = account.server?.toHttpUrlOrNull()?.let {
                it.host == request.url.host && it.port == request.url.port
            } == true
            val named = request.newBuilder().header("User-Agent", "DJINN GO for Android")
            if (token != null && ours) named.header("Authorization", "Bearer $token")
            chain.proceed(named.build())
        })
        .build()

    /** A song's picture, or null for no picture. */
    fun artUrl(hash: String?, size: Int? = 128): String? {
        val server = account.server ?: return null
        hash ?: return null
        return "$server/api/art/$hash" + (size?.let { "?size=$it" } ?: "")
    }

    /** Works on any thread that may block. */
    @PublishedApi
    internal fun send(method: String, path: String, body: String? = null): String {
        val server = account.server ?: throw ApiException(0, "No server chosen")
        val request = Request.Builder().url(server + path)
            .method(method, body?.toRequestBody(jsonType) ?: if (method == "GET") null else ByteArray(0).toRequestBody())
            .build()
        http.newCall(request).execute().use { response ->
            val text = response.body.string()
            if (response.isSuccessful) return text
            // A dead token ends the sign-in; the password check answers 401 too.
            if (response.code == 401 && account.token != null) account.signOut()
            val said = runCatching { json.decodeFromString<ErrorBody>(text).error }.getOrNull()
            throw ApiException(response.code, said ?: "The server answered ${response.code}")
        }
    }

    private suspend inline fun <reified T> get(path: String): T = withContext(Dispatchers.IO) {
        json.decodeFromString<T>(send("GET", path))
    }

    private suspend inline fun <reified B> write(method: String, path: String, body: B) {
        withContext(Dispatchers.IO) { send(method, path, json.encodeToString(body)) }
    }

    suspend fun check() = withContext(Dispatchers.IO) { send("GET", "/api/auth/mode"); Unit }

    /**
     * Looks at a server before trusting it, sending nothing private. Tries, in
     * order: HTTPS with a certificate an authority vouches for, HTTPS with the
     * server's own certificate, and plain HTTP. A failed connection throws.
     */
    suspend fun probe(typed: String): Probed = withContext(Dispatchers.IO) {
        fun hello(text: String) = runCatching { json.decodeFromString<ServerHello>(text) }.getOrNull()
        fun get(client: OkHttpClient, base: String) =
            client.newCall(Request.Builder().url("$base/api/go/hello").build()).execute()

        val quick = OkHttpClient.Builder().connectTimeout(6, TimeUnit.SECONDS).readTimeout(6, TimeUnit.SECONDS).build()
        if (typed.startsWith("https://")) {
            try {
                get(quick, typed).use { return@withContext Probed(typed, null, hello(it.body.string()), true) }
            } catch (_: IOException) {
                // No authority vouches for it; it may have a certificate of its own.
            }
        }
        val secure = withScheme(typed, "https") ?: throw IOException("Not an address")
        try {
            val reader = CertificateReader()
            val reading = quick.newBuilder()
                .sslSocketFactory(
                    SSLContext.getInstance("TLS").apply { init(null, arrayOf(reader), null) }.socketFactory,
                    reader,
                )
                .hostnameVerifier { _, _ -> true }
                .build()
            get(reading, secure).use { response ->
                val presented = reader.seen ?: throw IOException("No certificate")
                return@withContext Probed(secure, fingerprint(presented), hello(response.body.string()), true)
            }
        } catch (e: IOException) {
            Log.i("DjinnGo", "No encrypted connection to $secure", e)
            if (typed.startsWith("https://")) throw e
        }
        val plain = withScheme(typed, "http") ?: throw IOException("Not an address")
        get(quick, plain).use { Probed(plain, null, hello(it.body.string()), false) }
    }

    suspend fun login(username: String, password: String): SignInReply = withContext(Dispatchers.IO) {
        json.decodeFromString(send("POST", "/api/auth/login", json.encodeToString(Login(username, password))))
    }

    suspend fun twoStep(ticket: String, code: String): SignInReply = withContext(Dispatchers.IO) {
        json.decodeFromString(send("POST", "/api/auth/two-step", json.encodeToString(TwoStep(ticket, code))))
    }

    suspend fun logout() = withContext(Dispatchers.IO) { send("POST", "/api/auth/logout"); Unit }

    suspend fun home(): GoHome = get("/api/go/home")

    suspend fun keepRules(): KeepRules = get("/api/go/keep-rules")

    /** The app this server carries, if it does and says so. */
    suspend fun appRelease(): AppRelease? = try {
        get<AppRelease?>("/api/go/app")
    } catch (_: IOException) {
        null
    } catch (_: kotlinx.serialization.SerializationException) {
        null
    }

    /** Who the server says it is. Older servers don't say. */
    suspend fun hello(): ServerHello? = try {
        get("/api/go/hello")
    } catch (_: IOException) {
        null
    }
    suspend fun artists(sort: String, offset: Int): Page<ArtistSummary> =
        get("/api/artists?sort=$sort&offset=$offset")
    suspend fun albums(sort: String, filter: String, offset: Int): Page<Album> =
        get("/api/albums?sort=$sort&filter=$filter&offset=$offset")
    suspend fun songs(sort: String, offset: Int): Page<Track> = get("/api/tracks?sort=$sort&offset=$offset")
    suspend fun artistJumps(sort: String): List<Jump> = get("/api/artists/jumps?sort=$sort")
    suspend fun albumJumps(sort: String, filter: String): List<Jump> =
        get("/api/albums/jumps?sort=$sort&filter=$filter")
    suspend fun songJumps(sort: String): List<Jump> = get("/api/tracks/jumps?sort=$sort")
    suspend fun genres(top: Int, lines: Int): GenreList = get("/api/genres?top=$top&lines=$lines")
    suspend fun genre(name: String): GenrePage = get(genrePath(name))
    suspend fun genreSongs(name: String, offset: Int): Page<Track> = get(genrePath(name) + "/songs?offset=$offset")
    suspend fun genreArtists(name: String, offset: Int): Page<ArtistSummary> =
        get(genrePath(name) + "/artists?offset=$offset")
    suspend fun album(id: Long): AlbumDetail = get("/api/albums/$id")
    suspend fun artist(id: Long): ArtistDetail = get("/api/artists/$id")
    suspend fun playlist(id: Long): GoPlaylistDetail = get("/api/go/playlists/$id")
    /** Songs like this one, leaving out any already lined up. */
    suspend fun radio(id: Long, exclude: Collection<Long> = emptyList()): List<Track> =
        get("/api/go/radio/$id" + if (exclude.isEmpty()) "" else "?exclude=" + exclude.joinToString(","))
    suspend fun resume(): GoResume? = home().resume
    suspend fun search(query: String): SearchResults =
        get("/api/search?q=${URLEncoder.encode(query, "UTF-8")}")

    /** The search read by the server's AI reader, which takes a few seconds. Null if it can't. */
    suspend fun ask(query: String): Described? =
        get("/api/search/ask?q=${URLEncoder.encode(query, "UTF-8")}")

    /** A song's lyrics, null when there are none. */
    suspend fun lyrics(id: Long): Lyrics? = get("/api/tracks/$id/lyrics")

    suspend fun likedIds(): LikedIds = get("/api/go/likes/ids")
    suspend fun liked(): Liked = get("/api/go/likes")
    suspend fun setAlbumLike(id: Long, on: Boolean) = withContext(Dispatchers.IO) {
        send(if (on) "PUT" else "DELETE", "/api/go/likes/album/$id")
        Unit
    }
    suspend fun setArtistLike(id: Long, on: Boolean) = withContext(Dispatchers.IO) {
        send(if (on) "PUT" else "DELETE", "/api/go/likes/artist/$id")
        Unit
    }
    suspend fun setLike(id: Long, on: Boolean) = withContext(Dispatchers.IO) {
        send(if (on) "PUT" else "DELETE", "/api/go/likes/track/$id")
        Unit
    }

    /** A fresh link to a song's file. Blocks: the player asks from its own thread. */
    fun playLinksBlocking(id: Long): PlayLinks = json.decodeFromString(send("GET", "/api/go/play/$id"))

    suspend fun playLinks(id: Long): PlayLinks = withContext(Dispatchers.IO) { playLinksBlocking(id) }

    suspend fun reportPlay(id: Long, msPlayed: Long, at: Long? = null) =
        write("POST", "/api/go/plays", PlayReport(id, msPlayed, at))

    suspend fun me(): Me = get("/api/me")
    suspend fun track(id: Long): Track = get("/api/tracks/$id")
    suspend fun suggested(): List<Track> = get("/api/go/offline-suggested")

    /** Average bit rate for lossless songs, or the original with 0. */
    suspend fun offlineLink(id: Long, kbps: Int): OfflineLink =
        get("/api/go/offline/$id" + if (kbps > 0) "?kbps=$kbps&vbr=true" else "")

    suspend fun prefs(): GoPrefs = get("/api/go/prefs")
    suspend fun setPrefs(prefs: GoPrefs) = write("PUT", "/api/go/prefs", prefs)

    suspend fun versions(ids: List<Long>): List<SongVersion> = withContext(Dispatchers.IO) {
        json.decodeFromString(send("POST", "/api/go/offline/versions", json.encodeToString(VersionQuery(ids))))
    }

    suspend fun createPlaylist(name: String): GoPlaylistDetail = withContext(Dispatchers.IO) {
        json.decodeFromString(send("POST", "/api/go/playlists", json.encodeToString(NewPlaylist(name))))
    }

    suspend fun renamePlaylist(id: Long, name: String) = write("PATCH", "/api/go/playlists/$id", PlaylistName(name))

    suspend fun deletePlaylist(id: Long) = withContext(Dispatchers.IO) {
        send("DELETE", "/api/go/playlists/$id")
        Unit
    }

    /** Replaces the songs, for taking one out. */
    suspend fun setPlaylistSongs(id: Long, songs: List<Long>) =
        write("PUT", "/api/go/playlists/$id/songs", SongIds(songs))

    suspend fun playlists(): List<GoPlaylist> = get("/api/go/playlists")

    /** Makes playlists from a file or pasted songs. */
    suspend fun youtubeLinks(): YoutubeLinks = get("/api/playlists/youtube-key")

    suspend fun importPlaylist(text: String, filename: String?): ImportResult = withContext(Dispatchers.IO) {
        json.decodeFromString(send("POST", "/api/playlists/import", json.encodeToString(ImportRequest(text, filename))))
    }

    /** Takes one line out of a playlist, whether or not it can play. */
    suspend fun removePlaylistEntry(playlist: Long, entry: Long) = withContext(Dispatchers.IO) {
        send("DELETE", "/api/playlists/$playlist/entries/$entry")
        Unit
    }

    suspend fun setPlaylistShared(id: Long, shared: Boolean) =
        write("PATCH", "/api/go/playlists/$id", SharedSetting(shared))

    suspend fun shareTargets(): List<ShareTarget> = get("/api/playlists/share-targets")
    suspend fun playlistShares(id: Long): List<ShareTarget> = get("/api/playlists/$id/shares")

    suspend fun sharePlaylist(id: Long, user: Long) = withContext(Dispatchers.IO) {
        send("PUT", "/api/playlists/$id/shares/$user", "{}")
        Unit
    }

    suspend fun unsharePlaylist(id: Long, user: Long) = withContext(Dispatchers.IO) {
        send("DELETE", "/api/playlists/$id/shares/$user")
        Unit
    }
    suspend fun addToPlaylist(playlist: Long, song: Long) =
        write("POST", "/api/go/playlists/$playlist/songs", SongIds(listOf(song)))

    suspend fun sendToParty(id: Long) = withContext(Dispatchers.IO) {
        send("POST", "/api/go/party/$id")
        Unit
    }

    suspend fun saveResume(save: ResumeSave) = write("PUT", "/api/go/resume", save)

    /** Every song by an artist, a page at a time (up to 5,000). */
    suspend fun artistSongs(id: Long, offset: Int, limit: Int): ArtistSongs =
        get("/api/artists/$id/songs?offset=$offset&limit=$limit")

    /** Songs you played on GO, newest first, 100 at a time. */
    suspend fun history(offset: Int): List<Track> = get("/api/go/history?offset=$offset")

    suspend fun year(year: Int?): GoYear = get(if (year == null) "/api/go/year" else "/api/go/year?year=$year")

    /** Whether this account gets "Not in the library?". */
    suspend fun wishesEnabled(): Boolean = get("/api/wishes/enabled")

    suspend fun addWish(artist: String, title: String) = write("POST", "/api/wishes", NewWish(artist, title))

    suspend fun reportSong(id: Long, reason: String, note: String?) =
        write("POST", "/api/tracks/$id/report", SongReport(reason, note))
}
