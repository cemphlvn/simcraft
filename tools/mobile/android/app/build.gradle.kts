// No Java or Kotlin: the whole app is libsim_mobile.so, started by Android's NativeActivity.
plugins {
    id("com.android.application")
}

android {
    namespace = "dev.simcraft.mobile"
    compileSdk = 36
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "dev.simcraft.mobile"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += "arm64-v8a" }
    }

    buildTypes {
        release { isMinifyEnabled = false }
    }

    // The Rust library, built by cargo-ndk into the workspace's target/ (never committed).
    sourceSets {
        getByName("main") { jniLibs.directories.add("../../../../target/mobile/android/jniLibs") }
    }
}
