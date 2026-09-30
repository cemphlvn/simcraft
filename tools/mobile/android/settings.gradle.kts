// The Android app around sim-mobile (docs/architecture.md, Platforms and builds). Built by tools/mobile/build.sh,
// which compiles the Rust library with cargo-ndk first.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}
rootProject.name = "simcraft"
include(":app")
