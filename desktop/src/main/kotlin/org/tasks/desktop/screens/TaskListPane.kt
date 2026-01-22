package org.tasks.desktop.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CheckBox
import androidx.compose.material.icons.filled.CheckBoxOutlineBlank
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.todoroo.astrid.core.SortHelper
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.tasks.data.TaskContainer
import org.tasks.data.TaskListQuery
import org.tasks.desktop.DesktopApplication
import org.tasks.kmp.formatDate
import org.tasks.kmp.org.tasks.time.DateStyle
import org.tasks.preferences.QueryPreferences
import org.tasks.tasklist.SectionedDataSource
import org.tasks.tasklist.UiItem
import org.tasks.time.DateTimeUtils2.currentTimeMillis

/**
 * Default QueryPreferences implementation for desktop.
 */
private class DesktopQueryPreferences : QueryPreferences {
    override var sortMode: Int = SortHelper.SORT_DUE
    override var groupMode: Int = SortHelper.GROUP_NONE
    override var completedMode: Int = SortHelper.SORT_COMPLETED
    override var subtaskMode: Int = SortHelper.SORT_MANUAL
    override var isManualSort: Boolean = false
    override var isAstridSort: Boolean = false
    override var sortAscending: Boolean = true
    override var groupAscending: Boolean = true
    override var completedAscending: Boolean = false
    override var subtaskAscending: Boolean = true
    override val showHidden: Boolean = false
    override val showCompleted: Boolean = false
    override val alwaysDisplayFullDate: Boolean = false
    override var completedTasksAtBottom: Boolean = true
}

@Composable
fun TaskListPane(
    application: DesktopApplication,
    onTaskClick: (Long) -> Unit,
    onNewTask: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()
    var tasks by remember { mutableStateOf<SectionedDataSource>(SectionedDataSource()) }
    val currentFilter = application.currentFilter
    val preferences = remember { DesktopQueryPreferences() }

    LaunchedEffect(currentFilter) {
        withContext(Dispatchers.IO) {
            val query = TaskListQuery.getQuery(
                preferences = preferences,
                filter = currentFilter,
            )
            val result = application.taskDao.fetchTasks(query)
            tasks = SectionedDataSource(
                tasks = result,
                disableHeaders = true,
                groupMode = SortHelper.GROUP_NONE,
                subtaskMode = SortHelper.SORT_MANUAL,
                collapsed = emptySet(),
                completedAtBottom = true,
            )
        }
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        floatingActionButton = {
            FloatingActionButton(
                onClick = onNewTask,
                containerColor = MaterialTheme.colorScheme.primary,
            ) {
                Icon(
                    imageVector = Icons.Default.Add,
                    contentDescription = "New Task",
                    tint = MaterialTheme.colorScheme.onPrimary,
                )
            }
        }
    ) { paddingValues ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
        ) {
            // Header with filter name
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp)
            ) {
                Text(
                    text = currentFilter.title ?: "",
                    style = MaterialTheme.typography.headlineSmall,
                    color = MaterialTheme.colorScheme.onSurface,
                )
            }

            // Task list
            if (tasks.isEmpty()) {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        text = "No tasks",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(bottom = 80.dp),
                ) {
                    items(
                        items = tasks.toList(),
                        key = { it.key }
                    ) { item ->
                        when (item) {
                            is UiItem.Header -> {
                                SectionHeader(
                                    value = item.value,
                                    collapsed = item.collapsed,
                                    onToggle = { /* TODO: implement collapse toggle */ }
                                )
                            }
                            is UiItem.Task -> {
                                TaskRow(
                                    task = item.task,
                                    onClick = { onTaskClick(item.task.id) },
                                    onToggleComplete = {
                                        scope.launch(Dispatchers.IO) {
                                            val task = item.task.task
                                            val updatedTask = task.copy(
                                                completionDate = if (task.isCompleted) 0L else currentTimeMillis()
                                            )
                                            application.taskDao.update(updatedTask, task)
                                            // Reload tasks
                                            val query = TaskListQuery.getQuery(
                                                preferences = preferences,
                                                filter = currentFilter,
                                            )
                                            val result = application.taskDao.fetchTasks(query)
                                            tasks = SectionedDataSource(
                                                tasks = result,
                                                disableHeaders = true,
                                                groupMode = SortHelper.GROUP_NONE,
                                                subtaskMode = SortHelper.SORT_MANUAL,
                                                collapsed = emptySet(),
                                                completedAtBottom = true,
                                            )
                                        }
                                    }
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SectionHeader(
    value: Long,
    collapsed: Boolean,
    onToggle: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onToggle)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = if (collapsed) Icons.Default.ExpandMore else Icons.Default.ExpandLess,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(modifier = Modifier.width(8.dp))
        Text(
            text = when (value) {
                SectionedDataSource.HEADER_OVERDUE -> "Overdue"
                SectionedDataSource.HEADER_COMPLETED -> "Completed"
                else -> formatDate(value, DateStyle.FULL)
            },
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun TaskRow(
    task: TaskContainer,
    onClick: () -> Unit,
    onToggleComplete: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Priority indicator
        val priorityColor = when (task.priority) {
            0 -> Color(0xFFD50000) // High
            1 -> Color(0xFFFF6D00) // Medium
            2 -> Color(0xFF2962FF) // Low
            else -> Color.Transparent // None
        }
        if (task.priority < 3) {
            Box(
                modifier = Modifier
                    .size(4.dp)
                    .clip(CircleShape)
                    .background(priorityColor)
            )
            Spacer(modifier = Modifier.width(8.dp))
        }

        // Checkbox
        IconButton(
            onClick = onToggleComplete,
            modifier = Modifier.size(24.dp)
        ) {
            Icon(
                imageVector = if (task.isCompleted) Icons.Default.CheckBox else Icons.Default.CheckBoxOutlineBlank,
                contentDescription = if (task.isCompleted) "Mark incomplete" else "Mark complete",
                tint = if (task.isCompleted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        Spacer(modifier = Modifier.width(12.dp))

        // Task content
        Column(
            modifier = Modifier.weight(1f),
            verticalArrangement = Arrangement.spacedBy(2.dp)
        ) {
            Text(
                text = task.title ?: "",
                style = MaterialTheme.typography.bodyLarge,
                color = if (task.isCompleted)
                    MaterialTheme.colorScheme.onSurfaceVariant
                else
                    MaterialTheme.colorScheme.onSurface,
                textDecoration = if (task.isCompleted) TextDecoration.LineThrough else null,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )

            // Due date
            if (task.hasDueDate()) {
                val isOverdue = task.dueDate < currentTimeMillis() && !task.isCompleted
                Text(
                    text = formatDate(task.dueDate, DateStyle.MEDIUM),
                    style = MaterialTheme.typography.bodySmall,
                    color = if (isOverdue)
                        MaterialTheme.colorScheme.error
                    else
                        MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        // Subtask indicator
        if (task.hasChildren()) {
            Spacer(modifier = Modifier.width(8.dp))
            Text(
                text = "${task.children}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
