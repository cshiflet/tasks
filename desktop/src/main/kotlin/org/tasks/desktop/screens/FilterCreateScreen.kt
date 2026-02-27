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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.tasks.data.NO_ORDER
import org.tasks.data.dao.TaskDao.TaskCriteria.activeAndVisible
import org.tasks.data.entity.Alarm
import org.tasks.data.entity.CaldavTask
import org.tasks.data.entity.Filter
import org.tasks.data.entity.Tag
import org.tasks.data.entity.Task
import org.tasks.data.sql.Criterion.Companion.and
import org.tasks.data.sql.Criterion.Companion.exists
import org.tasks.data.sql.Criterion.Companion.or
import org.tasks.data.sql.Field.Companion.field
import org.tasks.data.sql.Join.Companion.inner
import org.tasks.data.sql.Query.Companion.select
import org.tasks.data.sql.UnaryCriterion
import org.tasks.data.sql.UnaryCriterion.Companion.isNotNull
import com.todoroo.astrid.api.PermaSql
import org.tasks.desktop.DesktopApplication
import org.tasks.filters.SEPARATOR_ESCAPE
import org.tasks.filters.SERIALIZATION_SEPARATOR
import org.tasks.filters.mapToSerializedString
import java.util.UUID

// ────────────────────────────────────────────────────────────────────────────
// Data model
// ────────────────────────────────────────────────────────────────────────────

private const val ID_UNIVERSE = "active"
private const val ID_TITLE = "title"
private const val ID_IMPORTANCE = "importance"
private const val ID_STARTDATE = "startDate"
private const val ID_DUEDATE = "dueDate"
private const val ID_CALDAV = "caldavlist"
private const val ID_TAG_IS = "tag_is"
private const val ID_TAG_CONTAINS = "tag_contains"
private const val ID_RECUR = "recur"
private const val ID_COMPLETED = "completed"
private const val ID_HIDDEN = "hidden"
private const val ID_PARENT = "parent"
private const val ID_SUBTASK = "subtask"
private const val ID_REMINDERS = "reminders"

private const val TYPE_ADD = 0
private const val TYPE_SUBTRACT = 1
private const val TYPE_INTERSECT = 2
private const val TYPE_UNIVERSE = 3

/** Describes a single criterion template (not yet instantiated with a value). */
sealed class DesktopCriterionDef(
    val identifier: String,
    val name: String,
    /** Display text template; '?' is replaced with the selected value. */
    val textTemplate: String,
    /** SQL subquery template; '?' is replaced with the selected value. Null for universe. */
    val sql: String?,
    /** Map of task field → value template for new-task defaults. Null means no defaults. */
    val valuesForNewTasks: Map<String, Any>?,
) {
    /** No UI needed – the condition is added directly. */
    class Boolean(
        identifier: String, name: String, sql: String,
    ) : DesktopCriterionDef(identifier, name, name, sql, null)

    /** User picks from a fixed list. */
    class MultipleSelect(
        identifier: String, name: String, textTemplate: String, sql: String?,
        val displayLabels: List<String>,
        val entryValues: List<String>,
        valuesForNewTasks: Map<String, Any>?,
    ) : DesktopCriterionDef(identifier, name, textTemplate, sql, valuesForNewTasks)

    /** User types free text. */
    class TextInput(
        identifier: String, name: String, textTemplate: String, sql: String,
    ) : DesktopCriterionDef(identifier, name, textTemplate, sql, null)
}

/** One instantiated condition in the filter condition list. */
private data class DesktopCondition(
    val id: String = UUID.randomUUID().toString(),
    val criterionId: String,
    /** Text template (with ?) for serialization. */
    val textTemplate: String,
    /** Instantiated display text shown in the UI. */
    val displayText: String,
    val sql: String?,
    val value: String?,
    val valuesForNewTasks: Map<String, Any>?,
    val conditionType: Int,   // TYPE_UNIVERSE, TYPE_INTERSECT, TYPE_ADD, TYPE_SUBTRACT
)

// ────────────────────────────────────────────────────────────────────────────
// SQL template builders (computed once via lazy)
// ────────────────────────────────────────────────────────────────────────────

private val TITLE_CONTAINS_SQL by lazy {
    select(Task.ID).from(Task.TABLE).where(and(activeAndVisible(), Task.TITLE.like("%?%"))).toString()
}

private val PRIORITY_SQL by lazy {
    select(Task.ID).from(Task.TABLE).where(and(activeAndVisible(), Task.IMPORTANCE.lte("?"))).toString()
}

