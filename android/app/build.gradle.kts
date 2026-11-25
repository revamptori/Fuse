import java.io.File
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Signing: a release keystore can be provided through environment variables
// (CI) or ~/.fuss-keystore.properties. Without one the release variant is left
// unsigned; CI then ships the debug-signed APK for sideloading instead.
val keystoreProps = Properties().apply {
    val f = File(System.getProperty("user.home"), ".fuss-keystore.properties")
    if (f.exists()) f.inputStream().use { load(it) }
    listOf("FUSS_STORE_FILE", "FUSS_STORE_PASSWORD", "FUSS_KEY_ALIAS", "FUSS_KEY_PASSWORD").forEach { k ->
        System.getenv(k)?.let { setProperty(k, it) }
    }
}
val hasKeystore = keystoreProps.getProperty("FUSS_STORE_FILE") != null

android {
    namespace = "cloud.fuss"
    compileSdk = 35
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "cloud.fuss"
        minSdk = 30
        targetSdk = 35
        versionCode = 1
        versionName = "1.0.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        resourceConfigurations += listOf("en")
        ndk {
            // ARM64 is the primary target; armeabi-v7a keeps older devices
            // working. x86_64 is kept for emulator testing.
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // A real distribution key wins. Without one the release variant is
            // signed with the debug key so the APK is still installable — the
            // artifact name and the release notes say it is test-signed, and
            // nobody should mistake it for an official build.
            signingConfig = if (hasKeystore) {
                signingConfigs.create("release") {
                    storeFile = File(keystoreProps.getProperty("FUSS_STORE_FILE"))
                    storePassword = keystoreProps.getProperty("FUSS_STORE_PASSWORD")
                    keyAlias = keystoreProps.getProperty("FUSS_KEY_ALIAS")
                    keyPassword = keystoreProps.getProperty("FUSS_KEY_PASSWORD")
                }
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    packaging {
        resources.excludes += listOf("/META-INF/{AL2.0,LGPL2.1}")
        jniLibs.useLegacyPackaging = false
    }
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "armeabi-v7a", "x86_64")
            isUniversalApk = true
        }
    }
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")
}

// Every dependency here earns its place; nothing is pulled in "just in case".
// ---------------------------------------------------------------------------
// androidx.core:ktx                  FileProvider, edge-to-edge helpers (small)
// androidx.activity:activity-compose setContent + BackHandler + ActivityResult
// androidx.lifecycle:viewmodel-compose  viewModel() for BrowseViewModel
// compose BOM + ui/material3/foundation  the whole UI; Material3 is the design
//                                    system the brief asks for
// material-icons-extended           outlined pictograms (folder/archive/...)
// coil-compose                      image thumbnails, decode/caching done by
//                                    Coil instead of hand-rolled bitmaps
// documentfile                      SAF SAF-root enumeration (framework wrapper,
//                                    no transitive weight)
// junit                             the only test dependency
dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.documentfile:documentfile:1.0.1")

    val composeBom = platform("androidx.compose:compose-bom:2024.10.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui-tooling-preview")

    implementation("io.coil-kt:coil-compose:2.7.0")

    debugImplementation("androidx.compose.ui:ui-tooling")

    testImplementation("junit:junit:4.13.2")

    // Instrumented tests (./gradlew connectedDebugAndroidTest). The runner and
    // androidx.test are the standard device-test stack; nothing else is added.
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:rules:1.6.1")
}

// Build the Rust engine for every ABI that is being packaged by delegating to
// scripts/build-rust-android.sh (same script CI uses, so local and CI builds are
// identical). Set FUSS_SKIP_RUST=1 to reuse existing jniLibs (faster iteration
// on Kotlin-only changes).
val cargoTask = tasks.register<Exec>("cargoBuildAndroid") {
    val root = rootProject.projectDir.parentFile // repo root
    workingDir = root
    inputs.dir(File(root, "rust/fuss-core/src"))
    inputs.dir(File(root, "rust/fuss-jni/src"))
    inputs.file(File(root, "rust/Cargo.toml"))
    inputs.file(File(root, "rust/Cargo.lock"))
    outputs.dir(File(projectDir, "src/main/jniLibs"))
    commandLine("bash", "scripts/build-rust-android.sh")
    isIgnoreExitValue = System.getenv("FUSS_SKIP_RUST") == "1"
    onlyIf { System.getenv("FUSS_SKIP_RUST") != "1" }
}
tasks.named("preBuild") { dependsOn(cargoTask) }
