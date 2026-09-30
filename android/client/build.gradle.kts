plugins {
    alias(libs.plugins.android.library)
}

android {
    namespace = "dev.eris.auth.client"
    compileSdk = 37
    defaultConfig {
        minSdk = 30
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        aidl = false
        buildConfig = false
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    testImplementation(libs.junit)
    // Android's org.json is a stub on the JVM; the tests use the real one.
    testImplementation(libs.json)
}
