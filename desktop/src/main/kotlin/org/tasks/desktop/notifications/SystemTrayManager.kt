package org.tasks.desktop.notifications

import java.awt.AWTException
import java.awt.Image
import java.awt.MenuItem
import java.awt.PopupMenu
import java.awt.SystemTray
import java.awt.Toolkit
import java.awt.TrayIcon
import java.awt.TrayIcon.MessageType
import javax.imageio.ImageIO

class SystemTrayManager(
    private val onShowWindow: () -> Unit,
    private val onNewTask: () -> Unit,
    private val onSync: () -> Unit,
    private val onSettings: () -> Unit,
    private val onExit: () -> Unit,
) {
    private var trayIcon: TrayIcon? = null

    fun initialize(): Boolean {
        if (!SystemTray.isSupported()) {
            return false
        }

        try {
            val tray = SystemTray.getSystemTray()
            val image = loadTrayIcon()

            val popup = PopupMenu().apply {
                add(MenuItem("Show Tasks").apply {
                    addActionListener { onShowWindow() }
                })
                addSeparator()
                add(MenuItem("New Task").apply {
                    addActionListener { onNewTask() }
                })
                add(MenuItem("Sync Now").apply {
                    addActionListener { onSync() }
                })
                addSeparator()
                add(MenuItem("Settings").apply {
                    addActionListener { onSettings() }
                })
                addSeparator()
                add(MenuItem("Exit").apply {
                    addActionListener { onExit() }
                })
            }

            trayIcon = TrayIcon(image, "Tasks", popup).apply {
                isImageAutoSize = true
                addActionListener { onShowWindow() } // Double-click to show
            }

            tray.add(trayIcon)
            return true
        } catch (e: AWTException) {
            e.printStackTrace()
            return false
        }
    }

    fun showNotification(title: String, message: String, type: NotificationType = NotificationType.INFO) {
        trayIcon?.displayMessage(
            title,
            message,
            when (type) {
                NotificationType.INFO -> MessageType.INFO
                NotificationType.WARNING -> MessageType.WARNING
                NotificationType.ERROR -> MessageType.ERROR
                NotificationType.REMINDER -> MessageType.INFO
            }
        )
    }

    fun showTaskReminder(taskTitle: String, dueTime: String?) {
        val message = if (dueTime != null) {
            "Due: $dueTime"
        } else {
            "Task reminder"
        }
        showNotification(taskTitle, message, NotificationType.REMINDER)
    }

    fun updateBadgeCount(count: Int) {
        // Update tooltip with task count
        trayIcon?.toolTip = if (count > 0) {
            "Tasks ($count pending)"
        } else {
            "Tasks"
        }
    }

    fun remove() {
        trayIcon?.let { icon ->
            if (SystemTray.isSupported()) {
                SystemTray.getSystemTray().remove(icon)
            }
        }
        trayIcon = null
    }

    private fun loadTrayIcon(): Image {
        // Try to load icon from resources
        val iconStream = javaClass.getResourceAsStream("/icon.png")
        return if (iconStream != null) {
            ImageIO.read(iconStream)
        } else {
            // Create a simple fallback icon
            createFallbackIcon()
        }
    }

    private fun createFallbackIcon(): Image {
        // Create a simple 16x16 icon as fallback
        val size = 16
        val image = java.awt.image.BufferedImage(size, size, java.awt.image.BufferedImage.TYPE_INT_ARGB)
        val g = image.createGraphics()

        // Draw a simple checkmark icon
        g.color = java.awt.Color(0x2196F3) // Material Blue
        g.fillOval(0, 0, size, size)

        g.color = java.awt.Color.WHITE
        g.stroke = java.awt.BasicStroke(2f)
        g.drawLine(4, 8, 7, 11)
        g.drawLine(7, 11, 12, 5)

        g.dispose()
        return image
    }

    enum class NotificationType {
        INFO,
        WARNING,
        ERROR,
        REMINDER
    }

    companion object {
        fun isSupported(): Boolean = SystemTray.isSupported()
    }
}
