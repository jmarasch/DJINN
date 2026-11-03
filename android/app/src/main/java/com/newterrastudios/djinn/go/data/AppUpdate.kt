package com.newterrastudios.djinn.go.data

import android.content.Context
import android.content.Intent
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.security.MessageDigest

/** The app a server carries. */
@Serializable
data class AppRelease(
    val version: String,
    @SerialName("version_code") val versionCode: Long,
    val sha256: String,
    val size: Long,
)

/** Only ever newer: moving to a server with an older version offers nothing. */
fun offeredUpdate(release: AppRelease?, installedCode: Long): AppRelease? =
    release?.takeIf { it.versionCode > installedCode }

internal fun sha256Of(file: File): String {
    val digest = MessageDigest.getInstance("SHA-256")
    file.inputStream().use { input ->
        val buffer = ByteArray(64 * 1024)
        while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            digest.update(buffer, 0, read)
        }
    }
    return digest.digest().joinToString("") { "%02x".format(it) }
}

/**
 * Fetches a new version over the app's checked connection, then hands it to
 * Android, which asks the person first and refuses anything not signed with the
 * same key as this app.
 */
class AppUpdater(private val context: Context, private val api: GoApi, private val account: Account) {
    suspend fun fetch(release: AppRelease): File = withContext(Dispatchers.IO) {
        val server = account.server ?: throw IOException("No server chosen")
        val folder = File(context.cacheDir, "updates").apply { mkdirs() }
        folder.listFiles()?.forEach { it.delete() }
        val file = File(folder, "djinn-go-${release.version}.apk")
        val request = Request.Builder().url(server + APK_PATH).build()
        api.http.newCall(request).execute().use { response ->
            if (!response.isSuccessful) throw ApiException(response.code, "The server answered ${response.code}")
            file.outputStream().use { out -> response.body.byteStream().copyTo(out) }
        }
        if (sha256Of(file) != release.sha256) {
            file.delete()
            throw IOException("The download didn't match what the server said it was sending.")
        }
        file
    }

    fun install(file: File) {
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", file)
        context.startActivity(
            Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }

    private companion object {
        const val APK_PATH = "/go/app/djinn-go.apk"
    }
}
