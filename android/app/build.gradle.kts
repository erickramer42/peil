// The Android app (ADR-0004): platform APIs only, Kotlin through the
// Android Gradle plugin's built-in support. The only libraries in the APK are
// the Kotlin standard library and its annotations (ADR-0004 section 8).
plugins {
    id("com.android.application")
}

val major = 0
val minor = 1
val patch = 0

android {
    namespace = "io.github.erickramer42.sensio"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.github.erickramer42.peil"
        minSdk = 31
        targetSdk = 36
        versionCode = major * 10000 + minor * 100 + patch
        versionName = "$major.$minor.$patch"
        ndk { abiFilters += "arm64-v8a" }
    }

    // ADR-0004 section 4: the core ships as libsensio.so and must be extracted
    // to disk so it can be executed.
    packaging {
        jniLibs { useLegacyPackaging = true }
    }
}

dependencies {
    // Tests only; never in the APK.
    testImplementation("junit:junit:4.13.2")
}
