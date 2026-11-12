package com.newterrastudios.djinn.go

import com.newterrastudios.djinn.go.data.AppUpdater
import android.app.Application
import androidx.media3.common.util.UnstableApi
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.cache.LeastRecentlyUsedCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import coil3.network.okhttp.OkHttpNetworkFetcherFactory
import com.newterrastudios.djinn.go.data.Account
import com.newterrastudios.djinn.go.data.GoApi
import com.newterrastudios.djinn.go.data.OfflineStore
import com.newterrastudios.djinn.go.data.Prefs
import java.io.File

/** How much the recent cache keeps of what you've played. */
private const val RECENT_CACHE_BYTES = 2L * 1024 * 1024 * 1024

@UnstableApi
class DjinnApp : Application(), SingletonImageLoader.Factory {
    override fun onCreate() {
        super.onCreate()
        com.newterrastudios.djinn.go.i18n.Strings.load(this)
    }

    val account by lazy { Account(this) }
    val api by lazy { GoApi(account).also { it.onUnreachable = { seeker.lost() } } }
    /** Follows the server in use to a new address on the network. */
    val seeker by lazy { com.newterrastudios.djinn.go.data.ServerSeeker(this, account) }
    val prefs by lazy { Prefs(this) { account.scope.orEmpty() } }
    val offline by lazy { OfflineStore(this, account, api, prefs) }
    val updater by lazy { AppUpdater(this, api, account) }

    /** Songs as they were streamed, oldest dropped past the limit. */
    val recentCache by lazy {
        SimpleCache(
            File(cacheDir, "songs"),
            LeastRecentlyUsedCacheEvictor(RECENT_CACHE_BYTES),
            StandaloneDatabaseProvider(this),
        )
    }

    override fun newImageLoader(context: PlatformContext): ImageLoader =
        ImageLoader.Builder(context)
            .components { add(OkHttpNetworkFetcherFactory(callFactory = { api.http })) }
            .build()
}
