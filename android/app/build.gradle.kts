plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ksp)
}

// The app is released with the server and numbered by the same date, so each
// release is an upgrade to Android and the server can tell what's newer.
val release: List<Int> = (
    providers.gradleProperty("djinnVersion").orNull
        ?: Regex("(?m)^version = \"([0-9.]+)\"").find(rootDir.resolve("../Cargo.toml").readText())!!.groupValues[1]
    ).split('.').map(String::toInt)

android {
    namespace = "com.newterrastudios.djinn.go"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.newterrastudios.djinn.go"
        // The lowest the libraries allow: Cast and WorkManager need 24.
        minSdk = 24
        targetSdk = 37
        versionCode = release[0] * 10_000 + release[1] * 100 + release[2]
        versionName = "%04d.%02d.%02d".format(release[0], release[1], release[2])
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
