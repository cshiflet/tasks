package org.tasks.desktop.sync

import co.touchlab.kermit.Logger
import com.google.api.client.googleapis.json.GoogleJsonResponseException
import com.google.api.client.http.HttpResponseException
import com.google.api.client.http.javanet.NetHttpTransport
import com.google.api.client.json.gson.GsonFactory
import com.google.api.client.util.DateTime
import com.google.api.services.tasks.Tasks
import com.google.api.services.tasks.model.Task
import com.google.api.services.tasks.model.TaskList
import com.google.api.services.tasks.model.TaskLists
import com.google.auth.http.HttpCredentialsAdapter
import com.google.auth.oauth2.UserCredentials
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException
import java.util.Date
import java.util.TimeZone

private val LOG = Logger.withTag("DesktopGoogleTasksInvoker")

/**
 * Desktop-only wrapper around the Google Tasks REST API.
 *
 * Credentials: uses [UserCredentials] with a stored refresh token to obtain short-lived access
 * tokens automatically (the google-auth-library handles refresh transparently).
 *
 * Mirrors the API surface of the Android [GtasksInvoker] but avoids Android-specific types
 * (Timber, BuildConfig). Each call is executed on [Dispatchers.IO].
 */
class DesktopGoogleTasksInvoker(
    clientId: String,
    clientSecret: String,
    refreshToken: String,
) {
    private val service: Tasks = run {
        val credentials = UserCredentials.newBuilder()
            .setClientId(clientId)
            .setClientSecret(clientSecret)
            .setRefreshToken(refreshToken)
            .build()
        Tasks.Builder(
            NetHttpTransport(),
            GsonFactory.getDefaultInstance(),
            HttpCredentialsAdapter(credentials),
        )
            .setApplicationName("Tasks")
            .build()
    }

    // -----------------------------------------------------------------------
    // Task Lists
    // -----------------------------------------------------------------------

    @Throws(IOException::class)
    suspend fun allGtaskLists(pageToken: String?): TaskLists? =
        execute { service.tasklists().list().setMaxResults(100).setPageToken(pageToken) }

    @Throws(IOException::class)
    suspend fun createGtaskList(title: String?): TaskList? =
        execute { service.tasklists().insert(TaskList().setTitle(title)) }

    @Throws(IOException::class)
    suspend fun renameGtaskList(listId: String?, title: String?): TaskList? =
        execute { service.tasklists().patch(listId, TaskList().setTitle(title)) }

    @Throws(IOException::class)
    suspend fun deleteGtaskList(listId: String?) {
        try {
            execute { service.tasklists().delete(listId) }
        } catch (e: GoogleJsonResponseException) {
            if (e.statusCode != 404) throw e
        }
    }

    // -----------------------------------------------------------------------
    // Tasks
    // -----------------------------------------------------------------------

    @Throws(IOException::class)
    suspend fun getAllGtasksFromListId(
        listId: String?,
        lastSyncDate: Long,
        pageToken: String?,
    ): com.google.api.services.tasks.model.Tasks? = execute {
        service.tasks()
            .list(listId)
            .setMaxResults(100)
            .setShowDeleted(true)
            .setShowHidden(true)
            .setPageToken(pageToken)
            .setUpdatedMin(unixTimeToGtasksCompletionTime(lastSyncDate).toStringRfc3339())
    }

    @Throws(IOException::class)
    suspend fun getAllPositions(
        listId: String?,
        pageToken: String?,
    ): com.google.api.services.tasks.model.Tasks? = execute {
        service.tasks()
            .list(listId)
            .setMaxResults(100)
            .setShowDeleted(false)
            .setShowHidden(false)
            .setPageToken(pageToken)
            .setFields("items(id,parent,position),nextPageToken")
    }

    @Throws(IOException::class)
    suspend fun createGtask(
        listId: String?,
        task: Task?,
        parent: String?,
        previous: String?,
    ): Task? = execute {
        service.tasks().insert(listId, task).setParent(parent).setPrevious(previous)
    }

    @Throws(IOException::class)
    suspend fun updateGtask(listId: String?, task: Task): Task? =
        execute { service.tasks().update(listId, task.id, task) }

    @Throws(IOException::class)
    suspend fun moveGtask(
        listId: String?,
        taskId: String?,
        parentId: String?,
        previousId: String?,
    ): Task? = execute {
        service.tasks().move(listId, taskId).setParent(parentId).setPrevious(previousId)
    }

    @Throws(IOException::class)
    suspend fun deleteGtask(listId: String?, taskId: String?) {
        try {
            execute { service.tasks().delete(listId, taskId) }
        } catch (e: GoogleJsonResponseException) {
            if (e.statusCode != 404) throw e
        }
    }

    // -----------------------------------------------------------------------
    // Time utilities (adapted from GtasksApiUtilities.java — no Android deps)
    // -----------------------------------------------------------------------

    /** Mirrors GtasksApiUtilities.unixTimeToGtasksCompletionTime */
    fun unixTimeToGtasksCompletionTime(time: Long): DateTime =
        DateTime(Date(time), TimeZone.getDefault())

    /** Mirrors GtasksApiUtilities.unixTimeToGtasksDueDate */
    fun unixTimeToGtasksDueDate(time: Long): DateTime? {
        if (time <= 0) return null
        val date = Date(time / 1000 * 1000)
        @Suppress("DEPRECATION")
        date.hours = 0
        @Suppress("DEPRECATION")
        date.minutes = 0
        @Suppress("DEPRECATION")
        date.seconds = 0
        date.time = date.time - date.timezoneOffset * 60_000L
        return DateTime(date, TimeZone.getTimeZone("GMT"))
    }

    /** Mirrors GtasksApiUtilities.gtasksCompletedTimeToUnixTime */
    fun gtasksCompletedTimeToUnixTime(dt: DateTime?): Long =
        dt?.value ?: 0L

    /** Mirrors GtasksApiUtilities.gtasksDueTimeToUnixTime */
    fun gtasksDueTimeToUnixTime(dt: DateTime?): Long {
        dt ?: return 0L
        return try {
            val utcTime = dt.value
            @Suppress("DEPRECATION")
            val returnDate = Date(Date(utcTime).time + Date(utcTime).timezoneOffset * 60_000L)
            returnDate.time
        } catch (e: NumberFormatException) {
            LOG.e(e) { "Failed to parse gtasks due date" }
            0L
        }
    }

    // -----------------------------------------------------------------------
    // Internal execution
    // -----------------------------------------------------------------------

    @Throws(IOException::class)
    private suspend fun <T> execute(
        block: () -> com.google.api.client.googleapis.services.json.AbstractGoogleJsonClientRequest<T>,
    ): T? = withContext(Dispatchers.IO) {
        val request = block()
        try {
            request.execute()
        } catch (e: HttpResponseException) {
            if (e.statusCode == 404) null else throw e
        }
    }
}
