plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ksp)
}

// The app is released with the server and numbered by the same date, so each
// release is an upgrade to Android and the server can tell what's newer. A
// nightly (2026.11.5-nightly) comes just before that day's stable release; the
// server works the number out the same way (version_code in go_app.rs).
val djinnVersion: String = providers.gradleProperty("djinnVersion").orNull
    ?: Regex("(?m)^version = \"([0-9.]+(?:-nightly)?)\"").find(rootDir.resolve("../Cargo.toml").readText())!!.groupValues[1]
val nightly = djinnVersion.endsWith("-nightly")
val release: List<Int> = djinnVersion.removeSuffix("-nightly").split('.').map(String::toInt)

// The translations are the web app's (web/src/i18n), shared, copied into the
// app's assets as i18n/<lang>.json.
val copyTranslations by tasks.registering(Sync::class) {
    from("../../web/src/i18n") { include("*.json") }
    into(layout.buildDirectory.dir("generated/translations/i18n"))
}
tasks.named("preBuild") { dependsOn(copyTranslations) }

android {
    sourceSets.getByName("main").assets.srcDir(file("build/generated/translations"))
    namespace = "com.newterrastudios.djinn.go"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.newterrastudios.djinn.go"
        // The lowest the libraries allow: Cast and WorkManager need 24.
        minSdk = 24
        targetSdk = 37
        versionCode = (release[0] * 10_000 + release[1] * 100 + release[2]) * 100 + if (nightly) 0 else 50
        versionName = "%04d.%02d.%02d".format(release[0], release[1], release[2]) + if (nightly) "-nightly" else ""
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    // The release key comes from the release workflow; see docs/releasing.md.
    val keystore = System.getenv("DJINN_ANDROID_KEYSTORE")
    if (keystore != null) {
        signingConfigs.create("release") {
            storeFile = file(keystore)
            storePassword = System.getenv("DJINN_ANDROID_KEYSTORE_PASSWORD")
            keyAlias = System.getenv("DJINN_ANDROID_KEY_ALIAS")
            keyPassword = System.getenv("DJINN_ANDROID_KEY_PASSWORD")
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
        }
        release {
            if (keystore != null) signingConfig = signingConfigs.getByName("release")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    buildFeatures {
        buildConfig = true
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(libs.androidx.core)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.icons)
    implementation(libs.media3.exoplayer)
    implementation(libs.media3.session)
    implementation(libs.media3.okhttp)
    implementation(libs.okhttp)
    implementation(libs.serialization.json)
    implementation(libs.coil.compose)
    implementation(libs.coil.okhttp)
    implementation(libs.coroutines.guava)
    implementation(libs.room.runtime)
    implementation(libs.room.ktx)
    ksp(libs.room.compiler)
    implementation(libs.work)
    implementation(libs.camerax.camera2)
    implementation(libs.camerax.lifecycle)
    implementation(libs.camerax.view)
    implementation(libs.zxing)
    implementation(libs.mediarouter)
    implementation(libs.cast)
    // Cast brings an old one, which lint rejects with the activity result APIs.
    implementation(libs.fragment)
    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.junit)
}
