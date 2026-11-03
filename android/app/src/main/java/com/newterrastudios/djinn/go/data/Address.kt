package com.newterrastudios.djinn.go.data

import java.net.URI

/** GO's port unless the admin changed it. */
const val DEFAULT_GO_PORT = 8067

/**
 * Turns what someone typed into a server address, like
 * `http://192.168.1.20:8067`, or null if it can't be one. Without a scheme it's
 * plain HTTP on GO's port, since that's what a server at home looks like.
 */
fun serverAddress(input: String): String? {
    val typed = input.trim().trimEnd('/')
    if (typed.isEmpty() || typed.any { it.isWhitespace() }) return null
    val hasScheme = typed.contains("://")
    val uri = runCatching { URI(if (hasScheme) typed else "http://$typed") }.getOrNull() ?: return null
    val scheme = uri.scheme?.lowercase() ?: return null
    if (scheme != "http" && scheme != "https") return null
    val host = uri.host ?: return null
    val port = when {
        uri.port != -1 -> ":${uri.port}"
        hasScheme -> ""
        else -> ":$DEFAULT_GO_PORT"
    }
    return "$scheme://$host$port"
}

/**
 * Whether a password can go to this address as it is: HTTPS anywhere, or plain
 * HTTP on what counts as home for GO (private ranges and Tailscale).
 */
fun safeToSignIn(address: String): Boolean {
    val uri = runCatching { URI(address) }.getOrNull() ?: return false
    if (uri.scheme == "https") return true
    val host = uri.host?.lowercase() ?: return false
    if (host == "localhost" || host.endsWith(".local") || host.endsWith(".ts.net")) return true
    if (host.startsWith("[")) return host.startsWith("[fd") || host.startsWith("[fe80") || host == "[::1]"
    val parts = host.split('.').map { it.toIntOrNull() ?: return false }
    if (parts.size != 4 || parts.any { it !in 0..255 }) return false
    val (a, b) = parts
    return a == 10 || a == 127 || (a == 192 && b == 168) || (a == 172 && b in 16..31) ||
        (a == 169 && b == 254) || (a == 100 && b in 64..127)
}
