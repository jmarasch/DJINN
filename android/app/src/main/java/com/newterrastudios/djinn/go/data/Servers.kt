package com.newterrastudios.djinn.go.data

import android.annotation.SuppressLint
import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import java.security.KeyStore
import java.security.MessageDigest
import java.security.cert.Certificate
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLSession
import javax.net.ssl.TrustManagerFactory
import javax.net.ssl.X509TrustManager

/**
 * A server this phone knows. People move between servers, so each one keeps its
 * own sign-in, and leaving one for another signs nobody out.
 */
@Serializable
data class SavedServer(
    val address: String,
    /** The server's own id, which outlasts a new address on the network. */
    val id: String? = null,
    val name: String? = null,
    val token: String? = null,
    /** Who's signed in, as the server numbers them. */
    val accountId: Long? = null,
    /** The certificate it must present; see `tls.rs` on the server. */
    val pin: String? = null,
)

/** The address with another scheme, keeping the host and port. */
internal fun withScheme(address: String, scheme: String): String? =
    address.toHttpUrlOrNull()?.newBuilder()?.scheme(scheme)?.build()?.toString()?.trimEnd('/')

/** The certificate a QR code says to expect. */
internal fun pinFromQr(text: String): String? =
    text.substringAfter("#fp=", "").takeIf { pin ->
        pin.length == 64 && pin.all { it in '0'..'9' || it in 'a'..'f' }
    }

/** A short form of a certificate's fingerprint, to compare with Admin > GO. */
fun securityCode(pin: String): String = pin.take(16).uppercase().chunked(4).joinToString("-")

internal fun fingerprint(certificate: ByteArray): String =
    MessageDigest.getInstance("SHA-256").digest(certificate).joinToString("") { "%02x".format(it) }

fun fingerprint(certificate: Certificate): String = fingerprint(certificate.encoded)

/** Whose songs and settings: one account on one server, whatever its address is today. */
fun scopeOf(server: SavedServer): String? {
    val accountId = server.accountId ?: return null
    return "${serverScope(server)}$accountId"
}

/** What every account's scope on this server begins with. */
internal fun serverScope(server: SavedServer) = "${server.id ?: server.address}#"

/** The server at this host and port, among those known. */
internal fun serverAt(servers: List<SavedServer>, host: String, port: Int): SavedServer? =
    servers.firstOrNull { saved ->
        saved.address.toHttpUrlOrNull()?.let { it.host == host && it.port == port } == true
    }

/** Which servers this phone knows, and which one it's using. Private to the app; never backed up. */
class Account(context: Context) {
    private val prefs = context.getSharedPreferences("account", Context.MODE_PRIVATE)
    private val json = Json { ignoreUnknownKeys = true }
    private val _servers = MutableStateFlow(load())
    private val _current = MutableStateFlow(_servers.value.firstOrNull { it.address == prefs.getString(CURRENT, null) })
    private val _signedIn = MutableStateFlow(_current.value?.token != null && _current.value?.accountId != null)
    /** A new sign-in's token, while finding out whose it is. */
    @Volatile
    private var held: String? = null

    val servers: StateFlow<List<SavedServer>> = _servers
    val current: StateFlow<SavedServer?> = _current
    val signedIn: StateFlow<Boolean> = _signedIn

    val server: String? get() = _current.value?.address
    val token: String? get() = _current.value?.token ?: held
    val serverName: String? get() = _current.value?.name
    val pin: String? get() = _current.value?.pin

    /** Songs and per-server settings go under this; null when signed out. */
    val scope: String? get() = _current.value?.takeIf { it.token != null }?.let(::scopeOf)

    /** For what can't be encrypted this way: cast devices fetch songs themselves. */
    val plainAddress: String?
        get() = _current.value?.let { saved ->
            if (saved.pin != null) withScheme(saved.address, "http") else saved.address
        }

    private fun load(): List<SavedServer> =
        prefs.getString(SERVERS, null)
            ?.let { runCatching { json.decodeFromString<List<SavedServer>>(it) }.getOrNull() }
            .orEmpty()

    @Synchronized
    private fun save(servers: List<SavedServer>, current: String?) {
        prefs.edit {
            putString(SERVERS, json.encodeToString(servers))
            if (current != null) putString(CURRENT, current) else remove(CURRENT)
        }
        val now = servers.firstOrNull { it.address == current }
        _servers.value = servers
        _current.value = now
        _signedIn.value = now?.token != null && now.accountId != null
    }

