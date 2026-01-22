package org.tasks.desktop.platform

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.io.File
import java.util.Properties

/**
 * Manages theme preferences for the desktop application.
 */
class ThemeManager {
    private val configFile = File(DesktopPaths.configDir, "theme.properties")

    var isDarkTheme by mutableStateOf(loadDarkThemePreference())
        private set

    var themeMode by mutableStateOf(loadThemeMode())
        private set

    fun updateDarkTheme(dark: Boolean) {
        isDarkTheme = dark
        themeMode = if (dark) ThemeMode.DARK else ThemeMode.LIGHT
        savePreferences()
    }

    fun updateThemeMode(mode: ThemeMode) {
        themeMode = mode
        isDarkTheme = when (mode) {
            ThemeMode.LIGHT -> false
            ThemeMode.DARK -> true
            ThemeMode.SYSTEM -> isSystemDarkMode()
        }
        savePreferences()
    }

    private fun loadDarkThemePreference(): Boolean {
        val mode = loadThemeMode()
        return when (mode) {
            ThemeMode.LIGHT -> false
            ThemeMode.DARK -> true
            ThemeMode.SYSTEM -> isSystemDarkMode()
        }
    }

    private fun loadThemeMode(): ThemeMode {
        val properties = loadProperties()
        val modeStr = properties.getProperty("themeMode", ThemeMode.SYSTEM.name)
        return try {
            ThemeMode.valueOf(modeStr)
        } catch (e: IllegalArgumentException) {
            ThemeMode.SYSTEM
        }
    }

    private fun savePreferences() {
        val properties = Properties().apply {
            setProperty("themeMode", themeMode.name)
        }

        configFile.parentFile?.mkdirs()
        configFile.outputStream().use { output ->
            properties.store(output, "Tasks Desktop Theme Settings")
        }
    }

    private fun loadProperties(): Properties {
        val properties = Properties()
        if (configFile.exists()) {
            try {
                configFile.inputStream().use { input ->
                    properties.load(input)
                }
            } catch (e: Exception) {
                // Ignore corrupt config file
            }
        }
        return properties
    }

    private fun isSystemDarkMode(): Boolean {
        // Try to detect system dark mode on different platforms
        val os = System.getProperty("os.name").lowercase()
        return when {
            os.contains("mac") -> detectMacDarkMode()
            os.contains("win") -> detectWindowsDarkMode()
            else -> detectLinuxDarkMode()
        }
    }

    private fun detectMacDarkMode(): Boolean {
        return try {
            val process = ProcessBuilder("defaults", "read", "-g", "AppleInterfaceStyle")
                .redirectErrorStream(true)
                .start()
            val result = process.inputStream.bufferedReader().readText().trim()
            process.waitFor()
            result.equals("Dark", ignoreCase = true)
        } catch (e: Exception) {
            false
        }
    }

    private fun detectWindowsDarkMode(): Boolean {
        return try {
            val process = ProcessBuilder(
                "reg", "query",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
                "/v", "AppsUseLightTheme"
            ).redirectErrorStream(true).start()
            val result = process.inputStream.bufferedReader().readText()
            process.waitFor()
            // AppsUseLightTheme = 0 means dark mode
            result.contains("0x0")
        } catch (e: Exception) {
            false
        }
    }

    private fun detectLinuxDarkMode(): Boolean {
        return try {
            // Try GTK settings
            val process = ProcessBuilder("gsettings", "get", "org.gnome.desktop.interface", "color-scheme")
                .redirectErrorStream(true)
                .start()
            val result = process.inputStream.bufferedReader().readText().trim()
            process.waitFor()
            result.contains("dark", ignoreCase = true)
        } catch (e: Exception) {
            // Try KDE settings
            try {
                val kdeProcess = ProcessBuilder("kreadconfig5", "--group", "General", "--key", "ColorScheme")
                    .redirectErrorStream(true)
                    .start()
                val kdeResult = kdeProcess.inputStream.bufferedReader().readText().trim()
                kdeProcess.waitFor()
                kdeResult.contains("dark", ignoreCase = true)
            } catch (e2: Exception) {
                false
            }
        }
    }

    enum class ThemeMode {
        LIGHT,
        DARK,
        SYSTEM
    }
}
