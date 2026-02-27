package org.tasks.desktop.sync

import at.bitfire.dav4jvm.DavCalendar
import at.bitfire.dav4jvm.DavResource
import at.bitfire.dav4jvm.Response
import at.bitfire.dav4jvm.Response.HrefRelation
import at.bitfire.dav4jvm.exception.DavException
import at.bitfire.dav4jvm.exception.HttpException
import at.bitfire.dav4jvm.property.CalendarData
import at.bitfire.dav4jvm.property.DisplayName
import at.bitfire.dav4jvm.property.GetETag
import at.bitfire.dav4jvm.property.GetETag.Companion.fromResponse
import co.touchlab.kermit.Logger
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody
import org.tasks.caldav.VtodoCache
import org.tasks.data.UUIDHelper
import org.tasks.data.dao.CaldavDao
import org.tasks.data.dao.DeletionDao
import org.tasks.data.dao.TaskDao
import org.tasks.data.entity.CaldavAccount
import org.tasks.data.entity.CaldavCalendar
import org.tasks.data.entity.CaldavCalendar.Companion.ACCESS_READ_ONLY
import org.tasks.data.entity.CaldavTask
import org.tasks.data.entity.Task
import org.tasks.desktop.sync.DesktopCaldavClient.Companion.accessLevel
import org.tasks.desktop.sync.DesktopCaldavClient.Companion.ctag
import org.tasks.time.DateTimeUtils2.currentTimeMillis
import java.io.IOException

private val LOG = Logger.withTag("DesktopCaldavSynchronizer")

/**
 * Full CalDAV synchronizer for the Tasks desktop application.
 *
 * Lifecycle per [sync] call:
 *  1. Validate that credentials are present.
 *  2. Discover the CalDAV home set and obtain an [OkHttpClient] wired with BasicDigest auth.
 *  3. PROPFIND the home set for all VTODO-capable calendars.
 *  4. Reconcile the local calendar list against the remote list (insert / update / delete).
 *  5. For each calendar:
 *     a. [fetchChanges] — download changed/new VTODOs and apply them to the local DB.
 *     b. [pushLocalChanges] — upload locally modified tasks and delete remotely-deleted tasks.
 *  6. Update parent-task links via [CaldavDao.updateParents].
 *
 * This class has no Android, Hilt, Firebase, or Timber dependencies — it is plain JVM/Kotlin.
 */
