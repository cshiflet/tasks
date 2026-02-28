import org.jetbrains.compose.desktop.application.dsl.TargetFormat

plugins {
    alias(libs.plugins.jetbrains.kotlin.jvm)
    alias(libs.plugins.jetbrains.compose)
    alias(libs.plugins.kotlin.compose.compiler)
    alias(libs.plugins.kotlin.serialization)
}

configurations.all {
    // Force version 0.6.2 which has Instant as a class, not a type alias
    resolutionStrategy {
        force("org.jetbrains.kotlinx:kotlinx-datetime:0.6.2")
        force("org.jetbrains.kotlinx:kotlinx-datetime-jvm:0.6.2")
    }
}

dependencies {
    // Use api to ensure transitive dependencies are included
    api(projects.kmp)
    api(projects.data)
    implementation(compose.desktop.currentOs)
    implementation(compose.material3)
    implementation(compose.materialIconsExtended)
    implementation(compose.components.resources)
    implementation(libs.kotlinx.serialization)
    // Explicitly include kotlinx.datetime with JVM artifact for runtime
    implementation(libs.kotlinx.datetime)
    implementation("org.jetbrains.kotlinx:kotlinx-datetime-jvm:0.7.1")
    implementation(libs.kotlinx.immutable)
    implementation(libs.kermit)
    // Swing dispatcher for Dispatchers.Main on desktop
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-swing:1.10.2")
    implementation(libs.androidx.datastore)
    implementation(libs.androidx.room)
    implementation(libs.androidx.sqlite)
    implementation(libs.okhttp)
    implementation(libs.bitfire.dav4jvm)
    implementation(libs.etebase.jvm)
    // ical4j is available transitively via dav4jvm but we declare it explicitly for clarity.
    // ical4android is Android-only (AAR) so we use net.fortuna.ical4j directly here.
    implementation("org.mnode.ical4j:ical4j:3.2.19")
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

// Custom task to run with full classpath (workaround for Compose Desktop classpath issues)
tasks.register<JavaExec>("runApp") {
    dependsOn("classes")
    mainClass.set("org.tasks.desktop.MainKt")
    classpath = sourceSets["main"].runtimeClasspath
}
