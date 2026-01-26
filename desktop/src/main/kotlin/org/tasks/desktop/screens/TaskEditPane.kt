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
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.InputChip
import androidx.compose.material3.InputChipDefaults
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
import org.tasks.data.entity.CaldavTask
import org.tasks.data.entity.TagData
import org.tasks.data.entity.Task
import org.tasks.desktop.DesktopApplication
import org.tasks.kmp.formatDate
import org.tasks.kmp.org.tasks.time.DateStyle
import org.tasks.time.DateTimeUtils2.currentTimeMillis
import java.time.Instant
import java.time.LocalDate
import java.time.LocalDateTime
import java.time.LocalTime
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
    var showCalendarDialog by remember { mutableStateOf(false) }
    var showTimePicker by remember { mutableStateOf(false) }

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

            // Update list assignment (CaldavTask)
            val existingCaldavTask = application.caldavDao.getTask(savedTaskId)
            when {
                selectedListId != null && existingCaldavTask == null -> {
                    // Create new CaldavTask
                    application.caldavDao.insert(
                        CaldavTask(
                            task = savedTaskId,
                            calendar = selectedListId,
                        )
                    )
                }
                selectedListId != null && existingCaldavTask != null && existingCaldavTask.calendar != selectedListId -> {
                    // Update existing CaldavTask
                    application.caldavDao.update(existingCaldavTask.copy(calendar = selectedListId))
                }
                selectedListId == null && existingCaldavTask != null -> {
                    // Delete CaldavTask (remove from list)
                    application.caldavDao.delete(existingCaldavTask)
                }
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
                application.refreshTasks()
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
                application.refreshTasks()
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

                // Date button
                Box {
                    TextButton(onClick = { showDatePicker = !showDatePicker }) {
                        Text(
                            text = if (dueDate > 0) {
                                val dateTime = LocalDateTime.ofInstant(
                                    Instant.ofEpochMilli(dueDate),
                                    ZoneId.systemDefault()
                                )
                                dateTime.toLocalDate().toString()
                            } else "Set date",
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
                                val currentTime = if (dueDate > 0) {
                                    LocalDateTime.ofInstant(Instant.ofEpochMilli(dueDate), ZoneId.systemDefault()).toLocalTime()
                                } else LocalTime.MIDNIGHT
                                dueDate = LocalDate.now()
                                    .atTime(currentTime)
                                    .atZone(ZoneId.systemDefault())
                                    .toInstant()
                                    .toEpochMilli()
                                showDatePicker = false
                            }
                        )
                        DropdownMenuItem(
                            text = { Text("Tomorrow") },
                            onClick = {
                                val currentTime = if (dueDate > 0) {
                                    LocalDateTime.ofInstant(Instant.ofEpochMilli(dueDate), ZoneId.systemDefault()).toLocalTime()
                                } else LocalTime.MIDNIGHT
                                dueDate = LocalDate.now().plusDays(1)
                                    .atTime(currentTime)
                                    .atZone(ZoneId.systemDefault())
                                    .toInstant()
                                    .toEpochMilli()
                                showDatePicker = false
                            }
                        )
                        DropdownMenuItem(
                            text = { Text("Next week") },
                            onClick = {
                                val currentTime = if (dueDate > 0) {
                                    LocalDateTime.ofInstant(Instant.ofEpochMilli(dueDate), ZoneId.systemDefault()).toLocalTime()
                                } else LocalTime.MIDNIGHT
                                dueDate = LocalDate.now().plusWeeks(1)
                                    .atTime(currentTime)
                                    .atZone(ZoneId.systemDefault())
                                    .toInstant()
                                    .toEpochMilli()
                                showDatePicker = false
                            }
                        )
                        HorizontalDivider()
                        DropdownMenuItem(
                            text = { Text("Pick a date...") },
                            onClick = {
                                showDatePicker = false
                                showCalendarDialog = true
                            }
                        )
                        if (dueDate > 0) {
                            HorizontalDivider()
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

                // Time button (only show if date is set)
                if (dueDate > 0) {
                    Spacer(modifier = Modifier.width(8.dp))
                    val currentDateTime = LocalDateTime.ofInstant(
                        Instant.ofEpochMilli(dueDate),
                        ZoneId.systemDefault()
                    )
                    val hasTime = currentDateTime.toLocalTime() != LocalTime.MIDNIGHT
                    TextButton(onClick = { showTimePicker = true }) {
                        Text(
                            text = if (hasTime) {
                                String.format("%02d:%02d", currentDateTime.hour, currentDateTime.minute)
                            } else "Set time",
                            color = if (hasTime) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.primary,
                        )
                    }
                }
            }

            // Date Picker Dialog (custom implementation for desktop compatibility)
            if (showCalendarDialog) {
                val initialDateTime = if (dueDate > 0) {
                    LocalDateTime.ofInstant(Instant.ofEpochMilli(dueDate), ZoneId.systemDefault())
                } else LocalDateTime.now()

                val currentYear = LocalDate.now().year
                val years = (currentYear - 2..currentYear + 5).toList()
                val months = listOf(
                    1 to "January", 2 to "February", 3 to "March", 4 to "April",
                    5 to "May", 6 to "June", 7 to "July", 8 to "August",
                    9 to "September", 10 to "October", 11 to "November", 12 to "December"
                )

                var selectedYear by remember { mutableStateOf(initialDateTime.year) }
                var selectedMonth by remember { mutableStateOf(initialDateTime.monthValue) }
                var selectedDay by remember { mutableStateOf(initialDateTime.dayOfMonth) }

                var showYearDropdown by remember { mutableStateOf(false) }
                var showMonthDropdown by remember { mutableStateOf(false) }
                var showDayDropdown by remember { mutableStateOf(false) }

                // Calculate days in selected month
                val daysInMonth = try {
                    LocalDate.of(selectedYear, selectedMonth, 1).lengthOfMonth()
                } catch (e: Exception) { 31 }
                val days = (1..daysInMonth).toList()

                // Adjust day if it exceeds days in month
                if (selectedDay > daysInMonth) {
                    selectedDay = daysInMonth
                }

                androidx.compose.material3.AlertDialog(
                    onDismissRequest = { showCalendarDialog = false },
                    title = { Text("Select date") },
                    text = {
                        Column {
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                // Year dropdown
                                ExposedDropdownMenuBox(
                                    expanded = showYearDropdown,
                                    onExpandedChange = { showYearDropdown = it },
                                    modifier = Modifier.weight(1f)
                                ) {
                                    OutlinedTextField(
                                        value = selectedYear.toString(),
                                        onValueChange = {},
                                        label = { Text("Year") },
                                        readOnly = true,
                                        singleLine = true,
                                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = showYearDropdown) },
                                        modifier = Modifier.menuAnchor().fillMaxWidth()
                                    )
                                    ExposedDropdownMenu(
                                        expanded = showYearDropdown,
                                        onDismissRequest = { showYearDropdown = false }
                                    ) {
                                        years.forEach { year ->
                                            DropdownMenuItem(
                                                text = { Text(year.toString()) },
                                                onClick = {
                                                    selectedYear = year
                                                    showYearDropdown = false
                                                }
                                            )
                                        }
                                    }
                                }

                                // Month dropdown
                                ExposedDropdownMenuBox(
                                    expanded = showMonthDropdown,
                                    onExpandedChange = { showMonthDropdown = it },
                                    modifier = Modifier.weight(1.5f)
                                ) {
                                    OutlinedTextField(
                                        value = months.find { it.first == selectedMonth }?.second ?: "",
                                        onValueChange = {},
                                        label = { Text("Month") },
                                        readOnly = true,
                                        singleLine = true,
                                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = showMonthDropdown) },
                                        modifier = Modifier.menuAnchor().fillMaxWidth()
                                    )
                                    ExposedDropdownMenu(
                                        expanded = showMonthDropdown,
                                        onDismissRequest = { showMonthDropdown = false }
                                    ) {
                                        months.forEach { (num, name) ->
                                            DropdownMenuItem(
                                                text = { Text(name) },
                                                onClick = {
                                                    selectedMonth = num
                                                    showMonthDropdown = false
                                                }
                                            )
                                        }
                                    }
                                }

                                // Day dropdown
                                ExposedDropdownMenuBox(
                                    expanded = showDayDropdown,
                                    onExpandedChange = { showDayDropdown = it },
                                    modifier = Modifier.weight(0.8f)
                                ) {
                                    OutlinedTextField(
                                        value = selectedDay.toString(),
                                        onValueChange = {},
                                        label = { Text("Day") },
                                        readOnly = true,
                                        singleLine = true,
                                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = showDayDropdown) },
                                        modifier = Modifier.menuAnchor().fillMaxWidth()
                                    )
                                    ExposedDropdownMenu(
                                        expanded = showDayDropdown,
                                        onDismissRequest = { showDayDropdown = false }
                                    ) {
                                        days.forEach { day ->
                                            DropdownMenuItem(
                                                text = { Text(day.toString()) },
                                                onClick = {
                                                    selectedDay = day
                                                    showDayDropdown = false
                                                }
                                            )
                                        }
                                    }
                                }
                            }
                        }
                    },
                    confirmButton = {
                        TextButton(
                            onClick = {
                                val selectedDate = LocalDate.of(selectedYear, selectedMonth, selectedDay)
                                val existingTime = if (dueDate > 0) {
                                    LocalDateTime.ofInstant(Instant.ofEpochMilli(dueDate), ZoneId.systemDefault()).toLocalTime()
                                } else LocalTime.MIDNIGHT

                                dueDate = selectedDate
                                    .atTime(existingTime)
                                    .atZone(ZoneId.systemDefault())
                                    .toInstant()
                                    .toEpochMilli()
                                showCalendarDialog = false
                            }
                        ) {
                            Text("OK")
                        }
                    },
                    dismissButton = {
                        TextButton(onClick = { showCalendarDialog = false }) {
                            Text("Cancel")
                        }
                    }
                )
            }

            // Time Picker Dialog (custom implementation for desktop compatibility)
            if (showTimePicker) {
                val currentDateTime = if (dueDate > 0) {
                    LocalDateTime.ofInstant(Instant.ofEpochMilli(dueDate), ZoneId.systemDefault())
                } else LocalDateTime.now()

                val hours = (0..23).toList()
                val minutes = (0..59 step 5).toList() // 5-minute increments for easier selection

                var selectedHour by remember { mutableStateOf(currentDateTime.hour) }
                var selectedMinute by remember { mutableStateOf((currentDateTime.minute / 5) * 5) } // Round to nearest 5

                var showHourDropdown by remember { mutableStateOf(false) }
                var showMinuteDropdown by remember { mutableStateOf(false) }

                val existingHasTime = if (dueDate > 0) {
                    LocalDateTime.ofInstant(Instant.ofEpochMilli(dueDate), ZoneId.systemDefault()).toLocalTime() != LocalTime.MIDNIGHT
                } else false

                androidx.compose.material3.AlertDialog(
                    onDismissRequest = { showTimePicker = false },
                    title = { Text("Select time") },
                    text = {
                        Column {
                            Row(
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                                verticalAlignment = Alignment.CenterVertically,
                                modifier = Modifier.fillMaxWidth()
                            ) {
                                // Hour dropdown
                                ExposedDropdownMenuBox(
                                    expanded = showHourDropdown,
                                    onExpandedChange = { showHourDropdown = it },
                                    modifier = Modifier.weight(1f)
                                ) {
                                    OutlinedTextField(
                                        value = String.format("%02d", selectedHour),
                                        onValueChange = {},
                                        label = { Text("Hour") },
                                        readOnly = true,
                                        singleLine = true,
                                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = showHourDropdown) },
                                        modifier = Modifier.menuAnchor().fillMaxWidth()
                                    )
                                    ExposedDropdownMenu(
                                        expanded = showHourDropdown,
                                        onDismissRequest = { showHourDropdown = false }
                                    ) {
                                        hours.forEach { hour ->
                                            DropdownMenuItem(
                                                text = { Text(String.format("%02d", hour)) },
                                                onClick = {
                                                    selectedHour = hour
                                                    showHourDropdown = false
                                                }
                                            )
                                        }
                                    }
                                }

                                Text(":", style = MaterialTheme.typography.headlineMedium)

                                // Minute dropdown
                                ExposedDropdownMenuBox(
                                    expanded = showMinuteDropdown,
                                    onExpandedChange = { showMinuteDropdown = it },
                                    modifier = Modifier.weight(1f)
                                ) {
                                    OutlinedTextField(
                                        value = String.format("%02d", selectedMinute),
                                        onValueChange = {},
                                        label = { Text("Minute") },
                                        readOnly = true,
                                        singleLine = true,
                                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = showMinuteDropdown) },
                                        modifier = Modifier.menuAnchor().fillMaxWidth()
                                    )
                                    ExposedDropdownMenu(
                                        expanded = showMinuteDropdown,
                                        onDismissRequest = { showMinuteDropdown = false }
                                    ) {
                                        minutes.forEach { minute ->
                                            DropdownMenuItem(
                                                text = { Text(String.format("%02d", minute)) },
                                                onClick = {
                                                    selectedMinute = minute
                                                    showMinuteDropdown = false
                                                }
                                            )
                                        }
                                    }
                                }
                            }
                            Text(
                                text = "24-hour format",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                modifier = Modifier.padding(top = 4.dp)
                            )
                        }
                    },
                    confirmButton = {
                        TextButton(
                            onClick = {
                                val currentDate = if (dueDate > 0) {
                                    LocalDateTime.ofInstant(Instant.ofEpochMilli(dueDate), ZoneId.systemDefault()).toLocalDate()
                                } else LocalDate.now()

                                dueDate = currentDate
                                    .atTime(selectedHour, selectedMinute)
                                    .atZone(ZoneId.systemDefault())
                                    .toInstant()
                                    .toEpochMilli()
                                showTimePicker = false
                            }
                        ) {
                            Text("OK")
                        }
                    },
                    dismissButton = {
                        Row {
                            if (existingHasTime) {
                                TextButton(
                                    onClick = {
                                        // Clear the time, keep only the date
                                        val currentDate = LocalDateTime.ofInstant(
                                            Instant.ofEpochMilli(dueDate),
                                            ZoneId.systemDefault()
                                        ).toLocalDate()
                                        dueDate = currentDate
                                            .atStartOfDay(ZoneId.systemDefault())
                                            .toInstant()
                                            .toEpochMilli()
                                        showTimePicker = false
                                    }
                                ) {
                                    Text("Clear time", color = MaterialTheme.colorScheme.error)
                                }
                            }
                            TextButton(onClick = { showTimePicker = false }) {
                                Text("Cancel")
                            }
                        }
                    }
                )
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
                            val hasColor = tag.color != null && tag.color != 0
                            val tagColor = if (hasColor) Color(tag.color!!) else MaterialTheme.colorScheme.secondaryContainer
                            val contentColor = if (hasColor) Color.White else MaterialTheme.colorScheme.onSecondaryContainer

                            InputChip(
                                selected = true,
                                onClick = {
                                    selectedTags = selectedTags - tag
                                },
                                label = { Text(tag.name ?: "", color = contentColor) },
                                trailingIcon = {
                                    Icon(
                                        imageVector = Icons.Default.Close,
                                        contentDescription = "Remove",
                                        modifier = Modifier.size(16.dp),
                                        tint = contentColor,
                                    )
                                },
                                colors = InputChipDefaults.inputChipColors(
                                    selectedContainerColor = tagColor,
                                    selectedLabelColor = contentColor,
                                    selectedTrailingIconColor = contentColor,
                                )
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
                                            text = {
                                                Row(
                                                    verticalAlignment = Alignment.CenterVertically,
                                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                                ) {
                                                    if (tag.color != null && tag.color != 0) {
                                                        Box(
                                                            modifier = Modifier
                                                                .size(12.dp)
                                                                .clip(CircleShape)
                                                                .background(Color(tag.color!!))
                                                        )
                                                    }
                                                    Text(tag.name ?: "")
                                                }
                                            },
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
