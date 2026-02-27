package org.tasks.desktop.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import org.tasks.desktop.DesktopApplication
import org.tasks.desktop.navigation.Screen
import org.tasks.filters.Filter

@Composable
fun MainScreen(
    application: DesktopApplication,
    onNewTask: (Filter?) -> Unit,
    onEditTask: (Long) -> Unit,
) {
    val currentScreen by application.navigator.currentScreen.collectAsState()

    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background
    ) {
        Row(modifier = Modifier.fillMaxSize()) {
            // Sidebar (Navigation Drawer)
            Column(
                modifier = Modifier
                    .width(280.dp)
                    .fillMaxHeight()
                    .background(MaterialTheme.colorScheme.surface)
            ) {
                Box(modifier = Modifier.weight(1f)) {
                    SidebarPane(application = application)
                }

                // Settings button at bottom of sidebar
                HorizontalDivider()
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { application.navigator.navigateToSettings() }
                        .padding(16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        imageVector = Icons.Default.Settings,
                        contentDescription = "Settings",
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(modifier = Modifier.width(16.dp))
                    Text(
                        text = "Settings",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurface,
                    )
                }
            }

            VerticalDivider()

            // Task List Pane
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
            ) {
                TaskListPane(
                    application = application,
                    onTaskClick = onEditTask,
                    onNewTask = { onNewTask(application.currentFilter) },
                )
            }

            // Detail Pane (shown when editing a task)
            when (val screen = currentScreen) {
                is Screen.TaskEdit -> {
                    VerticalDivider()
                    Box(
                        modifier = Modifier
                            .width(400.dp)
                            .fillMaxHeight()
                            .background(MaterialTheme.colorScheme.surface)
                    ) {
                        TaskEditPane(
                            taskId = screen.taskId,
                            application = application,
                            onClose = { application.navigator.goBack() },
                            initialFilter = screen.filter,
                        )
                    }
                }
                else -> {}
            }
        }
    }
}
