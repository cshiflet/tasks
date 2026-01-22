package org.tasks.desktop.platform

import java.io.File

object DesktopPaths {
    private val os: String = System.getProperty("os.name").lowercase()
    private val userHome: String = System.getProperty("user.home")

    val appDataDir: File by lazy {
        val dir = when {
            os.contains("win") -> File(System.getenv("APPDATA") ?: "$userHome/AppData/Roaming", "Tasks")
            os.contains("mac") -> File(userHome, "Library/Application Support/Tasks")
            else -> File(userHome, ".local/share/tasks")
        }
        dir.also { it.mkdirs() }
    }

    val databaseFile: File
        get() = File(appDataDir, "tasks.db")

    val preferencesFile: File
        get() = File(appDataDir, "preferences.preferences_pb")

    val attachmentsDir: File
        get() = File(appDataDir, "attachments").also { it.mkdirs() }

    val cacheDir: File
        get() = File(appDataDir, "cache").also { it.mkdirs() }

    val logsDir: File
        get() = File(appDataDir, "logs").also { it.mkdirs() }

    val configDir: File
        get() = File(appDataDir, "config").also { it.mkdirs() }

    fun ensureDirectoriesExist() {
        appDataDir.mkdirs()
        attachmentsDir.mkdirs()
        cacheDir.mkdirs()
        logsDir.mkdirs()
    }
}