private val DUE_BEFORE_SQL by lazy {
    select(Task.ID).from(Task.TABLE).where(
        and(
            activeAndVisible(),
            or(field("?").eq(0), Task.DUE_DATE.gt(0)),
            or(
                Task.DUE_DATE.lte("?"),
                and(
                    field("${Task.DUE_DATE} / 1000 % 60").eq(0),
                    field("?").eq(field(PermaSql.VALUE_NOW)),
                    Task.DUE_DATE.lte(PermaSql.VALUE_EOD)
                )
            )
        )
    ).toString()
}

private val START_BEFORE_SQL by lazy {
    select(Task.ID).from(Task.TABLE).where(
        and(
            activeAndVisible(),
            or(field("?").eq(0), Task.HIDE_UNTIL.gt(0)),
            Task.HIDE_UNTIL.lte("?")
        )
    ).toString()
}

private val TAG_CONTAINS_SQL by lazy {
    select(Tag.TASK).from(Tag.TABLE)
        .join(inner(Task.TABLE, Tag.TASK.eq(Task.ID)))
        .where(and(activeAndVisible(), Tag.NAME.like("%?%")))
        .toString()
}

private val RECURRING_SQL by lazy {
    select(Task.ID).from(Task.TABLE)
        .where(field("LENGTH(${Task.RECURRENCE})>0").eq(1))
        .toString()
}

private val COMPLETED_SQL by lazy {
    select(Task.ID).from(Task.TABLE)
        .where(field("${Task.COMPLETION_DATE.lt(1)}").eq(0))
        .toString()
}

private val HIDDEN_SQL by lazy {
    select(Task.ID).from(Task.TABLE)
        .where(field("${Task.HIDE_UNTIL.gt(PermaSql.VALUE_NOW)}").eq(1))
        .toString()
}

private val PARENT_SQL by lazy {
    select(Task.ID).from(Task.TABLE)
        .join(inner(Task.TABLE.`as`("children"), Task.ID.eq(field("children.parent"))))
        .where(isNotNull(field("children._id")))
        .toString()
}

private val SUBTASK_SQL by lazy {
    select(Task.ID).from(Task.TABLE)
        .where(field("${Task.PARENT}>0").eq(1))
        .toString()
}

private val REMINDERS_SQL by lazy {
    select(Task.ID).from(Task.TABLE)
        .where(exists(select(field("1")).from(Alarm.TABLE).where(Alarm.TASK.eq(Task.ID))))
        .toString()
}

private val DATE_DISPLAY_LABELS = listOf(
    "no due date", "yesterday", "today", "tomorrow", "the day after tomorrow", "next week", "next month", "now"
)
private val DATE_ENTRY_VALUES = listOf(
    "0",
    PermaSql.VALUE_EOD_YESTERDAY,
    PermaSql.VALUE_EOD,
    PermaSql.VALUE_EOD_TOMORROW,
    PermaSql.VALUE_EOD_DAY_AFTER,
    PermaSql.VALUE_EOD_NEXT_WEEK,
    PermaSql.VALUE_EOD_NEXT_MONTH,
    PermaSql.VALUE_NOW,
)

private val PRIORITY_DISPLAY_LABELS = listOf("!!!", "!!", "!", "none")
private val PRIORITY_ENTRY_VALUES = listOf(
    Task.Priority.HIGH.toString(),
    Task.Priority.MEDIUM.toString(),
    Task.Priority.LOW.toString(),
    Task.Priority.NONE.toString(),
)

// Colors (same as in FilterEditScreen / TagEditScreen)
private val CREATE_FILTER_COLORS = listOf(
    0 to "None",
    0xFFE57373.toInt() to "Red",
    0xFFFF8A65.toInt() to "Orange",
    0xFFFFD54F.toInt() to "Yellow",
    0xFF81C784.toInt() to "Green",
    0xFF4FC3F7.toInt() to "Light Blue",
    0xFF64B5F6.toInt() to "Blue",
    0xFF9575CD.toInt() to "Purple",
    0xFFF06292.toInt() to "Pink",
    0xFFA1887F.toInt() to "Brown",
    0xFF90A4AE.toInt() to "Gray",
)

// ────────────────────────────────────────────────────────────────────────────
// SQL / criterion serialization helpers
// ────────────────────────────────────────────────────────────────────────────

private fun escapeField(s: String?): String =
    s?.replace(SERIALIZATION_SEPARATOR, SEPARATOR_ESCAPE) ?: ""

