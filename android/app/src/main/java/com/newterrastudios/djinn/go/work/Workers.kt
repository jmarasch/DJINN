package com.newterrastudios.djinn.go.work

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.media3.common.util.UnstableApi
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.Data
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.ForegroundInfo
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.newterrastudios.djinn.go.DjinnApp
import com.newterrastudios.djinn.go.R
import com.newterrastudios.djinn.go.data.ApiException
import com.newterrastudios.djinn.go.data.KIND_DOWNLOAD
import com.newterrastudios.djinn.go.data.KIND_SMART
import com.newterrastudios.djinn.go.data.Track
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.io.File
import java.io.IOException
import java.util.concurrent.TimeUnit

const val DOWNLOAD_TAG = "download"
private const val CHANNEL = "downloads"
private const val NOTIFICATION = 7

private fun Context.downloadNotice(text: String): ForegroundInfo {
    val manager = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Downloads", NotificationManager.IMPORTANCE_LOW),
        )
    }
    val notice = NotificationCompat.Builder(this, CHANNEL)
        .setSmallIcon(R.drawable.ic_notification)
        .setContentTitle("DJINN GO")
        .setContentText(text)
        .setOngoing(true)
        .setSilent(true)
        .build()
    return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        ForegroundInfo(NOTIFICATION, notice, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
    } else {
        ForegroundInfo(NOTIFICATION, notice)
    }
}

/** Fetches one song you asked to keep. Retries when the network comes back. */
@UnstableApi
class DownloadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    private val app get() = applicationContext as DjinnApp

    override suspend fun getForegroundInfo() =
        applicationContext.downloadNotice("Downloading ${inputData.getString(TITLE) ?: "songs"}")

    override suspend fun doWork(): Result {
        val track = Json.decodeFromString<Track>(inputData.getString(TRACK) ?: return Result.failure())
        runCatching { setForeground(getForegroundInfo()) }
        return try {
            app.offline.fetch(track, KIND_DOWNLOAD)
            Result.success()
        } catch (e: ApiException) {
            // 403: not allowed. 404: gone from the library. Trying again won't help.
            if (e.status in 400..499) Result.failure() else Result.retry()
        } catch (_: IOException) {
            Result.retry()
        }
    }

    companion object {
        private const val TRACK = "track"
        private const val TITLE = "title"

        fun enqueue(context: Context, tracks: List<Track>, wifiOnly: Boolean) {
            val network = if (wifiOnly) NetworkType.UNMETERED else NetworkType.CONNECTED
            val constraints = Constraints.Builder().setRequiredNetworkType(network).build()
            val manager = WorkManager.getInstance(context)
            for (track in tracks) {
                val request = OneTimeWorkRequestBuilder<DownloadWorker>()
                    .setInputData(
                        Data.Builder()
                            .putString(TRACK, Json.encodeToString(track))
                            .putString(TITLE, track.title)
                            .build(),
                    )
                    .setConstraints(constraints)
                    .addTag(DOWNLOAD_TAG)
                    .build()
                manager.enqueueUniqueWork("download-${track.id}", ExistingWorkPolicy.KEEP, request)
            }
        }
    }
}

/** Keeps what you play most on the phone, on Wi-Fi while charging. */
@UnstableApi
class SmartCacheWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    private val app get() = applicationContext as DjinnApp

    override suspend fun getForegroundInfo() =
        applicationContext.downloadNotice("Keeping your favorites on this phone")

    override suspend fun doWork(): Result {
        val server = app.account.server ?: return Result.success()
        if (app.account.token == null) return Result.success()
        app.offline.checkAccess()
        if (!app.prefs.smartCache) {
            removeSmart(server, keep = emptySet())
            return Result.success()
        }
        val me = try {
            app.api.me()
        } catch (_: IOException) {
            return Result.retry()
        }
        if (!me.permissions.downloads) return Result.success()
        val wanted = try {
            app.api.suggested()
        } catch (_: IOException) {
            return Result.retry()
        }
        runCatching { setForeground(getForegroundInfo()) }
        val limit = app.prefs.smartLimitMb * 1024L * 1024L
        var used = 0L
        val keep = mutableSetOf<Long>()
        for (track in wanted) {
            if (isStopped) return Result.retry()
            val song = try {
                app.offline.fetch(track, KIND_SMART)
            } catch (e: ApiException) {
                if (e.status in 400..499) continue
                return Result.retry()
            } catch (_: IOException) {
                return Result.retry()
            }
            if (song.kind == KIND_DOWNLOAD) continue
            used += song.bytes
            keep += track.id
            if (used >= limit) break
        }
        removeSmart(server, keep)
        return Result.success()
    }

    private suspend fun removeSmart(server: String, keep: Set<Long>) {
        app.offline.dao.songs(server).filter { it.kind == KIND_SMART && it.id !in keep }.forEach {
            File(it.path).delete()
            app.offline.dao.deleteSong(server, it.id)
        }
    }

    companion object {
        private const val NAME = "smart-cache"

        /** Once a day, on Wi-Fi while charging. */
        fun schedule(context: Context) {
            val constraints = Constraints.Builder()
                .setRequiredNetworkType(NetworkType.UNMETERED)
                .setRequiresCharging(true)
                .build()
            val request = PeriodicWorkRequestBuilder<SmartCacheWorker>(1, TimeUnit.DAYS)
                .setConstraints(constraints)
                .build()
            WorkManager.getInstance(context)
                .enqueueUniquePeriodicWork(NAME, ExistingPeriodicWorkPolicy.KEEP, request)
        }

        /** Now, from Settings, on any Wi-Fi. */
        fun runNow(context: Context) {
            val request = OneTimeWorkRequestBuilder<SmartCacheWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.UNMETERED).build())
                .addTag(DOWNLOAD_TAG)
                .build()
            WorkManager.getInstance(context)
                .enqueueUniqueWork("$NAME-now", ExistingWorkPolicy.REPLACE, request)
        }
    }
}

/** Sends plays and likes from while offline, as soon as there's a network. */
@UnstableApi
class SyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val app = applicationContext as DjinnApp
        return try {
            app.offline.sync()
            app.offline.checkAccess()
            Result.success()
        } catch (_: IOException) {
            Result.retry()
        }
    }

    companion object {
        fun enqueue(context: Context) {
            val request = OneTimeWorkRequestBuilder<SyncWorker>()
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context).enqueueUniqueWork("sync", ExistingWorkPolicy.REPLACE, request)
        }
    }
}
