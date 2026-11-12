package com.newterrastudios.djinn.go.data

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress

private const val TAG = "DjinnFind"
private const val SERVICE = "_djinn-go._tcp"

/** A server heard on this network. */
data class FoundServer(
    val id: String,
    val name: String,
    val address: String,
    /** The certificate it claims. Only as good as the network; the QR code is better. */
    val pin: String? = null,
)

/** A server's address from what its announcement said. */
internal fun foundAddress(host: String, port: Int): String =
    if (':' in host) "http://[${host.substringBefore('%')}]:$port" else "http://$host:$port"

/** The list with this server added or brought up to date, in name order. */
internal fun withServer(servers: List<FoundServer>, found: FoundServer): List<FoundServer> =
    (servers.filter { it.id != found.id } + found)
        .sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER, FoundServer::name).thenBy(FoundServer::address))

/**
 * The address to use from those a server announced: IPv4 when there is one,
 * and not a link-local IPv6 one, which needs more than a URL can carry.
 */
internal fun bestHost(addresses: List<InetAddress>): String? =
    (addresses.firstOrNull { it is Inet4Address }
        ?: addresses.firstOrNull { it is Inet6Address && !it.isLinkLocalAddress })?.hostAddress

/**
 * Listens for DJINN servers on the network this phone is on. Android resolves
 * one service at a time, so what's found waits its turn. Nothing here is proof
 * of who a server is: anyone on the network can announce anything, so the
 * screen shows the address too and sign-in still goes through the usual checks.
 */
class ServerFinder(context: Context) {
    private val nsd = context.applicationContext.getSystemService(NsdManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private val _servers = MutableStateFlow<List<FoundServer>>(emptyList())
    private val waiting = ArrayDeque<NsdServiceInfo>()
    private val idByInstance = HashMap<String, String>()
    private var resolving = false
    private var running = false

    val servers: StateFlow<List<FoundServer>> = _servers

    private val listener = object : NsdManager.DiscoveryListener {
        override fun onDiscoveryStarted(serviceType: String) = Unit

        override fun onDiscoveryStopped(serviceType: String) = Unit

        override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
            Log.w(TAG, "Couldn't start looking: $errorCode")
            main.post { running = false }
        }

        override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
            Log.w(TAG, "Couldn't stop looking: $errorCode")
        }

        override fun onServiceFound(service: NsdServiceInfo) {
            main.post {
                waiting.addLast(service)
                resolveNext()
            }
        }

        override fun onServiceLost(service: NsdServiceInfo) {
            main.post {
                val id = idByInstance.remove(service.serviceName) ?: return@post
                _servers.value = _servers.value.filter { it.id != id }
            }
        }
    }

    fun start() {
        if (running || nsd == null) return
        running = true
        try {
            nsd.discoverServices(SERVICE, NsdManager.PROTOCOL_DNS_SD, listener)
        } catch (e: RuntimeException) {
            Log.w(TAG, "Couldn't look for servers", e)
            running = false
        }
    }

    fun stop() {
        if (!running || nsd == null) return
        running = false
        waiting.clear()
        try {
            nsd.stopServiceDiscovery(listener)
        } catch (e: RuntimeException) {
            Log.w(TAG, "Couldn't stop looking", e)
        }
    }

    @Suppress("DEPRECATION") // Its replacements only exist from Android 14.
    private fun resolveNext() {
        if (resolving || nsd == null) return
        val service = waiting.removeFirstOrNull() ?: return
        resolving = true
        nsd.resolveService(
            service,
            object : NsdManager.ResolveListener {
                override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) {
                    Log.i(TAG, "Couldn't resolve ${info.serviceName}: $errorCode")
                    main.post {
                        resolving = false
                        resolveNext()
                    }
                }

                override fun onServiceResolved(info: NsdServiceInfo) {
                    main.post {
                        resolving = false
                        if (running) add(info)
                        resolveNext()
                    }
                }
            },
        )
    }

    @Suppress("DEPRECATION")
    private fun add(info: NsdServiceInfo) {
        val addresses = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            info.hostAddresses
        } else {
            listOfNotNull(info.host)
        }
        val host = bestHost(addresses) ?: return
        val said = info.attributes
        val id = said["id"]?.decodeToString()?.takeIf { it.isNotBlank() } ?: info.serviceName
        val name = said["name"]?.decodeToString()?.takeIf { it.isNotBlank() } ?: info.serviceName
        idByInstance[info.serviceName] = id
        val pin = said["fp"]?.decodeToString()?.takeIf { it.isNotBlank() }
        _servers.value = withServer(_servers.value, FoundServer(id, name, foundAddress(host, info.port), pin))
    }
}

/** How long to listen for the server each time, and how often at most. */
private const val SEEK_FOR_MS = 15_000L
private const val SEEK_EVERY_MS = 30_000L

/** Which announcement is the server in use, if any: the one with its id. */
internal fun announcedAs(servers: List<FoundServer>, id: String): FoundServer? = servers.firstOrNull { it.id == id }

/**
 * Finds the server in use again when its address changes (a router handing
 * out a new one). When it can't be reached, listens for its announcement and
 * follows it there. Only servers with a pinned certificate move: the new
 * address still has to present that certificate before anything is sent.
 */
class ServerSeeker(private val context: Context, private val account: Account) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    @Volatile
    private var lastLook = 0L

    /** Any thread. Looks at most every SEEK_EVERY_MS. */
    fun lost() {
        val now = SystemClock.elapsedRealtime()
        if (now - lastLook < SEEK_EVERY_MS) return
        lastLook = now
        val current = account.current.value ?: return
        val id = current.id ?: return
        if (current.pin == null) return
        scope.launch {
            val finder = ServerFinder(context)
            finder.start()
            val heard = withTimeoutOrNull(SEEK_FOR_MS) {
                finder.servers.first { announcedAs(it, id) != null }
            }
            finder.stop()
            val found = heard?.let { announcedAs(it, id) } ?: return@launch
            Log.i(TAG, "${current.name ?: id} is at ${found.address} now")
            account.moved(id, found.address)
        }
    }
}
