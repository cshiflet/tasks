package org.tasks.desktop.sync

import net.fortuna.ical4j.data.CalendarBuilder
import net.fortuna.ical4j.data.CalendarOutputter
import net.fortuna.ical4j.model.Calendar
import net.fortuna.ical4j.model.Component
import net.fortuna.ical4j.model.ComponentList
import net.fortuna.ical4j.model.DateTime
import net.fortuna.ical4j.model.Property
import net.fortuna.ical4j.model.PropertyList
import net.fortuna.ical4j.model.component.CalendarComponent
import net.fortuna.ical4j.model.component.VToDo
import net.fortuna.ical4j.model.property.CalScale
import net.fortuna.ical4j.model.property.Completed
import net.fortuna.ical4j.model.property.Description
import net.fortuna.ical4j.model.property.DtStamp
import net.fortuna.ical4j.model.property.Due
import net.fortuna.ical4j.model.property.LastModified
import net.fortuna.ical4j.model.property.ProdId
import net.fortuna.ical4j.model.property.Status
import net.fortuna.ical4j.model.property.Summary
import net.fortuna.ical4j.model.property.Uid
import net.fortuna.ical4j.model.property.Version
import net.fortuna.ical4j.model.parameter.RelType
import net.fortuna.ical4j.model.property.DtStart
import net.fortuna.ical4j.model.property.PercentComplete
import net.fortuna.ical4j.model.property.Priority
import net.fortuna.ical4j.model.property.RelatedTo
import net.fortuna.ical4j.model.property.RRule
import org.tasks.data.entity.CaldavTask
import org.tasks.data.entity.Task
import org.tasks.data.UUIDHelper
import java.io.ByteArrayOutputStream
import java.io.StringReader
import java.util.TimeZone

/**
 * Pure-JVM iCalendar conversion layer for the desktop CalDAV sync engine.
 *
 * This class intentionally has no Android dependencies and no Hilt injection — it is
 * instantiated directly by [DesktopCaldavSynchronizer].
 *
 * ### Priority mapping
 * iCalendar RFC 5545 §3.8.1.9:
 *   0 = undefined, 1-4 = high, 5 = medium, 6-9 = low.
 * Tasks.org internal:
 *   0 = HIGH, 1 = MEDIUM, 2 = LOW, 3 = NONE.
 *
 * ### Date handling
 * The Tasks.org date encoding stores dates as epoch-milliseconds with a special encoding:
 *   - If the last 60 seconds-worth of milliseconds are non-zero the date has a time component
 *     (URGENCY_SPECIFIC_DAY_TIME = 8), otherwise it is a day-only date (URGENCY_SPECIFIC_DAY = 7).
 * We reproduce the same logic used by [iCalendar] on Android so that tasks round-trip correctly.
 */
object DesktopVtodoConverter {

    private const val PROD_ID = "+//IDN tasks.org//desktop//EN"

    // ---- tasks.org urgency constants (mirrors Task.kt) -------------------------
    private const val URGENCY_SPECIFIC_DAY = 7
    private const val URGENCY_SPECIFIC_DAY_TIME = 8

    // ---- tasks.org hide-until constants ----------------------------------------
    private const val HIDE_UNTIL_SPECIFIC_DAY = 4
    private const val HIDE_UNTIL_SPECIFIC_DAY_TIME = 5

    // ---- iCalendar RRULE prefix ------------------------------------------------
    private const val RRULE_PREFIX = "RRULE:"

    // ---------------------------------------------------------------------------
    // Public API
    // ---------------------------------------------------------------------------

    /**
     * Parse a raw VCALENDAR/VTODO string and return a [ParsedVtodo] containing the
     * individual property values we care about.  Returns `null` if parsing fails or
     * there is no VTODO component inside the calendar object.
     */
    fun parse(vtodoString: String): ParsedVtodo? {
        return try {
            val builder = CalendarBuilder()
            val calendar = builder.build(StringReader(vtodoString))
            val vtodo = calendar.getComponent<VToDo>(Component.VTODO) ?: return null
            ParsedVtodo.from(vtodo)
        } catch (e: Exception) {
            null
        }
    }

