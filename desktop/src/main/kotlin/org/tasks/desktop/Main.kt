package org.tasks.desktop

import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyShortcut
import androidx.compose.ui.window.MenuBar
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import org.tasks.desktop.di.DesktopContainer
import org.tasks.desktop.navigation.Screen
import org.tasks.desktop.notifications.ReminderScheduler
import org.tasks.desktop.notifications.SystemTrayManager
import org.tasks.desktop.screens.AccountSetupScreen
import org.tasks.desktop.screens.MainScreen
import org.tasks.desktop.screens.SettingsScreen
import org.tasks.desktop.sync.DesktopSyncManager
import org.tasks.themes.TasksTheme

fun main() = application {
    val container = remember { DesktopContainer.getInstance() }
    val app = remember { DesktopApplication(container) }
    var isVisible by remember { mutableStateOf(true) }
    val windowStateManager = remember { container.windowStateManager }
    val windowState = remember { windowStateManager.loadWindowState() }
    val themeManager = remember { container.themeManager }

    // Initialize sync manager
    val syncManager = remember {
        DesktopSyncManager.getInstance(container.caldavDao, container.taskDao).apply {
            startAutoSync()
        }
    }

    // Initialize system tray
    val systemTray = remember {
        if (SystemTrayManager.isSupported()) {
            SystemTrayManager(
                onShowWindow = { isVisible = true },
                onNewTask = {
                    isVisible = true
                    app.navigator.navigate(Screen.TaskEdit(taskId = null))
                },
                onSync = { syncManager.syncNow() },
                onSettings = {
                    isVisible = true
                    app.navigator.navigateToSettings()
                },
                onExit = { exitApplication() }
            ).also { it.initialize() }
        } else null
    }

    // Initialize reminder scheduler
    val reminderScheduler = remember {
        ReminderScheduler(
            alarmDao = container.alarmDao,
            taskDao = container.taskDao,
            systemTrayManager = systemTray,
        ).also { it.start() }
    }

    // Cleanup on exit
    DisposableEffect(Unit) {
        onDispose {
            windowStateManager.saveWindowState(windowState)
            syncManager.stopAutoSync()
            reminderScheduler.stop()
            systemTray?.remove()
        }
    }

    if (isVisible) {
        Window(
            onCloseRequest = {
                // Save window state before closing
                windowStateManager.saveWindowState(windowState)
                if (systemTray != null) {
                    // Minimize to tray instead of exiting
                    isVisible = false
                } else {
                    exitApplication()
                }
            },
            title = "Tasks",
            state = windowState,
        ) {
            // Menu bar with keyboard shortcuts
            MenuBar {
                Menu("File") {
                    Item(
                        "New Task",
                        shortcut = KeyShortcut(Key.N, ctrl = true),
                        onClick = {
                            app.navigator.navigate(Screen.TaskEdit(taskId = null))
                        }
                    )
                    Separator()
                    Item(
                        "Sync Now",
                        shortcut = KeyShortcut(Key.R, ctrl = true),
                        onClick = { syncManager.syncNow() }
                    )
                    Separator()
                    Item(
                        "Settings",
                        shortcut = KeyShortcut(Key.Comma, ctrl = true),
                        onClick = { app.navigator.navigateToSettings() }
                    )
                    Separator()
                    Item(
                        "Exit",
                        shortcut = KeyShortcut(Key.Q, ctrl = true),
                        onClick = { exitApplication() }
                    )
                }
                Menu("Edit") {
                    Item(
                        "Undo",
                        shortcut = KeyShortcut(Key.Z, ctrl = true),
                        onClick = { /* TODO: implement undo */ }
                    )
                    Item(
                        "Redo",
                        shortcut = KeyShortcut(Key.Z, ctrl = true, shift = true),
                        onClick = { /* TODO: implement redo */ }
                    )
                }
                Menu("View") {
                    Item(
                        "My Tasks",
                        shortcut = KeyShortcut(Key.One, ctrl = true),
                        onClick = { app.navigator.navigateToTaskList() }
                    )
                }
                Menu("Help") {
                    Item(
                        "Documentation",
                        onClick = {
                            java.awt.Desktop.getDesktop().browse(java.net.URI("https://tasks.org/docs"))
                        }
                    )
                    Item(
                        "Report Issue",
                        onClick = {
                            java.awt.Desktop.getDesktop().browse(java.net.URI("https://github.com/tasks/tasks/issues"))
                        }
                    )
                    Separator()
                    Item(
                        "About Tasks",
                        onClick = { /* TODO: show about dialog */ }
                    )
                }
            }

            val currentScreen by app.navigator.currentScreen.collectAsState()

            // Theme: 0=light, 1=black, 2=dark, 5=system
            val themeValue = when (themeManager.themeMode) {
                org.tasks.desktop.platform.ThemeManager.ThemeMode.LIGHT -> 0
                org.tasks.desktop.platform.ThemeManager.ThemeMode.DARK -> 2
                org.tasks.desktop.platform.ThemeManager.ThemeMode.SYSTEM -> 5
            }
            TasksTheme(theme = themeValue) {
                when (currentScreen) {
                    is Screen.TaskList, is Screen.TaskEdit -> {
                        MainScreen(
                            application = app,
                            onNewTask = { filter ->
                                app.navigator.navigate(Screen.TaskEdit(taskId = null, filter = filter))
                            },
                            onEditTask = { taskId ->
                                app.navigator.navigate(Screen.TaskEdit(taskId = taskId))
                            },
                        )
                    }
                    is Screen.Settings -> {
                        SettingsScreen(
                            application = app,
                            onNavigateBack = { app.navigator.goBack() },
                            onAddAccount = { app.navigator.navigateToAccountSetup() },
                            onEditAccount = { accountId -> app.navigator.navigateToAccountEdit(accountId) },
                        )
                    }
                    is Screen.AccountSetup -> {
                        AccountSetupScreen(
                            application = app,
                            onNavigateBack = { app.navigator.goBack() },
                            onAccountCreated = {
                                app.loadFilters()
                                app.navigator.navigateToTaskList()
                            },
                        )
                    }
                    is Screen.AccountEdit -> {
                        SettingsScreen(
                            application = app,
                            onNavigateBack = { app.navigator.goBack() },
                            onAddAccount = { app.navigator.navigateToAccountSetup() },
                            onEditAccount = { accountId -> app.navigator.navigateToAccountEdit(accountId) },
                        )
                    }
                    is Screen.ListEdit, is Screen.TagEdit, is Screen.FilterEdit -> {
                        MainScreen(
                            application = app,
                            onNewTask = { filter ->
                                app.navigator.navigate(Screen.TaskEdit(taskId = null, filter = filter))
                            },
                            onEditTask = { taskId ->
                                app.navigator.navigate(Screen.TaskEdit(taskId = taskId))
                            },
                        )
                    }
                }
            }
        }
    }
}
