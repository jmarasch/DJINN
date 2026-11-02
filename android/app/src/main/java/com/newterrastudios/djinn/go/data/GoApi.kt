package com.newterrastudios.djinn.go.data

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
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
import java.net.URLEncoder
import java.util.concurrent.TimeUnit

/** The server said no, with something to show the person. */
class ApiException(val status: Int, message: String) : IOException(message)

@Serializable
private data class ErrorBody(val error: String? = null)

/** Which server and account this phone uses. Private to the app; never backed up. */
class Account(context: Context) {
    private val prefs = context.getSharedPreferences("account", Context.MODE_PRIVATE)
    private val _signedIn = MutableStateFlow(prefs.getString(TOKEN, null) != null)

    val signedIn: StateFlow<Boolean> = _signedIn

    val server: String? get() = prefs.getString(SERVER, null)
    val token: String? get() = prefs.getString(TOKEN, null)

    fun setServer(address: String) = prefs.edit { putString(SERVER, address) }

    fun forgetServer() = prefs.edit { remove(SERVER) }

    fun signIn(token: String) {
        prefs.edit { putString(TOKEN, token) }
        _signedIn.value = true
    }

    fun signOut() {
        prefs.edit { remove(TOKEN) }
        _signedIn.value = false
    }

    private companion object {
        const val SERVER = "server"
        const val TOKEN = "token"
    }
}

/** DJINN GO's API on the server this phone is connected to. */
class GoApi(private val account: Account) {
    val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }
    private val jsonType = "application/json".toMediaType()

    /** Shared with the player and images, so they carry the sign-in too. */
    val http: OkHttpClient = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .addInterceptor(Interceptor { chain ->
            val request = chain.request()
            val token = account.token
            // The token only ever goes to the server it came from.
            val ours = account.server?.toHttpUrlOrNull()?.host == request.url.host
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

    suspend fun login(username: String, password: String): SignInReply = withContext(Dispatchers.IO) {
        json.decodeFromString(send("POST", "/api/auth/login", json.encodeToString(Login(username, password))))
    }

    suspend fun twoStep(ticket: String, code: String): SignInReply = withContext(Dispatchers.IO) {
        json.decodeFromString(send("POST", "/api/auth/two-step", json.encodeToString(TwoStep(ticket, code))))
    }

    suspend fun logout() = withContext(Dispatchers.IO) { send("POST", "/api/auth/logout"); Unit }

    suspend fun home(): GoHome = get("/api/go/home")
    suspend fun artists(offset: Int): Page<ArtistSummary> = get("/api/artists?offset=$offset")
    suspend fun albums(offset: Int): Page<Album> = get("/api/albums?sort=name&filter=all&offset=$offset")
    suspend fun songs(offset: Int): Page<Track> = get("/api/tracks?sort=title&offset=$offset")
    suspend fun album(id: Long): AlbumDetail = get("/api/albums/$id")
    suspend fun artist(id: Long): ArtistDetail = get("/api/artists/$id")
    suspend fun playlist(id: Long): GoPlaylistDetail = get("/api/go/playlists/$id")
    suspend fun radio(id: Long): List<Track> = get("/api/go/radio/$id")
    suspend fun resume(): GoResume? = home().resume
    suspend fun search(query: String): SearchResults =
        get("/api/search?q=${URLEncoder.encode(query, "UTF-8")}")

    suspend fun likedIds(): LikedIds = get("/api/go/likes/ids")
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
    suspend fun addToPlaylist(playlist: Long, song: Long) =
        write("POST", "/api/go/playlists/$playlist/songs", SongIds(listOf(song)))

    suspend fun sendToParty(id: Long) = withContext(Dispatchers.IO) {
        send("POST", "/api/go/party/$id")
        Unit
    }

    suspend fun saveResume(save: ResumeSave) = write("PUT", "/api/go/resume", save)
}
