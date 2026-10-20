// 与 tauri-plugin-opener 的 build.gradle.kts 保持同一套写法，
// 减少和 tauri-cli 生成工程时的版本假设冲突。
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.junlive.immersive"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

dependencies {
    // WindowInsetsControllerCompat 在这个包里
    implementation("androidx.core:core-ktx:1.9.0")
    implementation(project(":tauri-android"))
}
