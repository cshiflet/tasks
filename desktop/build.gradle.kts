import org.jetbrains.compose.desktop.application.dsl.TargetFormat

plugins {
    alias(libs.plugins.jetbrains.kotlin.jvm)
    alias(libs.plugins.jetbrains.compose)
    alias(libs.plugins.kotlin.compose.compiler)
    alias(libs.plugins.kotlin.serialization)
}

dependencies {
    implementation(projects.kmp)
    implementation(projects.data)
    implementation(compose.desktop.currentOs)
    implementation(compose.material3)
    implementation(compose.materialIconsExtended)
    implementation(compose.components.resources)
    implementation(libs.kotlinx.serialization)
    implementation(libs.kotlinx.datetime)
    implementation(libs.kotlinx.immutable)
    implementation(libs.kermit)
    implementation(libs.androidx.datastore)
    implementation(libs.androidx.room)
    implementation(libs.androidx.sqlite)
    implementation(libs.okhttp)
    implementation(libs.bitfire.dav4jvm)
    // Note: ical4android and etebase are Android-only (AAR)
    // CalDAV sync will need alternative iCal parsing for desktop
    implementation(libs.google.api.tasks)
    implementation(libs.google.oauth2)
}

compose.desktop {
    application {
        mainClass = "org.tasks.desktop.MainKt"
        nativeDistributions {
            targetFormats(TargetFormat.Dmg, TargetFormat.Msi, TargetFormat.Deb)
            packageName = "Tasks"
            packageVersion = "${libs.versions.versionName.get()}.0"
            description = "Tasks - Open Source To-Do List"
            vendor = "Tasks.org"

            linux {
                iconFile.set(project.file("src/main/resources/icon.png"))
            }
            macOS {
                bundleID = "org.tasks.desktop"
                // Note: macOS requires .icns format - convert icon.png to icon.icns
                // for distribution: iconFile.set(project.file("src/main/resources/icon.icns"))
            }
            windows {
                menuGroup = "Tasks"
                // Note: Windows requires .ico format - convert icon.png to icon.ico
                // for distribution: iconFile.set(project.file("src/main/resources/icon.ico"))
            }
        }
    }
}

kotlin {
    jvmToolchain(21)
}