class DesktopCaldavSynchronizer(
    private val caldavDao: CaldavDao,
    private val taskDao: TaskDao,
    private val deletionDao: DeletionDao,
    private val vtodoCache: VtodoCache,
) {

    // MIME type expected by CalDAV servers for iCalendar objects.
    private val MIME_ICALENDAR = DavCalendar.MIME_ICALENDAR

    // ---------------------------------------------------------------------------
    // Top-level entry point
    // ---------------------------------------------------------------------------

    /**
     * Perform a full bidirectional sync for [account].
     *
     * @throws IllegalStateException if credentials are missing.
     * @throws IOException / HttpException / DavException on network or protocol errors.
     */
    suspend fun sync(account: CaldavAccount) {
        val serverUrl = account.url?.takeIf { it.isNotBlank() }
            ?: throw IllegalStateException("CalDAV account '${account.name}' has no server URL.")
        val username = account.username?.takeIf { it.isNotBlank() }
            ?: throw IllegalStateException("CalDAV account '${account.name}' has no username.")
        val password = account.password?.takeIf { it.isNotBlank() }
            ?: throw IllegalStateException("CalDAV account '${account.name}' has no password.")

        LOG.i { "Starting CalDAV sync for account '${account.name}' at $serverUrl" }

        // Build the HTTP client and discover the calendar home set.
        val client = DesktopCaldavClient.forAccount(serverUrl, username, password)

        // Fetch the list of remote calendars and reconcile the local DB.
        val remoteCalendars = client.calendars()
        reconcileCalendars(account, remoteCalendars)

        // Sync each calendar individually.
        val localCalendars = caldavDao.getCalendarsByAccount(account.uuid!!)
        for (calendar in localCalendars) {
            val calendarUrl = calendar.url?.toHttpUrl() ?: continue
            LOG.d { "Syncing calendar '${calendar.name}'" }
            try {
                fetchChanges(account, calendar, client.httpClient, calendarUrl)
                if (calendar.access != ACCESS_READ_ONLY) {
                    pushLocalChanges(account, calendar, client.httpClient, calendarUrl)
                }
            } catch (e: HttpException) {
                LOG.e(e) { "HTTP error syncing calendar '${calendar.name}': ${e.code} ${e.message}" }
                // Don't abort the entire sync for a single calendar failure.
            } catch (e: IOException) {
                LOG.e(e) { "IO error syncing calendar '${calendar.name}': ${e.message}" }
            }
        }

        LOG.i { "CalDAV sync complete for account '${account.name}'" }
    }

    // ---------------------------------------------------------------------------
    // Calendar list reconciliation
    // ---------------------------------------------------------------------------

    /**
     * Compare [remoteCalendars] to the locally stored calendars for [account] and:
     *  - Insert new calendars.
     *  - Update changed calendars (name, color, access level).
     *  - Mark calendars that no longer exist on the server for deletion (soft-delete via
     *    [CaldavDao.findDeletedCalendars] already returns them; here we hard-delete only
     *    the CaldavCalendar row — tasks themselves are left in the local DB to avoid data loss).
     */
    private suspend fun reconcileCalendars(
        account: CaldavAccount,
        remoteCalendars: List<Response>,
    ) {
        val remoteUrls = remoteCalendars.map { it.href.toString() }.toHashSet()

        // Calendars that existed locally but are no longer on the server.
        for (deleted in caldavDao.findDeletedCalendars(account.uuid!!, remoteUrls.toList())) {
            LOG.d { "Remote calendar removed: ${deleted.url}" }
            // Delete calendar row and cascade-delete its tasks, then clean up the vtodo cache.
            deletionDao.delete(deleted) { taskIds ->
                vtodoCache.delete(deleted)
            }
        }

        for (resource in remoteCalendars) {
            val url = resource.href.toString()
            val remoteName = resource[DisplayName::class.java]?.displayName ?: url
            val color = resource[at.bitfire.dav4jvm.property.CalendarColor::class.java]?.color ?: 0
            val access = resource.accessLevel

            var calendar = caldavDao.getCalendarByUrl(account.uuid!!, url)
            if (calendar == null) {
                calendar = CaldavCalendar(
                    name = remoteName,
                    account = account.uuid,
                    url = url,
                    uuid = UUIDHelper.newUUID(),
                    color = color,
                    access = access,
                )
                caldavDao.insert(calendar)
                LOG.d { "Inserted new calendar '$remoteName'" }
            } else if (
                calendar.name != remoteName ||
                calendar.color != color ||
                calendar.access != access
            ) {
                caldavDao.update(calendar.copy(name = remoteName, color = color, access = access))
                LOG.d { "Updated calendar '$remoteName'" }
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Fetch (pull) remote changes
    // ---------------------------------------------------------------------------

    /**
     * Download remote changes for [caldavCalendar] and apply them to the local database.
     *
     * Algorithm:
     *  1. Check whether the server's ctag/sync-token has changed.  If not, nothing to do.
     *  2. REPORT a calendar-query for all VTODO objects (filename + ETag only).
     *  3. Filter to items whose ETag differs from our locally stored value.
     *  4. Multi-GET the changed items in batches of 30 to retrieve full VCALENDAR data.
     *  5. Parse each VTODO and upsert into the local DB.
     *  6. Remove local tasks that no longer exist on the server.
     *  7. Persist the new ctag so subsequent syncs can short-circuit.
     */
    private suspend fun fetchChanges(
        account: CaldavAccount,
        caldavCalendar: CaldavCalendar,
        httpClient: OkHttpClient,
        httpUrl: HttpUrl,
    ) = withContext(Dispatchers.IO) {
        // Find the matching remote resource to get the current ctag.
        // We re-query via PROPFIND depth-0 on the calendar URL to get fresh metadata.
        val remoteCtag = getRemoteCtag(httpClient, httpUrl)
        if (caldavCalendar.ctag != null && caldavCalendar.ctag == remoteCtag) {
            LOG.d { "'${caldavCalendar.name}' is up to date (ctag match)" }
            return@withContext
        }

        LOG.d { "Fetching changes for '${caldavCalendar.name}'" }

        val davCalendar = DavCalendar(httpClient, httpUrl)

        // Step 2: query all VTODO hrefs + ETags.
        val members = ArrayList<Response>()
        davCalendar.calendarQuery("VTODO", null, null) { response, relation ->
            if (relation == HrefRelation.MEMBER) {
                members.add(response)
            }
        }

        // Step 3: filter to items that have changed (ETag differs or we don't have them locally).
        val changed = members.filter { memberResponse ->
            val eTag = memberResponse[GetETag::class.java]?.eTag
            if (eTag.isNullOrBlank()) return@filter false
            val fileName = memberResponse.hrefName()
            eTag != caldavDao.getTask(caldavCalendar.uuid!!, fileName)?.etag
        }
        LOG.d { "${changed.size} item(s) changed in '${caldavCalendar.name}'" }

        // Step 4: multi-GET the changed items in batches.
        for (batch in changed.chunked(30)) {
            val urls = batch.map { it.href }
            val responses = ArrayList<Response>()
            davCalendar.multiget(urls) { response, relation ->
                if (relation == HrefRelation.MEMBER) {
                    responses.add(response)
                }
            }

            // Step 5: parse and upsert each received VTODO.
            for (itemResponse in responses) {
                applyRemoteVtodo(account, caldavCalendar, itemResponse)
            }
        }

        // Step 6: delete local tasks that no longer exist on the server.
        val remoteFileNames = members.map { it.hrefName() }.toSet()
        val localObjectNames = caldavDao.getRemoteObjects(caldavCalendar.uuid!!).toSet()
        val removedLocally = localObjectNames - remoteFileNames
        if (removedLocally.isNotEmpty()) {
            LOG.d { "Deleting ${removedLocally.size} task(s) removed from server" }
            val tasksToDelete = caldavDao.getTasks(caldavCalendar.uuid!!, removedLocally.toList())
            vtodoCache.delete(caldavCalendar, tasksToDelete)
            for (caldavTask in tasksToDelete) {
                // Mark the Task row as deleted (soft delete) rather than hard-deleting.
                taskDao.fetch(caldavTask.task)?.let { task ->
                    taskDao.update(task.copy(deletionDate = currentTimeMillis()))
                }
                caldavDao.delete(caldavTask)
            }
        }

        // Step 7: persist the new ctag.
        caldavDao.update(caldavCalendar.copy(ctag = remoteCtag))
        caldavDao.updateParents(caldavCalendar.uuid!!)
        LOG.d { "Fetch complete for '${caldavCalendar.name}', new ctag=$remoteCtag" }
    }

    /**
     * PROPFIND depth-0 on [httpUrl] to retrieve the current ctag/sync-token.
     */
    private suspend fun getRemoteCtag(httpClient: OkHttpClient, httpUrl: HttpUrl): String? =
        withContext(Dispatchers.IO) {
            val responses = ArrayList<Pair<Response, HrefRelation>>()
            DavResource(httpClient, httpUrl).propfind(
                0,
                at.bitfire.dav4jvm.property.GetCTag.NAME,
                at.bitfire.dav4jvm.property.SyncToken.NAME,
            ) { response, relation ->
                responses.add(response to relation)
            }
            responses.firstOrNull()?.first?.ctag
        }

    /**
     * Parse one VTODO [Response] from a multi-GET and upsert it into the local database.
     */
    private suspend fun applyRemoteVtodo(
        account: CaldavAccount,
        caldavCalendar: CaldavCalendar,
        itemResponse: Response,
    ) {
        val eTag = itemResponse[GetETag::class.java]?.eTag
        if (eTag.isNullOrBlank()) {
            LOG.w { "Skipping response without ETag: ${itemResponse.href}" }
            return
        }
        val vtodoString = itemResponse[CalendarData::class.java]?.iCalendar
        if (vtodoString.isNullOrBlank()) {
            LOG.w { "Skipping response without CalendarData: ${itemResponse.href}" }
            return
        }

        val fileName = itemResponse.hrefName()
        val parsed = DesktopVtodoConverter.parse(vtodoString)
        if (parsed == null) {
            LOG.e { "Failed to parse VTODO from $fileName" }
            return
        }

        val existingCaldavTask = caldavDao.getTask(caldavCalendar.uuid!!, fileName)

        if (existingCaldavTask?.isDeleted() == true) {
            // Locally deleted — skip the incoming remote change.
            return
        }

        // Fetch or create the underlying Task row.
        val task: Task = if (existingCaldavTask != null) {
            taskDao.fetch(existingCaldavTask.task) ?: createNewTask(caldavCalendar)
        } else {
            createNewTask(caldavCalendar)
        }

        val caldavTask: CaldavTask = existingCaldavTask
            ?: CaldavTask(
                task = task.id,
                calendar = caldavCalendar.uuid,
                remoteId = parsed.uid ?: UUIDHelper.newUUID(),
                obj = fileName,
            )

        // Check whether the local task was modified after the last sync — if so, the local
        // version wins and we do NOT overwrite with the server version (server-wins would
        // lose local changes between two sync cycles).
        val locallyDirty = task.modificationDate > caldavTask.lastSync && caldavTask.lastSync != 0L

        if (!locallyDirty) {
            applyParsedToTask(parsed, task)
            task.suppressSync()
            task.suppressRefresh()
            taskDao.update(task)
        }

        // Always update the CaldavTask metadata (etag, remoteParent, etc.).
        val updatedCaldavTask = caldavTask.copy(
            task = task.id,
            remoteParent = parsed.parentUid,
            etag = eTag,
            lastSync = task.modificationDate,
        )

        // Persist the raw VTODO to the local cache.
        vtodoCache.putVtodo(caldavCalendar, updatedCaldavTask, vtodoString)

        if (existingCaldavTask == null) {
            caldavDao.insert(updatedCaldavTask)
            LOG.d { "Inserted task from $fileName" }
        } else {
            caldavDao.update(updatedCaldavTask)
            LOG.d { "Updated task from $fileName" }
        }
    }

    /** Create a new bare [Task] row and return it with a valid [Task.id]. */
    private suspend fun createNewTask(caldavCalendar: CaldavCalendar): Task {
        val task = Task(
            creationDate = currentTimeMillis(),
            modificationDate = currentTimeMillis(),
            readOnly = caldavCalendar.readOnly(),
        )
        val id = taskDao.createNew(task)
        return task.copy(id = id)
    }

    /**
     * Write all fields from [parsed] into [task] without touching sync-control fields.
     */
    private fun applyParsedToTask(parsed: DesktopVtodoConverter.ParsedVtodo, task: Task) {
        task.title = parsed.title?.takeIf { it.isNotBlank() } ?: task.title
        task.notes = parsed.notes
        task.priority = parsed.priority
        task.dueDate = parsed.dueDate
        task.hideUntil = parsed.hideUntil
        task.completionDate = parsed.completionDate
        task.recurrence = parsed.recurrence
        task.modificationDate = if (parsed.lastModified > 0) parsed.lastModified
                                 else currentTimeMillis()
    }

    // ---------------------------------------------------------------------------
    // Push (upload) local changes
    // ---------------------------------------------------------------------------

    /**
     * Push all locally modified tasks in [caldavCalendar] to the CalDAV server.
     *
     * Steps:
     *  1. Delete remote resources for tasks that have been moved out of this calendar.
     *  2. For each locally modified task: PUT the VTODO to the server.
     *  3. For tasks marked as deleted: DELETE the remote resource.
     */
    private suspend fun pushLocalChanges(
        account: CaldavAccount,
        caldavCalendar: CaldavCalendar,
        httpClient: OkHttpClient,
        httpUrl: HttpUrl,
    ) = withContext(Dispatchers.IO) {
        // Step 1: delete remote resources for tasks moved to a different calendar.
        for (movedTask in caldavDao.getMoved(caldavCalendar.uuid!!)) {
            deleteRemoteResource(httpClient, httpUrl, caldavCalendar, movedTask)
        }

        // Step 2 & 3: push every locally dirty task.
        for (task in taskDao.getCaldavTasksToPush(caldavCalendar.uuid!!)) {
            try {
                pushTask(caldavCalendar, task, httpClient, httpUrl)
            } catch (e: IOException) {
                LOG.e(e) { "IO error pushing task ${task.id}: ${e.message}" }
            } catch (e: HttpException) {
                LOG.e(e) { "HTTP ${e.code} pushing task ${task.id}: ${e.message}" }
            }
        }
    }

    /**
     * Push a single [task] to the CalDAV server.
     *
     * If the task is deleted locally, the remote resource is deleted (and the local CaldavTask row
     * is removed).  Otherwise the VTODO is serialized and PUT to the server.
     */
    private suspend fun pushTask(
        caldavCalendar: CaldavCalendar,
        task: Task,
        httpClient: OkHttpClient,
        httpUrl: HttpUrl,
    ) {
        val caldavTask = caldavDao.getTask(task.id) ?: return

        if (task.isDeleted) {
            deleteRemoteResource(httpClient, httpUrl, caldavCalendar, caldavTask)
            // Hard-delete the task from the local DB since it was explicitly deleted by the user.
            taskDao.update(task)  // leave task row; CaldavTask row is removed in deleteRemoteResource
            return
        }

        // Ensure we have an object filename.
        val objPath = ensureObjPath(caldavTask) ?: run {
            LOG.e { "Cannot push task ${task.id} — missing obj path" }
            return
        }

        // Load the cached VTODO so that any server-specific properties are preserved.
        val existingVtodo = vtodoCache.getVtodo(caldavCalendar, caldavTask)

        val vtodoBytes = DesktopVtodoConverter.toVtodo(task, caldavTask, existingVtodo)

        val remote = DavResource(
            httpClient = httpClient,
            location = httpUrl.newBuilder().addPathSegment(objPath).build(),
        )

        // The dav4jvm put() callback runs on the network thread (not a coroutine), so we
        // capture the response ETag synchronously inside the callback and do all suspend
        // work (cache write, DAO update) afterwards.
        var responseEtag: String? = null
        var uploadSucceeded = false
        remote.put(vtodoBytes.toRequestBody(contentType = MIME_ICALENDAR)) { response ->
            if (response.isSuccessful) {
                responseEtag = fromResponse(response)?.eTag?.takeIf { it.isNotBlank() }
                uploadSucceeded = true
            }
        }

        if (uploadSucceeded) {
            // Write the cached VTODO after the network call completes (suspend is safe here).
            vtodoCache.putVtodo(caldavCalendar, caldavTask, String(vtodoBytes))
        }

        val updatedCaldavTask = caldavTask.copy(
            etag = responseEtag ?: caldavTask.etag,
            lastSync = task.modificationDate,
        )
        caldavDao.update(updatedCaldavTask)
        LOG.d { "Pushed task ${task.id} → $objPath" }
    }

    /**
     * DELETE [caldavTask]'s remote resource, ignore 404 (already gone), and remove the
     * local [CaldavTask] row.
     *
     * @return true if the deletion was successful (or already absent on server).
     */
    private suspend fun deleteRemoteResource(
        httpClient: OkHttpClient,
        httpUrl: HttpUrl,
        calendar: CaldavCalendar,
        caldavTask: CaldavTask,
    ): Boolean {
        val objPath = ensureObjPath(caldavTask)
        if (!objPath.isNullOrBlank()) {
            try {
                val remote = DavResource(
                    httpClient = httpClient,
                    location = httpUrl.newBuilder().addPathSegment(objPath).build(),
                )
                remote.delete(null) {}
            } catch (e: HttpException) {
                if (e.code != 404) {
                    LOG.e(e) { "HTTP ${e.code} deleting $objPath" }
                    return false
                }
                // 404 = already gone, which is fine.
            } catch (e: IOException) {
                LOG.e(e) { "IO error deleting $objPath: ${e.message}" }
                return false
            }
        }

        vtodoCache.delete(calendar, caldavTask)
        caldavDao.delete(caldavTask)
        LOG.d { "Deleted remote resource $objPath" }
        return true
    }

    // ---------------------------------------------------------------------------
    // Utility
    // ---------------------------------------------------------------------------

    /**
     * Return the object filename (e.g. `"<uuid>.ics"`) for [caldavTask].
     * If [CaldavTask.obj] is null but [CaldavTask.remoteId] is present, the filename is
     * derived and saved back to the database.
     */
    private suspend fun ensureObjPath(caldavTask: CaldavTask): String? {
        if (!caldavTask.obj.isNullOrBlank()) return caldavTask.obj
        val derived = caldavTask.remoteId?.let { "$it.ics" } ?: return null
        caldavDao.update(caldavTask.copy(obj = derived))
        return derived
    }
}