/** Assemble the filter's WHERE clause from a list of instantiated conditions. */
private fun buildFilterSql(conditions: List<DesktopCondition>): String {
    val sb = StringBuilder(" WHERE ")
    for (c in conditions) {
        when (c.conditionType) {
            TYPE_ADD -> sb.append(" OR ")
            TYPE_SUBTRACT -> sb.append(" AND NOT ")
            TYPE_INTERSECT -> sb.append(" AND ")
            // TYPE_UNIVERSE: no prefix on the first item
        }
        if (c.conditionType == TYPE_UNIVERSE || c.sql == null) {
            sb.append(activeAndVisible())
        } else {
            val subSql = c.sql.replace("?", UnaryCriterion.sanitize(c.value!!)).trim()
            sb.append("${Task.ID} IN ($subSql)")
        }
    }
    return sb.toString()
}

/** Build the serialized criterion string (stored in Filter.criterion). */
private fun buildCriterionString(conditions: List<DesktopCondition>): String {
    return conditions.joinToString("\n") { c ->
        listOf(
            escapeField(c.criterionId),
            escapeField(c.value),
            escapeField(c.textTemplate),
            c.conditionType,
            c.sql ?: ""
        ).joinToString(SERIALIZATION_SEPARATOR)
    }.trim()
}

/** Build the serialized values string for new-task defaults (stored in Filter.values). */
private fun buildValuesString(conditions: List<DesktopCondition>): String {
    val values = mutableMapOf<String, Any>()
    for (c in conditions) {
        if (c.conditionType == TYPE_INTERSECT && c.valuesForNewTasks != null) {
            for ((key, value1) in c.valuesForNewTasks) {
                values[key.replace("?", c.value!!)] = value1.toString().replace("?", c.value)
            }
        }
    }
    return mapToSerializedString(values)
}

