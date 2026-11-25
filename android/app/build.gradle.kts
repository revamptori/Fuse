import java.io.File
import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// Signing: a release keystore can be provided through environment variables
// (CI) or ~/.fuss-keystore.properties. Without one, releases are unsigned and
// CI produces a debug-signed APK for sideloading instead.
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
            if (hasKeystore) {
                signingConfig = signingConfigs.create("release") {
                    storeFile = File(keystoreProps.getProperty("FUSS_STORE_FILE"))
                    storePassword = keystoreProps.getProperty("FUSS_STORE_PASSWORD")
                    keyAlias = keystoreProps.getProperty("FUSS_KEY_ALIAS")
                    keyPassword = keystoreProps.getProperty("FUSS_KEY_PASSWORD")
                }
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
