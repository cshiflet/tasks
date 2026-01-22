package org.tasks.desktop.platform

import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.WindowPosition
import androidx.compose.ui.window.WindowState
import java.io.File
import java.util.Properties

/**
 * Manages window state persistence (size, position) across app restarts.
 */
class WindowStateManager {
    private val configFile = File(DesktopPaths.configDir, "window.properties")

    private val defaultWidth = 1200
    private val defaultHeight = 800

    fun loadWindowState(): WindowState {
        val properties = loadProperties()

        val width = properties.getProperty("width")?.toIntOrNull() ?: defaultWidth
        val height = properties.getProperty("height")?.toIntOrNull() ?: defaultHeight
        val x = properties.getProperty("x")?.toIntOrNull()
        val y = properties.getProperty("y")?.toIntOrNull()

        val position = if (x != null && y != null) {
            WindowPosition(x.dp, y.dp)
        } else {
            WindowPosition.PlatformDefault
        }

        return WindowState(
            size = DpSize(width.dp, height.dp),
            position = position,
        )
    }

    fun saveWindowState(state: WindowState) {
        val properties = Properties().apply {
            setProperty("width", state.size.width.value.toInt().toString())
            setProperty("height", state.size.height.value.toInt().toString())
            val pos = state.position
            if (pos is WindowPosition.Absolute) {
                setProperty("x", pos.x.value.toInt().toString())
                setProperty("y", pos.y.value.toInt().toString())
            }
        }

        configFile.parentFile?.mkdirs()
        configFile.outputStream().use { output ->
            properties.store(output, "Tasks Desktop Window State")
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
}
