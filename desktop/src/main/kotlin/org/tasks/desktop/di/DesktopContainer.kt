package org.tasks.desktop.di

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.room.Room
import androidx.sqlite.driver.bundled.BundledSQLiteDriver
import kotlinx.coroutines.Dispatchers
import org.tasks.caldav.FileStorage
import org.tasks.caldav.VtodoCache
import org.tasks.compose.drawer.DrawerConfiguration
import org.tasks.data.dao.AlarmDao
import org.tasks.data.dao.CaldavDao
import org.tasks.data.dao.DeletionDao
import org.tasks.data.dao.FilterDao
import org.tasks.data.dao.GoogleTaskDao
import org.tasks.data.dao.LocationDao
import org.tasks.data.dao.NotificationDao
import org.tasks.data.dao.TagDao
import org.tasks.data.dao.TagDataDao
import org.tasks.data.dao.TaskAttachmentDao
import org.tasks.data.dao.TaskDao
import org.tasks.data.dao.TaskListMetadataDao
import org.tasks.data.dao.UserActivityDao
import org.tasks.data.db.Database
import org.tasks.desktop.navigation.Navigator
import org.tasks.desktop.platform.DesktopPaths
import org.tasks.desktop.platform.ThemeManager
import org.tasks.desktop.platform.WindowStateManager
import org.tasks.desktop.sync.DesktopSyncManager
import org.tasks.filters.FilterProvider
import org.tasks.kmp.createDataStore
import org.tasks.kmp.dataStoreFileName
import org.tasks.preferences.TasksPreferences
import java.io.File

class DesktopContainer {
    init {
        DesktopPaths.ensureDirectoriesExist()
    }

    val database: Database by lazy { createDatabase() }

    private val dataStore: DataStore<Preferences> by lazy {
        createDataStore {
            File(DesktopPaths.appDataDir, dataStoreFileName).absolutePath
        }
    }

    val tasksPreferences: TasksPreferences by lazy { TasksPreferences(dataStore) }

    val drawerConfiguration: DrawerConfiguration = object : DrawerConfiguration {
        override val filtersEnabled: Boolean = true
        override val placesEnabled: Boolean = true
        override val hideUnusedPlaces: Boolean = false
        override val tagsEnabled: Boolean = true
        override val hideUnusedTags: Boolean = false
        override val todayFilter: Boolean = true
        override val recentlyModifiedFilter: Boolean = true
    }

    // DAOs
    val taskDao: TaskDao by lazy { database.taskDao() }
    val caldavDao: CaldavDao by lazy { database.caldavDao() }
    val tagDataDao: TagDataDao by lazy { database.tagDataDao() }
    val tagDao: TagDao by lazy { database.tagDao() }
    val filterDao: FilterDao by lazy { database.filterDao() }
    val locationDao: LocationDao by lazy { database.locationDao() }
    val alarmDao: AlarmDao by lazy { database.alarmDao() }
    val deletionDao: DeletionDao by lazy { database.deletionDao() }
    val taskAttachmentDao: TaskAttachmentDao by lazy { database.taskAttachmentDao() }
    val notificationDao: NotificationDao by lazy { database.notificationDao() }
    val userActivityDao: UserActivityDao by lazy { database.userActivityDao() }
    val googleTaskDao: GoogleTaskDao by lazy { database.googleTaskDao() }
    val taskListMetadataDao: TaskListMetadataDao by lazy { database.taskListMetadataDao() }

    // VTODO file cache — stores raw iCalendar strings for conflict detection and upload.
    // FileStorage uses appDataDir as its root; VtodoCache adds a "vtodo/" subdirectory.
    val vtodoCache: VtodoCache by lazy {
        VtodoCache(
            caldavDao = caldavDao,
            fileStorage = FileStorage(DesktopPaths.appDataDir.absolutePath),
        )
    }

    // Filter provider
    val filterProvider: FilterProvider by lazy {
        FilterProvider(
            filterDao = filterDao,
            tagDataDao = tagDataDao,
            caldavDao = caldavDao,
            configuration = drawerConfiguration,
            locationDao = locationDao,
            taskDao = taskDao,
            tasksPreferences = tasksPreferences,
        )
    }

    // Navigation
    val navigator: Navigator = Navigator()

    // Sync manager
    val syncManager: DesktopSyncManager by lazy {
        DesktopSyncManager.getInstance(
            caldavDao = caldavDao,
            taskDao = taskDao,
            deletionDao = deletionDao,
            vtodoCache = vtodoCache,
        )
    }

    // Platform managers
    val windowStateManager: WindowStateManager by lazy { WindowStateManager() }
    val themeManager: ThemeManager by lazy { ThemeManager() }

    private fun createDatabase(): Database {
        val dbFile = DesktopPaths.databaseFile
        return Room
            .databaseBuilder<Database>(name = dbFile.absolutePath)
            .setDriver(BundledSQLiteDriver())
            .setQueryCoroutineContext(Dispatchers.IO)
            .fallbackToDestructiveMigration(dropAllTables = true)
            .build()
    }

    companion object {
        @Volatile
        private var instance: DesktopContainer? = null

        fun getInstance(): DesktopContainer {
            return instance ?: synchronized(this) {
                instance ?: DesktopContainer().also { instance = it }
            }
        }
    }
}