    /**
     * Serialize a [Task] + [CaldavTask] pair into a VCALENDAR byte array ready to PUT
     * to a CalDAV server.
     *
     * If [existingVtodo] is supplied (from the local VTODO cache) it is used as the base
     * so that server-specific or client-unknown properties are preserved.  Otherwise a
     * fresh VTODO component is created.
     *
     * @param task            The Tasks.org [Task] entity.
     * @param caldavTask      The corresponding [CaldavTask] with remoteId / obj etc.
     * @param existingVtodo   Raw VCALENDAR string from the local cache (may be null).
     * @return                A UTF-8 encoded VCALENDAR byte array.
     */
    fun toVtodo(
        task: Task,
        caldavTask: CaldavTask,
        existingVtodo: String?,
    ): ByteArray {
        // Use an existing VTODO as the base to preserve unknown properties.
        val vtodo: VToDo = loadExistingVtodo(existingVtodo) ?: VToDo()

        applyTaskToVtodo(task, caldavTask, vtodo)

        @Suppress("UNCHECKED_CAST")
        val components = ComponentList<CalendarComponent>()
            .also { it.add(vtodo) }

        val calendar = Calendar(
            PropertyList<Property>().apply {
                add(ProdId(PROD_ID))
                add(Version.VERSION_2_0)
                add(CalScale.GREGORIAN)
            },
            components,
        )

        val out = ByteArrayOutputStream()
        CalendarOutputter(false).output(calendar, out)
        return out.toByteArray()
    }

    // ---------------------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------------------

    private fun loadExistingVtodo(vtodoString: String?): VToDo? {
        if (vtodoString.isNullOrBlank()) return null
        return try {
            val builder = CalendarBuilder()
            val calendar = builder.build(StringReader(vtodoString))
            calendar.getComponent(Component.VTODO)
        } catch (e: Exception) {
            null
        }
    }