    private fun updateCurrent(change: (SavedServer) -> SavedServer) {
        val now = _current.value ?: return
        val changed = change(now)
        save(_servers.value.map { if (it.address == now.address) changed else it }, changed.address)
    }

    /** The saved server with this id, or else at this address. */
    fun saved(id: String?, address: String?): SavedServer? =
        _servers.value.firstOrNull { id != null && it.id == id }
            ?: address?.toHttpUrlOrNull()?.let { serverAt(_servers.value, it.host, it.port) }

    /** Starts using a server, remembering it, and its sign-in if it already has one. */
    fun use(server: SavedServer) {
        val others = _servers.value.filter {
            it.address != server.address && (server.id == null || it.id != server.id)
        }
        save(others + server, server.address)
    }

    /** Back to choosing a server. Nothing is signed out. */
    fun leave() = save(_servers.value, null)

    /** Forgets a server altogether. */
    fun forget(server: SavedServer) {
        val left = _servers.value.filter { it.address != server.address }
        save(left, _current.value?.address?.takeIf { it != server.address })
    }

    /** A server with a certificate to check can safely follow a new address. */
    fun moved(id: String, address: String) {
        val known = _servers.value.firstOrNull { it.id == id && it.pin != null } ?: return
        val secure = withScheme(address, "https") ?: return
        if (known.address == secure) return
        val current = _current.value?.address
        save(
            _servers.value.map { if (it === known) it.copy(address = secure) else it },
            if (current == known.address) secure else current,
        )
    }

    fun setServerName(name: String) = updateCurrent { it.copy(name = name) }

    /** Lets a new sign-in's first request through, before it counts as signed in. */
    fun holdToken(token: String?) {
        held = token
    }

    fun signIn(token: String, accountId: Long) {
        held = null
        updateCurrent { it.copy(token = token, accountId = accountId) }
    }

    fun signOut() {
        held = null
        updateCurrent { it.copy(token = null, accountId = null) }
    }

    private companion object {
        const val SERVERS = "servers"
        const val CURRENT = "current"
    }
}

/**
 * Accepts a known server's own certificate, and for everything else (a server
 * behind a real HTTPS certificate) what Android normally would. This only lets
 * the connection open: `GoApi` then checks the certificate is the one for that
 * particular server before anything is sent.
 */
class PinnedTrust(private val pins: () -> Set<String>) : X509TrustManager {
    private val system: X509TrustManager =
        TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm())
            .apply { init(null as KeyStore?) }
            .trustManagers
            .filterIsInstance<X509TrustManager>()
            .first()

    override fun checkClientTrusted(chain: Array<out X509Certificate>, authType: String) =
        system.checkClientTrusted(chain, authType)

    override fun checkServerTrusted(chain: Array<out X509Certificate>, authType: String) {
        val leaf = chain.firstOrNull() ?: throw CertificateException("No certificate")
        if (fingerprint(leaf) !in pins()) system.checkServerTrusted(chain, authType)
    }

    override fun getAcceptedIssuers(): Array<X509Certificate> = system.acceptedIssuers
}

/** A known server's certificate names no host; anything else has to match as usual. */
class PinnedHostnames(private val pins: () -> Set<String>) : HostnameVerifier {
    override fun verify(hostname: String, session: SSLSession): Boolean {
        val leaf = runCatching { session.peerCertificates.firstOrNull() }.getOrNull()
        if (leaf != null && fingerprint(leaf) in pins()) return true
        return HttpsURLConnection.getDefaultHostnameVerifier().verify(hostname, session)
    }
}

/** For the one request that reads a certificate before anything is trusted. Sends nothing private. */
@SuppressLint("CustomX509TrustManager", "TrustAllX509TrustManager")
internal class CertificateReader : X509TrustManager {
    @Volatile
    var seen: X509Certificate? = null
        private set

    override fun checkClientTrusted(chain: Array<out X509Certificate>, authType: String) = Unit

    override fun checkServerTrusted(chain: Array<out X509Certificate>, authType: String) {
        seen = chain.firstOrNull()
    }

    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
}
