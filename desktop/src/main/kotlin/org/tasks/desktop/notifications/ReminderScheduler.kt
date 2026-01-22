package org.tasks.desktop.notifications

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.tasks.data.dao.AlarmDao
import org.tasks.data.dao.TaskDao
import org.tasks.data.entity.Alarm
import org.tasks.kmp.formatDateTime
import org.tasks.kmp.org.tasks.time.DateStyle
import org.tasks.time.DateTimeUtils2.currentTimeMillis

class ReminderScheduler(
    private val alarmDao: AlarmDao,
    private val taskDao: TaskDao,
    private val systemTrayManager: SystemTrayManager?,
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var schedulerJob: Job? = null
    private val pendingReminders = mutableSetOf<Long>() // Track shown reminders

    // Check for reminders every minute
    private val checkInterval = 60_000L

    fun start() {
        schedulerJob?.cancel()
        schedulerJob = scope.launch {
            while (isActive) {
                checkReminders()
                delay(checkInterval)
            }
        }
    }

    fun stop() {
        schedulerJob?.cancel()
        schedulerJob = null
    }

    private suspend fun checkReminders() {
        val now = currentTimeMillis()
        val upcomingAlarms = alarmDao.getActiveAlarms()

        for (alarm in upcomingAlarms) {
            val alarmTime = calculateAlarmTime(alarm)
            if (alarmTime != null && alarmTime <= now && alarm.id !in pendingReminders) {
                triggerReminder(alarm)
                pendingReminders.add(alarm.id)
            }
        }

        // Clean up old reminders from tracking set
        pendingReminders.removeIf { alarmId ->
            val alarm = upcomingAlarms.find { it.id == alarmId }
            alarm == null || (calculateAlarmTime(alarm) ?: 0) < now - 3600_000 // 1 hour ago
        }
    }

    private fun calculateAlarmTime(alarm: Alarm): Long? {
        return when (alarm.type) {
            Alarm.TYPE_DATE_TIME -> alarm.time
            Alarm.TYPE_REL_START, Alarm.TYPE_REL_END -> {
                // These need task info to calculate - simplified for now
                alarm.time.takeIf { it > 0 }
            }
            Alarm.TYPE_SNOOZE -> alarm.time
            else -> null
        }
    }

    private suspend fun triggerReminder(alarm: Alarm) {
        val task = taskDao.fetch(alarm.task) ?: return

        // Don't show reminders for completed/deleted tasks
        if (task.isCompleted || task.isDeleted) return

        val dueTime = if (task.hasDueDate()) {
            formatDateTime(task.dueDate, DateStyle.MEDIUM)
        } else null

        systemTrayManager?.showTaskReminder(
            taskTitle = task.title ?: "Task Reminder",
            dueTime = dueTime
        )
    }

    /**
     * Schedule a reminder for a specific time.
     */
    fun scheduleReminder(taskId: Long, time: Long) {
        scope.launch(Dispatchers.IO) {
            // Check if alarm already exists
            val existingAlarms = alarmDao.getAlarms(taskId)
            val hasExisting = existingAlarms.any {
                it.type == Alarm.TYPE_DATE_TIME && it.time == time
            }

            if (!hasExisting) {
                alarmDao.insert(
                    Alarm(
                        task = taskId,
                        time = time,
                        type = Alarm.TYPE_DATE_TIME,
                    )
                )
            }
        }
    }

    /**
     * Cancel all reminders for a task.
     */
    fun cancelReminders(taskId: Long) {
        scope.launch(Dispatchers.IO) {
            val alarms = alarmDao.getAlarms(taskId)
            alarms.forEach { alarm ->
                pendingReminders.remove(alarm.id)
            }
            // Note: Actual deletion would need to be added to AlarmDao
        }
    }
}