    /**
     * Write all task fields into [vtodo], replacing existing properties where relevant.
     */
    private fun applyTaskToVtodo(task: Task, caldavTask: CaldavTask, vtodo: VToDo) {
        val props = vtodo.properties

        // UID — use the caldavTask remoteId; generate one if missing.
        val uid = caldavTask.remoteId?.takeIf { it.isNotBlank() } ?: UUIDHelper.newUUID()
        replaceProperty(props, Property.UID, Uid(uid))

        // SUMMARY
        replaceProperty(props, Property.SUMMARY, Summary(task.title ?: ""))

        // DESCRIPTION
        if (!task.notes.isNullOrBlank()) {
            replaceProperty(props, Property.DESCRIPTION, Description(task.notes))
        } else {
            props.removeIf { it.name == Property.DESCRIPTION }
        }

        // PRIORITY
        val icalPriority = taskPriorityToIcal(task.priority)
        replaceProperty(props, Property.PRIORITY, Priority(icalPriority))

        // DUE
        if (task.dueDate > 0) {
            replaceProperty(props, Property.DUE, duePropertyFor(task.dueDate))
        } else {
            props.removeIf { it.name == Property.DUE }
        }

        // DTSTART (hide-until / start date)
        if (task.hideUntil > 0) {
            replaceProperty(props, Property.DTSTART, dtStartPropertyFor(task.hideUntil))
        } else {
            props.removeIf { it.name == Property.DTSTART }
        }

        // STATUS + COMPLETED
        if (task.isCompleted) {
            replaceProperty(props, Property.STATUS, Status.VTODO_COMPLETED)
            replaceProperty(
                props,
                Property.COMPLETED,
                Completed(DateTime(task.completionDate))
            )
            replaceProperty(props, "PERCENT-COMPLETE", PercentComplete(100))
        } else {
            // Remove completion markers if the task is not done.
            props.removeIf {
                it.name == Property.STATUS ||
                    it.name == Property.COMPLETED ||
                    it.name == "PERCENT-COMPLETE"
            }
        }

        // LAST-MODIFIED
        replaceProperty(
            props,
            Property.LAST_MODIFIED,
            LastModified(DateTime(task.modificationDate))
        )

        // DTSTAMP (required by RFC 5545)
        replaceProperty(
            props,
            Property.DTSTAMP,
            DtStamp(DateTime(System.currentTimeMillis()))
        )

        // RELATED-TO (parent link — remoteParent holds the parent's UID)
        val parentUid = caldavTask.remoteParent
        if (!parentUid.isNullOrBlank()) {
            // Replace any existing PARENT-type RELATED-TO or add a new one.
            val existingParent = props.filterIsInstance<RelatedTo>().firstOrNull { rt ->
                val relType = rt.parameters.getParameter<RelType>(net.fortuna.ical4j.model.Parameter.RELTYPE)
                relType == null || relType == RelType.PARENT
            }
            if (existingParent != null) {
                existingParent.value = parentUid
            } else {
                props.add(RelatedTo(parentUid))
            }
        } else {
            props.removeIf { prop ->
                if (prop !is RelatedTo) return@removeIf false
                val relType = prop.parameters.getParameter<RelType>(net.fortuna.ical4j.model.Parameter.RELTYPE)
                relType == null || relType == RelType.PARENT
            }
        }

        // RRULE
        val recurrence = task.recurrence?.takeIf { it.isNotBlank() }
        if (recurrence != null) {
            val rruleValue = if (recurrence.startsWith(RRULE_PREFIX)) {
                recurrence.removePrefix(RRULE_PREFIX)
            } else {
                recurrence
            }
            try {
                val rrule = RRule(rruleValue)
                replaceProperty(props, Property.RRULE, rrule)
            } catch (e: Exception) {
                // Malformed RRULE — leave existing value in place or remove.
                if (props.getProperty<RRule>(Property.RRULE) == null) {
                    props.removeIf { it.name == Property.RRULE }
                }
            }
        } else {
            props.removeIf { it.name == Property.RRULE }
        }
    }

    // ---------------------------------------------------------------------------
    // Date helpers
    // ---------------------------------------------------------------------------

    /**
     * Convert a Tasks.org encoded due-date millis to a [Due] property.
     * If the timestamp has sub-minute precision it is treated as a date+time value.
     */
    private fun duePropertyFor(dueDateMillis: Long): Due {
        return if (Task.hasDueTime(dueDateMillis)) {
            Due(DateTime(dueDateMillis))
        } else {
            // Strip the time component — use a date-only value.
            Due(net.fortuna.ical4j.model.Date(stripTime(dueDateMillis)))
        }
    }

    private fun dtStartPropertyFor(hideUntilMillis: Long): DtStart {
        return if (Task.hasDueTime(hideUntilMillis)) {
            DtStart(DateTime(hideUntilMillis))
        } else {
            DtStart(net.fortuna.ical4j.model.Date(stripTime(hideUntilMillis)))
        }
    }

    /**
     * Strip the time-of-day from [millis], returning midnight of the same calendar day
     * in the default timezone.
     */
    private fun stripTime(millis: Long): Long {
        val cal = java.util.Calendar.getInstance(TimeZone.getDefault())
        cal.timeInMillis = millis
        cal.set(java.util.Calendar.HOUR_OF_DAY, 0)
        cal.set(java.util.Calendar.MINUTE, 0)
        cal.set(java.util.Calendar.SECOND, 0)
        cal.set(java.util.Calendar.MILLISECOND, 0)
        return cal.timeInMillis
    }

    // ---------------------------------------------------------------------------
    // Priority mapping
    // ---------------------------------------------------------------------------