// ────────────────────────────────────────────────────────────────────────────
// Main composable
// ────────────────────────────────────────────────────────────────────────────

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun FilterCreateScreen(
    application: DesktopApplication,
    onNavigateBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()

    var name by remember { mutableStateOf("") }
    var selectedColor by remember { mutableStateOf(0) }

    // Available tags and calendars (loaded from DB)
    var tagNames by remember { mutableStateOf<List<String>>(emptyList()) }
    var calendarNames by remember { mutableStateOf<List<String>>(emptyList()) }
    var calendarUuids by remember { mutableStateOf<List<String>>(emptyList()) }

    // Condition list – starts with the immutable universe row
    val conditions = remember {
        mutableStateListOf(
            DesktopCondition(
                criterionId = ID_UNIVERSE,
                textTemplate = "Active",
                displayText = "Active tasks",
                sql = null,
                value = null,
                valuesForNewTasks = null,
                conditionType = TYPE_UNIVERSE,
            )
        )
    }

    // Dialog state
    var showCriterionPicker by remember { mutableStateOf(false) }
    var pendingCriterionDef by remember { mutableStateOf<DesktopCriterionDef?>(null) }
    var showMultiSelectDialog by remember { mutableStateOf(false) }
    var showTextInputDialog by remember { mutableStateOf(false) }
    // Edit condition-type (AND/OR/NOT) dialog
    var editingConditionId by remember { mutableStateOf<String?>(null) }

    // Load tags and calendars
    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) {
            tagNames = application.tagDataDao.tagDataOrderedByName()
                .mapNotNull { it.name }
                .distinct()
            val calendars = application.caldavDao.getCalendars()
            calendarNames = calendars.mapNotNull { it.name }
            calendarUuids = calendars.mapNotNull { it.uuid }
        }
    }

    // Build criterion definitions (depends on loaded tags/calendars)
    val criterionDefs: List<DesktopCriterionDef> = remember(tagNames, calendarNames, calendarUuids) {
        buildCriterionDefs(tagNames, calendarNames, calendarUuids)
    }

    fun saveFilter() {
        if (name.isBlank()) return
        scope.launch(Dispatchers.IO) {
            val filter = Filter(
                title = name.trim(),
                color = selectedColor,
                sql = buildFilterSql(conditions),
                criterion = buildCriterionString(conditions),
                values = buildValuesString(conditions),
                order = NO_ORDER,
            )
            application.filterDao.insert(filter)
            withContext(Dispatchers.Main) { onNavigateBack() }
        }
    }

    // ── Dialogs ──────────────────────────────────────────────────────────────

    // 1. Pick criterion category
    if (showCriterionPicker) {
        PickCriterionDialog(
            criteria = criterionDefs,
            onDismiss = { showCriterionPicker = false },
            onSelected = { def ->
                showCriterionPicker = false
                when (def) {
                    is DesktopCriterionDef.Boolean -> {
                        conditions.add(
                            DesktopCondition(
                                criterionId = def.identifier,
                                textTemplate = def.textTemplate,
                                displayText = def.name,
                                sql = def.sql,
                                value = def.name,
                                valuesForNewTasks = null,
                                conditionType = TYPE_INTERSECT,
                            )
                        )
                    }
                    is DesktopCriterionDef.MultipleSelect -> {
                        pendingCriterionDef = def
                        showMultiSelectDialog = true
                    }
                    is DesktopCriterionDef.TextInput -> {
                        pendingCriterionDef = def
                        showTextInputDialog = true
                    }
                }
            }
        )
    }

    // 2. MultipleSelect value picker
    if (showMultiSelectDialog) {
        val def = pendingCriterionDef as? DesktopCriterionDef.MultipleSelect
        if (def != null) {
            PickFromListDialog(
                title = def.name,
                items = def.displayLabels,
                onDismiss = { showMultiSelectDialog = false; pendingCriterionDef = null },
                onSelected = { index ->
                    val value = def.entryValues[index]
                    val label = def.displayLabels[index]
                    conditions.add(
                        DesktopCondition(
                            criterionId = def.identifier,
                            textTemplate = def.textTemplate,
                            displayText = def.textTemplate.replace("?", label),
                            sql = def.sql,
                            value = value,
                            valuesForNewTasks = def.valuesForNewTasks,
                            conditionType = TYPE_INTERSECT,
                        )
                    )
                    showMultiSelectDialog = false
                    pendingCriterionDef = null
                }
            )
        }
    }

    // 3. TextInput value dialog
    if (showTextInputDialog) {
        val def = pendingCriterionDef as? DesktopCriterionDef.TextInput
        if (def != null) {
            TextInputDialog(
                title = def.name,
                onDismiss = { showTextInputDialog = false; pendingCriterionDef = null },
                onConfirm = { text ->
                    conditions.add(
                        DesktopCondition(
                            criterionId = def.identifier,
                            textTemplate = def.textTemplate,
                            displayText = def.textTemplate.replace("?", text),
                            sql = def.sql,
                            value = text,
                            valuesForNewTasks = null,
                            conditionType = TYPE_INTERSECT,
                        )
                    )
                    showTextInputDialog = false
                    pendingCriterionDef = null
                }
            )
        }
    }

    // 4. Condition-type (AND / OR / AND NOT) editor
    editingConditionId?.let { editId ->
        val idx = conditions.indexOfFirst { it.id == editId }
        if (idx >= 0) {
            ConditionTypeDialog(
                currentType = conditions[idx].conditionType,
                onDismiss = { editingConditionId = null },
                onSelected = { newType ->
                    conditions[idx] = conditions[idx].copy(conditionType = newType)
                    editingConditionId = null
                }
            )
        }
    }

    // ── Main scaffold ─────────────────────────────────────────────────────────
    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = { Text("New filter") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    IconButton(onClick = ::saveFilter, enabled = name.isNotBlank()) {
                        Icon(Icons.Default.Check, contentDescription = "Save")
                    }
                }
            )
        },
        floatingActionButton = {
            FloatingActionButton(onClick = { showCriterionPicker = true }) {
                Icon(Icons.Default.Add, contentDescription = "Add condition")
            }
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(16.dp)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Filter name") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )

            Text("Color", style = MaterialTheme.typography.titleMedium)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                CREATE_FILTER_COLORS.forEach { (colorValue, colorName) ->
                    FilterColorOption(
                        color = colorValue,
                        name = colorName,
                        isSelected = selectedColor == colorValue,
                        onClick = { selectedColor = colorValue },
                    )
                }
            }

            Text("Conditions", style = MaterialTheme.typography.titleMedium)

            conditions.forEachIndexed { index, condition ->
                ConditionRow(
                    condition = condition,
                    isUniverse = index == 0,
                    onClickType = { editingConditionId = condition.id },
                    onDelete = { conditions.removeAt(index) },
                )
            }

            // Leave space at the bottom for the FAB
            Spacer(modifier = Modifier.height(72.dp))
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Criterion definitions builder
// ────────────────────────────────────────────────────────────────────────────

