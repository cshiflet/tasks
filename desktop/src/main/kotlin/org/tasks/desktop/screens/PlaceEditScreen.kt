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
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import org.tasks.data.entity.Place
import org.tasks.desktop.DesktopApplication

// Preset colors for places (same as tags)
private val PLACE_COLORS = listOf(
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

// Preset radius options in meters
private val RADIUS_OPTIONS = listOf(
    100 to "100m",
    150 to "150m",
    200 to "200m",
    250 to "250m (default)",
    300 to "300m",
    500 to "500m",
    750 to "750m",
    1000 to "1km",
)

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun PlaceEditScreen(
    placeId: Long?,
    application: DesktopApplication,
    onNavigateBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scope = rememberCoroutineScope()
    val isNew = placeId == null

    var place by remember { mutableStateOf<Place?>(null) }
    var name by remember { mutableStateOf("") }
    var address by remember { mutableStateOf("") }
    var phone by remember { mutableStateOf("") }
    var url by remember { mutableStateOf("") }
    var latitude by remember { mutableStateOf("") }
    var longitude by remember { mutableStateOf("") }
    var radius by remember { mutableStateOf(250) }
    var selectedColor by remember { mutableStateOf(0) }
    var showDeleteDialog by remember { mutableStateOf(false) }
    var showRadiusMenu by remember { mutableStateOf(false) }
    var isLoading by remember { mutableStateOf(!isNew) }

    // Load existing place data
    LaunchedEffect(placeId) {
        if (placeId != null) {
            withContext(Dispatchers.IO) {
                place = application.locationDao.getPlace(placeId)
                place?.let {
                    name = it.name ?: ""
                    address = it.address ?: ""
                    phone = it.phone ?: ""
                    url = it.url ?: ""
                    latitude = if (it.latitude != 0.0) it.latitude.toString() else ""
                    longitude = if (it.longitude != 0.0) it.longitude.toString() else ""
                    radius = it.radius
                    selectedColor = it.color
                }
            }
            isLoading = false
        }
    }

    fun savePlace() {
        if (name.isBlank() && address.isBlank()) return

        scope.launch(Dispatchers.IO) {
            val lat = latitude.toDoubleOrNull() ?: 0.0
            val lng = longitude.toDoubleOrNull() ?: 0.0

            if (isNew) {
                application.locationDao.insert(
                    Place(
                        name = name.trim().ifBlank { null },
                        address = address.trim().ifBlank { null },
                        phone = phone.trim().ifBlank { null },
                        url = url.trim().ifBlank { null },
                        latitude = lat,
                        longitude = lng,
                        radius = radius,
                        color = selectedColor,
                    )
                )
            } else {
                place?.let {
                    application.locationDao.update(
                        it.copy(
                            name = name.trim().ifBlank { null },
                            address = address.trim().ifBlank { null },
                            phone = phone.trim().ifBlank { null },
                            url = url.trim().ifBlank { null },
                            latitude = lat,
                            longitude = lng,
                            radius = radius,
                            color = selectedColor,
                        )
                    )
                }
            }
            withContext(Dispatchers.Main) {
                onNavigateBack()
            }
        }
    }

    fun deletePlace() {
        scope.launch(Dispatchers.IO) {
            place?.let {
                // Delete associated geofences first
                it.uid?.let { uid -> application.locationDao.deleteGeofencesByPlace(uid) }
                application.locationDao.delete(it)
            }
            withContext(Dispatchers.Main) {
                onNavigateBack()
            }
        }
    }

    // Delete confirmation dialog
    if (showDeleteDialog) {
        AlertDialog(
            onDismissRequest = { showDeleteDialog = false },
            title = { Text("Delete place?") },
            text = { Text("This will remove the place \"${name.ifBlank { address }}\" and its geofences from all tasks.") },
            confirmButton = {
                TextButton(
                    onClick = {
                        showDeleteDialog = false
                        deletePlace()
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
                title = { Text(if (isNew) "New place" else "Edit place") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back"
                        )
                    }
                },
                actions = {
                    if (!isNew && place != null) {
                        IconButton(onClick = { showDeleteDialog = true }) {
                            Icon(
                                imageVector = Icons.Default.Delete,
                                contentDescription = "Delete",
                                tint = MaterialTheme.colorScheme.error
                            )
                        }
                    }
                    IconButton(
                        onClick = { savePlace() },
                        enabled = name.isNotBlank() || address.isNotBlank()
                    ) {
                        Icon(
                            imageVector = Icons.Default.Check,
                            contentDescription = "Save"
                        )
                    }
                }
            )
        }
    ) { paddingValues ->
        if (isLoading) {
            Box(
                modifier = Modifier.fillMaxSize().padding(paddingValues),
                contentAlignment = Alignment.Center
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
                // Name field
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Name") },
                    placeholder = { Text("e.g., Home, Office, Grocery Store") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(16.dp))

                // Address field
                OutlinedTextField(
                    value = address,
                    onValueChange = { address = it },
                    label = { Text("Address") },
                    placeholder = { Text("e.g., 123 Main St, City, Country") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(16.dp))

                // Phone field
                OutlinedTextField(
                    value = phone,
                    onValueChange = { phone = it },
                    label = { Text("Phone (optional)") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(16.dp))

                // URL field
                OutlinedTextField(
                    value = url,
                    onValueChange = { url = it },
                    label = { Text("Website (optional)") },
                    placeholder = { Text("https://...") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )

                Spacer(modifier = Modifier.height(24.dp))

                // Coordinates section
                Text(
                    text = "Coordinates (optional)",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    text = "Enter coordinates for location-based reminders",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )

                Spacer(modifier = Modifier.height(12.dp))

                Row(
                    horizontalArrangement = Arrangement.spacedBy(16.dp),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    OutlinedTextField(
                        value = latitude,
                        onValueChange = { latitude = it },
                        label = { Text("Latitude") },
                        placeholder = { Text("e.g., 37.7749") },
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = longitude,
                        onValueChange = { longitude = it },
                        label = { Text("Longitude") },
                        placeholder = { Text("e.g., -122.4194") },
                        modifier = Modifier.weight(1f),
                        singleLine = true,
                    )
                }

                Spacer(modifier = Modifier.height(24.dp))

                // Radius section
                Text(
                    text = "Geofence Radius",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    text = "Distance from location to trigger reminders",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )

                Spacer(modifier = Modifier.height(12.dp))

                Box {
                    TextButton(onClick = { showRadiusMenu = true }) {
                        Text(
                            text = RADIUS_OPTIONS.find { it.first == radius }?.second
                                ?: "${radius}m",
                            style = MaterialTheme.typography.bodyLarge,
                        )
                    }
                    DropdownMenu(
                        expanded = showRadiusMenu,
                        onDismissRequest = { showRadiusMenu = false }
                    ) {
                        RADIUS_OPTIONS.forEach { (value, label) ->
                            DropdownMenuItem(
                                text = { Text(label) },
                                onClick = {
                                    radius = value
                                    showRadiusMenu = false
                                }
                            )
                        }
                    }
                }

                Spacer(modifier = Modifier.height(24.dp))

                // Color picker
                Text(
                    text = "Color",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )

                Spacer(modifier = Modifier.height(12.dp))

                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                    modifier = Modifier.fillMaxWidth()
                ) {
                    PLACE_COLORS.forEach { (colorValue, colorName) ->
                        PlaceColorOption(
                            color = colorValue,
                            name = colorName,
                            isSelected = selectedColor == colorValue,
                            onClick = { selectedColor = colorValue }
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun PlaceColorOption(
    color: Int,
    name: String,
    isSelected: Boolean,
    onClick: () -> Unit,
) {
    Column(
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.clickable(onClick = onClick)
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
                        shape = CircleShape
                    ) else Modifier
                ),
            contentAlignment = Alignment.Center
        ) {
            if (isSelected) {
                Icon(
                    imageVector = Icons.Default.Check,
                    contentDescription = "Selected",
                    tint = if (color == 0) MaterialTheme.colorScheme.onSurfaceVariant else Color.White,
                    modifier = Modifier.size(20.dp)
                )
            }
        }
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = name,
            style = MaterialTheme.typography.bodySmall,
            color = if (isSelected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant
        )
    }
}