    private fun taskPriorityToIcal(priority: Int): Int = when (priority) {
        Task.Priority.HIGH   -> 1   // iCal 1 = highest
        Task.Priority.MEDIUM -> 5   // iCal 5 = medium
        Task.Priority.LOW    -> 9   // iCal 9 = lowest
        else                 -> 0   // iCal 0 = undefined
    }

    private fun icalPriorityToTask(icalPriority: Int): Int = when {
        icalPriority == 0              -> Task.Priority.NONE
        icalPriority in 1..4           -> Task.Priority.HIGH
        icalPriority == 5              -> Task.Priority.MEDIUM
        icalPriority in 6..9           -> Task.Priority.LOW
        else                           -> Task.Priority.NONE
    }

    // ---------------------------------------------------------------------------
    // Property replacement helper
    // ---------------------------------------------------------------------------

    private fun replaceProperty(props: PropertyList<Property>, name: String, value: Property) {
        props.removeIf { it.name.equals(name, ignoreCase = true) }
        props.add(value)
    }

    // Convenience overload: derive the name from the property itself.
    private fun replaceProperty(props: PropertyList<Property>, value: Property) =
        replaceProperty(props, value.name, value)

    // ---------------------------------------------------------------------------
    // ParsedVtodo
    // ---------------------------------------------------------------------------

