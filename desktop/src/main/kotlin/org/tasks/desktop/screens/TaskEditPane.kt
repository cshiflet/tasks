package org.tasks.desktop.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.CalendarMonth
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Flag
import androidx.compose.material.icons.filled.List
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Tag
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.InputChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
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
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.tasks.data.entity.CaldavCalendar
import org.tasks.data.entity.TagData
import org.tasks.data.entity.Task
import org.tasks.desktop.DesktopApplication
import org.tasks.kmp.formatDate
import org.tasks.kmp.org.tasks.time.DateStyle
import org.tasks.time.DateTimeUtils2.currentTimeMillis
import java.time.LocalDate
import java.time.ZoneId

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun TaskEditPane(
    taskId: Long?,
    application: DesktopApplication,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()
    var task by remember { mutableStateOf<Task?>(null) }
    var title by remember { mutableStateOf("") }
    var notes by remember { mutableStateOf("") }
    var priority by remember { mutableStateOf(3) } // 0=High, 1=Med, 2=Low, 3=None
    var dueDate by remember { mutableStateOf(0L) }
    var selectedListId by remember { mutableStateOf<String?>(null) }
    var selectedTags by remember { mutableStateOf<List<TagData>>(emptyList()) }
    var isNew by remember { mutableStateOf(taskId == null) }

    // Available lists and tags
    var availableLists by remember { mutableStateOf<List<CaldavCalendar>>(emptyList()) }
    var availableTags by remember { mutableStateOf<List<TagData>>(emptyList()) }

    // Dropdowns
    var showPriorityMenu by remember { mutableStateOf(false) }
    var showListMenu by remember { mutableStateOf(false) }
    var showTagMenu by remember { mutableStateOf(false) }
    var showDatePicker by remember { mutableStateOf(false) }

    LaunchedEffect(taskId) {
        withContext(Dispatchers.IO) {
            // Load available lists and tags
            availableLists = application.caldavDao.getCalendars()
            availableTags = application.tagDataDao.getAll()

            if (taskId != null) {
                val loadedTask = application.taskDao.fetch(taskId)
                task = loadedTask
                title = loadedTask?.title ?: ""
                notes = loadedTask?.notes ?: ""
                priority = loadedTask?.priority ?: 3
                dueDate = loadedTask?.dueDate ?: 0L
                isNew = false

                // Load task's current list
                val caldavTask = application.caldavDao.getTask(taskId)
                selectedListId = caldavTask?.calendar

                // Load task's tags
                selectedTags = application.tagDataDao.getTagDataForTask(taskId)
            } else {
                task = Task()
                title = ""
                notes = ""
                priority = 3
                dueDate = 0L
                selectedListId = null
                selectedTags = emptyList()
                isNew = true
            }
        }
    }

    fun saveTask() {
        scope.launch(Dispatchers.IO) {
            val currentTask = task ?: Task()
            val updatedTask = currentTask.copy(
                title = title.ifBlank { null },
                notes = notes.ifBlank { null },
                priority = priority,
                dueDate = dueDate,
                modificationDate = currentTimeMillis(),
            )

            val savedTaskId = if (isNew) {
                val newTask = updatedTask.copy(
                    creationDate = currentTimeMillis(),
                )
                application.taskDao.insert(newTask)
            } else {
                application.taskDao.update(updatedTask)
                updatedTask.id
            }

            // Update tags using the proper DAO method
            val existingTags = application.tagDao.getTagsForTask(savedTaskId)
            val toRemove = existingTags.filter { existing ->
                selectedTags.none { it.remoteId == existing.tagUid }
            }
            if (toRemove.isNotEmpty()) {
                application.tagDao.delete(toRemove)
            }
            val existingTagUids = existingTags.map { it.tagUid }.toSet()
            val toAdd = selectedTags.filter { it.remoteId !in existingTagUids }
            toAdd.forEach { tag ->
                application.tagDao.insert(
                    org.tasks.data.entity.Tag(
                        task = savedTaskId,
                        tagUid = tag.remoteId,
                        name = tag.name,
                    )
                )
            }

            withContext(Dispatchers.Main) {
                onClose()
            }
        }
    }

    fun deleteTask() {
        scope.launch(Dispatchers.IO) {
            task?.let {
                application.deletionDao.markDeleted(
                    ids = listOf(it.id),
                    cleanup = { /* No cleanup needed for desktop */ }
                )
            }
            withContext(Dispatchers.Main) {
                onClose()
            }
        }
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = if (isNew) "New Task" else "Edit Task",
                        style = MaterialTheme.typography.titleMedium,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onClose) {
                        Icon(
                            imageVector = Icons.Default.Close,
                            contentDescription = "Close",
                        )
                    }
                },
                actions = {
                    if (!isNew) {
                        IconButton(onClick = { deleteTask() }) {
                            Icon(
                                imageVector = Icons.Default.Delete,
                                contentDescription = "Delete",
                                tint = MaterialTheme.colorScheme.error,
                            )
                        }
                    }
                    IconButton(onClick = { saveTask() }) {
                        Icon(
                            imageVector = Icons.Default.Save,
                            contentDescription = "Save",
                            tint = MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            )
        }
    ) { paddingValues ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
                .padding(16.dp)
                .verticalScroll(rememberScrollState())
        ) {
            // Title
            OutlinedTextField(
                value = title,
                onValueChange = { title = it },
                label = { Text("Title") },
                placeholder = { Text("Enter task title") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Priority
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = Icons.Default.Flag,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(20.dp),
                )
                Spacer(modifier = Modifier.width(12.dp))
                Text(
                    text = "Priority",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(modifier = Modifier.weight(1f))
                Box {
                    Row(
                        modifier = Modifier
                            .clip(RoundedCornerShape(8.dp))
                            .clickable { showPriorityMenu = true }
                            .padding(8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        PriorityIndicator(priority)
                        Spacer(modifier = Modifier.width(8.dp))
                        Text(
                            text = getPriorityLabel(priority),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                    DropdownMenu(
                        expanded = showPriorityMenu,
                        onDismissRequest = { showPriorityMenu = false }
                    ) {
                        listOf(0, 1, 2, 3).forEach { p ->
                            DropdownMenuItem(
                                text = {
                                    Row(verticalAlignment = Alignment.CenterVertically) {
                                        PriorityIndicator(p)
                                        Spacer(modifier = Modifier.width(8.dp))
                                        Text(getPriorityLabel(p))
                                    }
                                },
                                onClick = {
                                    priority = p
                                    showPriorityMenu = false
                                }
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))
            HorizontalDivider()
            Spacer(modifier = Modifier.height(12.dp))

            // Due Date
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = Icons.Default.CalendarMonth,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(20.dp),
                )
                Spacer(modifier = Modifier.width(12.dp))
                Text(
                    text = "Due date",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(modifier = Modifier.weight(1f))
                Box {
                    TextButton(onClick = { showDatePicker = !showDatePicker }) {
                        Text(
                            text = if (dueDate > 0) formatDate(dueDate, DateStyle.MEDIUM) else "Set date",
                            color = if (dueDate > 0) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.primary,
                        )
                    }
                    DropdownMenu(
                        expanded = showDatePicker,
                        onDismissRequest = { showDatePicker = false }
                    ) {
                        DropdownMenuItem(
                            text = { Text("Today") },
                            onClick = {
                                dueDate = LocalDate.now()
                                    .atStartOfDay(ZoneId.systemDefault())
                                    .toInstant()
                                    .toEpochMilli()
                                showDatePicker = false
                            }
                        )
                        DropdownMenuItem(
                            text = { Text("Tomorrow") },
                            onClick = {
                                dueDate = LocalDate.now().plusDays(1)
                                    .atStartOfDay(ZoneId.systemDefault())
                                    .toInstant()
                                    .toEpochMilli()
                                showDatePicker = false
                            }
                        )
                        DropdownMenuItem(
                            text = { Text("Next week") },
                            onClick = {
                                dueDate = LocalDate.now().plusWeeks(1)
                                    .atStartOfDay(ZoneId.systemDefault())
                                    .toInstant()
                                    .toEpochMilli()
                                showDatePicker = false
                            }
                        )
                        if (dueDate > 0) {
                            DropdownMenuItem(
                                text = { Text("Clear", color = MaterialTheme.colorScheme.error) },
                                onClick = {
                                    dueDate = 0L
                                    showDatePicker = false
                                }
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))
            HorizontalDivider()
            Spacer(modifier = Modifier.height(12.dp))

            // List
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = Icons.Default.List,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(20.dp),
                )
                Spacer(modifier = Modifier.width(12.dp))
                Text(
                    text = "List",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(modifier = Modifier.weight(1f))
                Box {
                    TextButton(onClick = { showListMenu = true }) {
                        Text(
                            text = availableLists.find { it.uuid == selectedListId }?.name ?: "No list",
                            color = MaterialTheme.colorScheme.onSurface,
                        )
                    }
                    DropdownMenu(
                        expanded = showListMenu,
                        onDismissRequest = { showListMenu = false }
                    ) {
                        DropdownMenuItem(
                            text = { Text("No list") },
                            onClick = {
                                selectedListId = null
                                showListMenu = false
                            }
                        )
                        availableLists.forEach { list ->
                            DropdownMenuItem(
                                text = { Text(list.name ?: "Unnamed") },
                                onClick = {
                                    selectedListId = list.uuid
                                    showListMenu = false
                                }
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(12.dp))
            HorizontalDivider()
            Spacer(modifier = Modifier.height(12.dp))

            // Tags
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.Top,
            ) {
                Icon(
                    imageVector = Icons.Default.Tag,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.size(20.dp).padding(top = 4.dp),
                )
                Spacer(modifier = Modifier.width(12.dp))
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        text = "Tags",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        selectedTags.forEach { tag ->
                            InputChip(
                                selected = true,
                                onClick = {
                                    selectedTags = selectedTags - tag
                                },
                                label = { Text(tag.name ?: "") },
                                trailingIcon = {
                                    Icon(
                                        imageVector = Icons.Default.Close,
                                        contentDescription = "Remove",
                                        modifier = Modifier.size(16.dp),
                                    )
                                }
                            )
                        }
                        Box {
                            AssistChip(
                                onClick = { showTagMenu = true },
                                label = { Text("Add tag") },
                                leadingIcon = {
                                    Icon(
                                        imageVector = Icons.Default.Add,
                                        contentDescription = null,
                                        modifier = Modifier.size(16.dp),
                                    )
                                }
                            )
                            DropdownMenu(
                                expanded = showTagMenu,
                                onDismissRequest = { showTagMenu = false }
                            ) {
                                val unselectedTags = availableTags.filter { tag ->
                                    selectedTags.none { it.remoteId == tag.remoteId }
                                }
                                if (unselectedTags.isEmpty()) {
                                    DropdownMenuItem(
                                        text = { Text("No more tags available") },
                                        onClick = { showTagMenu = false },
                                        enabled = false,
                                    )
                                } else {
                                    unselectedTags.forEach { tag ->
                                        DropdownMenuItem(
                                            text = { Text(tag.name ?: "") },
                                            onClick = {
                                                selectedTags = selectedTags + tag
                                                showTagMenu = false
                                            }
                                        )
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))
            HorizontalDivider()
            Spacer(modifier = Modifier.height(16.dp))

            // Notes
            OutlinedTextField(
                value = notes,
                onValueChange = { notes = it },
                label = { Text("Notes") },
                placeholder = { Text("Add notes...") },
                modifier = Modifier.fillMaxWidth().height(150.dp),
                maxLines = 8,
            )
        }
    }
}

@Composable
private fun PriorityIndicator(priority: Int) {
    val color = when (priority) {
        0 -> Color(0xFFD50000) // High - Red
        1 -> Color(0xFFFF6D00) // Medium - Orange
        2 -> Color(0xFF2962FF) // Low - Blue
        else -> Color.Transparent // None
    }
    if (priority < 3) {
        Box(
            modifier = Modifier
                .size(12.dp)
                .clip(CircleShape)
                .background(color)
        )
    } else {
        Box(
            modifier = Modifier
                .size(12.dp)
                .clip(CircleShape)
                .border(1.dp, MaterialTheme.colorScheme.outline, CircleShape)
        )
    }
}

private fun getPriorityLabel(priority: Int): String {
    return when (priority) {
        0 -> "High"
        1 -> "Medium"
        2 -> "Low"
        else -> "None"
    }
}
