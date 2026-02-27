package org.tasks.desktop.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import org.tasks.data.entity.Filter
import org.tasks.desktop.DesktopApplication

private val FILTER_COLORS = listOf(
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

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun FilterEditScreen(
    filterId: Long,
    application: DesktopApplication,
    onNavigateBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()

    var filter by remember { mutableStateOf<Filter?>(null) }
    var name by remember { mutableStateOf("") }
    var selectedColor by remember { mutableStateOf(0) }
    var showDeleteDialog by remember { mutableStateOf(false) }
    var isLoading by remember { mutableStateOf(true) }

    LaunchedEffect(filterId) {
        withContext(Dispatchers.IO) {
            val loaded = application.filterDao.getById(filterId)
            filter = loaded
            loaded?.let {
                name = it.title ?: ""
                selectedColor = it.color ?: 0
            }
        }
        isLoading = false
    }

    fun saveFilter() {
        if (name.isBlank()) return
        scope.launch(Dispatchers.IO) {
            filter?.let {
                application.filterDao.update(
                    it.copy(
                        title = name.trim(),
                        color = selectedColor,
                    )
                )
            }
            withContext(Dispatchers.Main) {
                onNavigateBack()
            }
        }
    }

    fun deleteFilter() {
        scope.launch(Dispatchers.IO) {
            filter?.let {
                application.filterDao.delete(it)
            }
            withContext(Dispatchers.Main) {
                onNavigateBack()
            }
        }
    }

    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Delete filter?") },
            text = { Text("This will permanently delete the filter \"$name\".") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteDialog = false
                        deleteFilter()
                    }
                ) {
                    Text("Delete", color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { showDeleteDialog = false }) {
                    Text("Cancel")
                }
            }
        )
    }

    Scaffold(
        modifier = modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = { Text("Edit filter") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                        )
                    }
                },
                actions = {
                    if (filter != null) {
                        IconButton(onClick = { showDeleteDialog = true }) {
                            Icon(
                                imageVector = Icons.Default.Delete,
                                contentDescription = "Delete",
                                tint = MaterialTheme.colorScheme.error,
                            )
                        }
                    }
                    IconButton(
                        onClick = { saveFilter() },
                        enabled = name.isNotBlank(),
                    ) {
                        Icon(
                            imageVector = Icons.Default.Check,
                            contentDescription = "Save",
                        )
                    }
                }
            )
        }
    ) { paddingValues ->
        if (isLoading) {
            Box(
                modifier = Modifier.fillMaxSize().padding(paddingValues),
                contentAlignment = Alignment.Center,
            ) {
                Text("Loading...")
            }
        } else {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(paddingValues)
                    .padding(16.dp)
                    .verticalScroll(rememberScrollState())
            ) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Filter name") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(24.dp))

                Text(
                    text = "Color",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )

                Spacer(modifier = Modifier.height(12.dp))

                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    FILTER_COLORS.forEach { (colorValue, colorName) ->
                        ColorOption(
                            color = colorValue,
                            name = colorName,
                            isSelected = selectedColor == colorValue,
                            onClick = { selectedColor = colorValue },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun ColorOption(
    color: Int,
    name: String,
    isSelected: Boolean,
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