    /**
     * Value object holding the VTODO fields relevant to Tasks.org.
     * Conversion from raw ical4j types happens here so that [DesktopCaldavSynchronizer]
     * only works with plain Kotlin/JVM types.
     */
    data class ParsedVtodo(
        val uid: String?,
        val title: String?,
        val notes: String?,
        /** Tasks.org priority (0=HIGH, 1=MED, 2=LOW, 3=NONE). */
        val priority: Int,
        /** Epoch millis, or 0 if absent. Encoded per Tasks.org convention. */
        val dueDate: Long,
        /** Epoch millis, or 0 if absent. */
        val hideUntil: Long,
        /** Epoch millis, or 0 if not completed. */
        val completionDate: Long,
        /** Epoch millis of LAST-MODIFIED, or 0 if absent. */
        val lastModified: Long,
        /** UID of the parent task (from RELATED-TO with RELTYPE=PARENT). */
        val parentUid: String?,
        /** Raw RRULE string (without "RRULE:" prefix), or null. */
        val recurrence: String?,
    ) {
        companion object {
            fun from(vtodo: VToDo): ParsedVtodo {
                val props = vtodo.properties

                val uid = props.getProperty<Uid>(Property.UID)?.value

                val title = props.getProperty<Summary>(Property.SUMMARY)?.value

                val notes = props.getProperty<Description>(Property.DESCRIPTION)?.value
                    ?.takeIf { it.isNotBlank() }

                val icalPriority = props.getProperty<Priority>(Property.PRIORITY)
                    ?.level ?: 0
                val priority = DesktopVtodoConverter.icalPriorityToTask(icalPriority)

                val due = props.getProperty<Due>(Property.DUE)
                val dueDate = if (due != null) {
                    parseDueDate(due)
                } else {
                    0L
                }

                val dtStart = props.getProperty<DtStart>(Property.DTSTART)
                val hideUntil = if (dtStart != null) {
                    parseDtStart(dtStart)
                } else {
                    0L
                }

                val completedProp = props.getProperty<Completed>(Property.COMPLETED)
                val statusProp = props.getProperty<Status>(Property.STATUS)
                val completionDate = if (
                    completedProp != null ||
                    statusProp?.value == Status.VTODO_COMPLETED.value
                ) {
                    completedProp?.date?.time ?: System.currentTimeMillis()
                } else {
                    0L
                }

                val lastModified = props.getProperty<LastModified>(Property.LAST_MODIFIED)
                    ?.dateTime?.time ?: 0L

                // RELATED-TO with no RELTYPE parameter or RELTYPE=PARENT is treated as parent.
                val parentUid = props.getProperties<RelatedTo>(Property.RELATED_TO)
                    ?.filterIsInstance<RelatedTo>()
                    ?.firstOrNull { rt ->
                        val relType = rt.parameters
                            .getParameter<RelType>(net.fortuna.ical4j.model.Parameter.RELTYPE)
                        relType == null || relType == RelType.PARENT
                    }
                    ?.value

                val recurrence = props.getProperty<RRule>(Property.RRULE)
                    ?.let { "RRULE:${it.value}" }

                return ParsedVtodo(
                    uid = uid,
                    title = title,
                    notes = notes,
                    priority = priority,
                    dueDate = dueDate,
                    hideUntil = hideUntil,
                    completionDate = completionDate,
                    lastModified = lastModified,
                    parentUid = parentUid,
                    recurrence = recurrence,
                )
            }

            /**
             * Decode a [Due] property into the Tasks.org epoch-millis encoding.
             *
             * The Tasks.org encoding (from [org.tasks.data.createDueDate]):
             *   - DATE-only:      noon on that calendar day, seconds = 0  (hasDueTime == false)
             *   - DATE+TIME:      keep actual time, set seconds to 1      (hasDueTime == true)
             *
             * [Task.hasDueTime] checks `dueDate % 60000 > 0`, i.e. the seconds field is non-zero.
             */
            private fun parseDueDate(due: Due): Long {
                val date = due.date ?: return 0L
                return if (date is DateTime) {
                    createDueDateWithTime(date.time)
                } else {
                    createDueDateDayOnly(date.time)
                }
            }

            private fun parseDtStart(dtStart: DtStart): Long {
                val date = dtStart.date ?: return 0L
                return if (date is DateTime) {
                    createHideUntilWithTime(date.time)
                } else {
                    startOfDay(date.time)
                }
            }

            /**
             * Produce a Tasks.org due-date millis for a DATE+TIME value.
             * Strips milliseconds then sets seconds to 1 so [Task.hasDueTime] returns true.
             */
            private fun createDueDateWithTime(epochMillis: Long): Long {
                val cal = java.util.Calendar.getInstance(TimeZone.getDefault())
                cal.timeInMillis = epochMillis
                cal.set(java.util.Calendar.SECOND, 1)
                cal.set(java.util.Calendar.MILLISECOND, 0)
                return cal.timeInMillis
            }

            /**
             * Produce a Tasks.org due-date millis for a DATE-only value.
             * Sets time to noon with seconds = 0 so [Task.hasDueTime] returns false.
             */
            private fun createDueDateDayOnly(epochMillis: Long): Long {
                val cal = java.util.Calendar.getInstance(TimeZone.getDefault())
                cal.timeInMillis = epochMillis
                cal.set(java.util.Calendar.HOUR_OF_DAY, 12)
                cal.set(java.util.Calendar.MINUTE, 0)
                cal.set(java.util.Calendar.SECOND, 0)
                cal.set(java.util.Calendar.MILLISECOND, 0)
                return cal.timeInMillis
            }

            /**
             * Produce a Tasks.org hide-until millis for a DATE+TIME value.
             * Same convention as due-date: seconds = 1, ms = 0.
             */
            private fun createHideUntilWithTime(epochMillis: Long): Long {
                val cal = java.util.Calendar.getInstance(TimeZone.getDefault())
                cal.timeInMillis = epochMillis
                cal.set(java.util.Calendar.SECOND, 1)
                cal.set(java.util.Calendar.MILLISECOND, 0)
                return cal.timeInMillis
            }

            /** Midnight (start of day) for a DATE-only hide-until value. */
            private fun startOfDay(epochMillis: Long): Long {
                val cal = java.util.Calendar.getInstance(TimeZone.getDefault())
                cal.timeInMillis = epochMillis
                cal.set(java.util.Calendar.HOUR_OF_DAY, 0)
                cal.set(java.util.Calendar.MINUTE, 0)
                cal.set(java.util.Calendar.SECOND, 0)
                cal.set(java.util.Calendar.MILLISECOND, 0)
                return cal.timeInMillis
            }
        }
    }
}