private fun buildCriterionDefs(
    tagNames: List<String>,
    calendarNames: List<String>,
    calendarUuids: List<String>,
): List<DesktopCriterionDef> = buildList {
    // Tag is (multi-select of tag names)
    if (tagNames.isNotEmpty()) {
        add(
            DesktopCriterionDef.MultipleSelect(
                identifier = ID_TAG_IS,
                name = "Tag is",
                textTemplate = "Tag is: ?",
                sql = select(Tag.TASK).from(Tag.TABLE)
                    .join(inner(Task.TABLE, Tag.TASK.eq(Task.ID)))
                    .where(and(activeAndVisible(), Tag.NAME.eq("?")))
                    .toString(),
                displayLabels = tagNames,
                entryValues = tagNames,
                valuesForNewTasks = mapOf(Tag.KEY to "?"),
            )
        )
    }
    // Tag contains
    add(
        DesktopCriterionDef.TextInput(
            identifier = ID_TAG_CONTAINS,
            name = "Tag contains",
            textTemplate = "Tag contains: ?",
            sql = TAG_CONTAINS_SQL,
        )
    )
    // Start date
    add(
        DesktopCriterionDef.MultipleSelect(
            identifier = ID_STARTDATE,
            name = "Start date before",
            textTemplate = "Starts before: ?",
            sql = START_BEFORE_SQL,
            displayLabels = DATE_DISPLAY_LABELS,
            entryValues = DATE_ENTRY_VALUES,
            valuesForNewTasks = mapOf(Task.HIDE_UNTIL.name to "?"),
        )
    )
    // Due date
    add(
        DesktopCriterionDef.MultipleSelect(
            identifier = ID_DUEDATE,
            name = "Due date before",
            textTemplate = "Due before: ?",
            sql = DUE_BEFORE_SQL,
            displayLabels = DATE_DISPLAY_LABELS,
            entryValues = DATE_ENTRY_VALUES,
            valuesForNewTasks = mapOf(Task.DUE_DATE.name to "?"),
        )
    )
    // Priority
    add(
        DesktopCriterionDef.MultipleSelect(
            identifier = ID_IMPORTANCE,
            name = "Priority at most",
            textTemplate = "Priority ≤ ?",
            sql = PRIORITY_SQL,
            displayLabels = PRIORITY_DISPLAY_LABELS,
            entryValues = PRIORITY_ENTRY_VALUES,
            valuesForNewTasks = mapOf(Task.IMPORTANCE.name to "?"),
        )
    )
    // Title contains
    add(
        DesktopCriterionDef.TextInput(
            identifier = ID_TITLE,
            name = "Title contains",
            textTemplate = "Title contains: ?",
            sql = TITLE_CONTAINS_SQL,
        )
    )
    // List is (CalDAV calendar)
    if (calendarNames.isNotEmpty()) {
        add(
            DesktopCriterionDef.MultipleSelect(
                identifier = ID_CALDAV,
                name = "List is",
                textTemplate = "List is: ?",
                sql = select(CaldavTask.TASK).from(CaldavTask.TABLE)
                    .join(inner(Task.TABLE, CaldavTask.TASK.eq(Task.ID)))
                    .where(and(activeAndVisible(), CaldavTask.DELETED.eq(0), CaldavTask.CALENDAR.eq("?")))
                    .toString(),
                displayLabels = calendarNames,
                entryValues = calendarUuids,
                valuesForNewTasks = mapOf(CaldavTask.KEY to "?"),
            )
        )
    }
    // Boolean conditions
    add(DesktopCriterionDef.Boolean(ID_RECUR, "Is recurring", RECURRING_SQL))
    add(DesktopCriterionDef.Boolean(ID_COMPLETED, "Is completed", COMPLETED_SQL))
    add(DesktopCriterionDef.Boolean(ID_HIDDEN, "Is unstarted", HIDDEN_SQL))
    add(DesktopCriterionDef.Boolean(ID_PARENT, "Has subtask", PARENT_SQL))
    add(DesktopCriterionDef.Boolean(ID_SUBTASK, "Is a subtask", SUBTASK_SQL))
    add(DesktopCriterionDef.Boolean(ID_REMINDERS, "Has a reminder", REMINDERS_SQL))
}

// ────────────────────────────────────────────────────────────────────────────
// Small composables
// ────────────────────────────────────────────────────────────────────────────

