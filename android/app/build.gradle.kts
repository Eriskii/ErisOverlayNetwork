plugins {
    alias(libs.plugins.android.application)
}

android {
    namespace = "dev.eris.auth"
    compileSdk = 37
    defaultConfig {
        applicationId = "dev.eris.auth"
        minSdk = 30
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        aidl = false
        buildConfig = false
        shaders = false
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation(project(":client"))
    testImplementation(libs.junit)
    testImplementation(libs.json)
}