@Composable
private fun ConditionRow(
    condition: DesktopCondition,
    isUniverse: kotlin.Boolean,
    onClickType: () -> Unit,
    onDelete: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (!isUniverse) {
                val typeLabel = when (condition.conditionType) {
                    TYPE_ADD -> "OR"
                    TYPE_SUBTRACT -> "AND NOT"
                    else -> "AND"
                }
                Surface(
                    shape = RoundedCornerShape(4.dp),
                    color = MaterialTheme.colorScheme.secondaryContainer,
                    modifier = Modifier.clickable(onClick = onClickType),
                ) {
                    Text(
                        text = typeLabel,
                        style = MaterialTheme.typography.labelSmall,
                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 3.dp),
                    )
                }
                Spacer(modifier = Modifier.width(8.dp))
            }
            Text(
                text = condition.displayText,
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.bodyMedium,
            )
            if (!isUniverse) {
                IconButton(onClick = onDelete, modifier = Modifier.size(32.dp)) {
                    Icon(
                        Icons.Default.Close,
                        contentDescription = "Remove",
                        modifier = Modifier.size(16.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun PickCriterionDialog(
    criteria: List<DesktopCriterionDef>,
    onDismiss: () -> Unit,
    onSelected: (DesktopCriterionDef) -> Unit,
) {
    Dialog(onDismissRequest = onDismiss) {
        Surface(shape = RoundedCornerShape(12.dp)) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    "Add condition",
                    style = MaterialTheme.typography.titleMedium,
                    modifier = Modifier.padding(bottom = 8.dp),
                )
                LazyColumn(modifier = Modifier.height(360.dp)) {
                    items(criteria) { def ->
                        Text(
                            text = def.name,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { onSelected(def) }
                                .padding(vertical = 12.dp, horizontal = 8.dp),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
                TextButton(onClick = onDismiss, modifier = Modifier.align(Alignment.End)) {
                    Text("Cancel")
                }
            }
        }
    }
}

@Composable
private fun PickFromListDialog(
    title: String,
    items: List<String>,
    onDismiss: () -> Unit,
    onSelected: (Int) -> Unit,
) {
    Dialog(onDismissRequest = onDismiss) {
        Surface(shape = RoundedCornerShape(12.dp)) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(bottom = 8.dp))
                LazyColumn(modifier = Modifier.height(300.dp)) {
                    items(items.size) { index ->
                        Text(
                            text = items[index],
                            modifier = Modifier
                                .fillMaxWidth()
                                .clickable { onSelected(index) }
                                .padding(vertical = 12.dp, horizontal = 8.dp),
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
                TextButton(onClick = onDismiss, modifier = Modifier.align(Alignment.End)) {
                    Text("Cancel")
                }
            }
        }
    }
}

@Composable
private fun TextInputDialog(
    title: String,
    onDismiss: () -> Unit,
    onConfirm: (String) -> Unit,
) {
    var text by remember { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                label = { Text("Value") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(
                onClick = { if (text.isNotBlank()) onConfirm(text.trim()) },
                enabled = text.isNotBlank(),
            ) { Text("Add") }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        }
    )
}

@Composable
private fun ConditionTypeDialog(
    currentType: Int,
    onDismiss: () -> Unit,
    onSelected: (Int) -> Unit,
) {
    val types = listOf(TYPE_INTERSECT to "AND", TYPE_ADD to "OR", TYPE_SUBTRACT to "AND NOT")
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Condition type") },
        text = {
            Column {
                types.forEach { (type, label) ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onSelected(type) }
                            .padding(vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(
                            selected = currentType == type,
                            onClick = { onSelected(type) },
                        )
                        Spacer(modifier = Modifier.width(8.dp))
                        Text(label, style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } }
    )
}

@Composable
private fun FilterColorOption(
    color: Int,
    name: String,
    isSelected: kotlin.Boolean,
    onClick: () -> Unit,
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.clickable(onClick = onClick),
    ) {
        Box(
            modifier = Modifier
                .size(40.dp)
                .clip(CircleShape)
                .background(
                    if (color == 0) MaterialTheme.colorScheme.surfaceVariant
                    else Color(color)
                )
                .then(
                    if (isSelected) Modifier.border(
                        width = 3.dp,
                        color = MaterialTheme.colorScheme.primary,
                        shape = CircleShape,
                    ) else Modifier
                ),
            contentAlignment = Alignment.Center,
        ) {
            if (isSelected) {
                Icon(
                    imageVector = Icons.Default.Check,
                    contentDescription = "Selected",
                    tint = if (color == 0) MaterialTheme.colorScheme.onSurfaceVariant else Color.White,
                    modifier = Modifier.size(20.dp),
                )
            }
        }
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = name,
            style = MaterialTheme.typography.bodySmall,
            color = if (isSelected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
