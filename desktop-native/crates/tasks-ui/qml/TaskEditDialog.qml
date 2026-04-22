// Modal edit form for a single task.
//
// The caller (TaskDetailPane) seeds `initialTitle`, `initialNotes`,
// `initialDueText`, `initialHideUntilText`, and `initialPriority`
// from the view model's `selected*` properties, then opens the
// dialog. Saving calls `vm.updateSelectedTask(...)` — the Rust side
// re-parses the date text, writes the UPDATE, reloads the active
// filter, and refreshes the detail pane. Cancel discards without
// touching the DB.
//
// Recurrence is intentionally read-only in this dialog: editing a
// repeat rule properly needs its own FREQ/BYDAY picker, which
// belongs in a follow-up. The summary field below is a reminder of
// what's currently set, not an editable field.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

Dialog {
    id: dialog
    modal: true
    title: qsTr("Edit task")
    standardButtons: Dialog.Cancel | Dialog.Save

    // Pin an explicit size so the wrap-mode TextArea inside doesn't
    // drive the dialog's own implicit size (the binding-loop trap
    // that bit the confirm-delete dialog earlier).
    implicitWidth: 520
    implicitHeight: 520

    // View model handle, passed in from TaskDetailPane.
    required property QtObject vm

    // Seed values. Assigned just before `open()` by the caller so
    // every open starts from the currently-selected task's state.
    property string initialTitle: ""
    property string initialNotes: ""
    property string initialDueText: ""
    property string initialHideUntilText: ""
    property int initialPriority: 3 // Priority.NONE
    property string initialRecurrenceSummary: ""
    property string initialCaldavUuid: ""
    // Tag UIDs attached to the selected task at dialog-open time.
    // The tag CheckBoxes bind their checked state to membership in
    // this array (pre-check); the array is mutated as the user
    // toggles so the selection can be read back on save.
    property var selectedTagUids: []
    // Working copy of the task's alarm list. Each entry is
    // `{time: Number, type: Number, label: String}`. `label` is
    // rendered in the UI; `time`+`type` are sent to the bridge on
    // save. Mutated by the [+ Add before due] row and the per-row
    // delete buttons. Kept as an array of plain JS objects so QML
    // Repeater sees a stable model.
    property var workingAlarms: []
    property string initialPlaceUid: ""
    property bool initialPlaceArrival: false
    property bool initialPlaceDeparture: false
    property var initialParentId: 0
    property string initialEstimatedText: ""
    property string initialElapsedText: ""
    // Raw RRULE string (not humanised) that the recurrence editor
    // round-trips. Empty = no recurrence. The editor's controls
    // are seeded from this in loadFromSelection() and the save
    // path rebuilds an RRULE from them.
    property string initialRecurrenceRaw: ""
    property int initialRepeatFrom: 0

    // Called by TaskDetailPane right before `open()`. Resets the
    // form controls to the incoming values and clears any stale
    // validation state.
    function loadFromSelection() {
        titleField.text = initialTitle;
        notesField.text = initialNotes;
        dueField.text = initialDueText;
        hideField.text = initialHideUntilText;
        // Priority maps 0=HIGH … 3=NONE, matching ComboBox's index.
        priorityBox.currentIndex = initialPriority;
        // List picker: find the current UUID's index in the parallel
        // arrays. -1 (not found) becomes 0; if the task has no
        // caldav row we fall back to "(no CalDAV list)" at index 0.
        if (vm) {
            const idx = vm.caldavCalendarUuids.indexOf(initialCaldavUuid);
            // +1 for the "(no CalDAV list)" prepended at index 0.
            calendarBox.currentIndex = idx >= 0 ? idx + 1 : 0;
        } else {
            calendarBox.currentIndex = 0;
        }
        // Seed the mutable selection from the VM's current tag set.
        // Copy (rather than alias) so toggling inside the dialog
        // doesn't mutate the VM's QStringList.
        selectedTagUids = vm ? Array.from(vm.selectedTagUids) : [];

        // Snapshot alarms. Pair labels with (time, type) by index;
        // the bridge populates all three arrays in lockstep.
        const alarms = [];
        if (vm) {
            const n = Math.min(
                vm.selectedAlarmLabels.length,
                vm.selectedAlarmTimes.length,
                vm.selectedAlarmTypes.length);
            for (let i = 0; i < n; i++) {
                alarms.push({
                    time: vm.selectedAlarmTimes[i],
                    type: vm.selectedAlarmTypes[i],
                    label: vm.selectedAlarmLabels[i],
                });
            }
        }
        workingAlarms = alarms;

        addReminderField.text = "";

        // Location: map current place UID to its index in the
        // parallel arrays; 0 = "(none)".
        if (vm) {
            const pi = vm.placeUids.indexOf(initialPlaceUid);
            placeBox.currentIndex = pi >= 0 ? pi + 1 : 0;
        } else {
            placeBox.currentIndex = 0;
        }
        arrivalBox.checked = initialPlaceArrival;
        departureBox.checked = initialPlaceDeparture;

        // Parent picker: index 0 = "(none)", 1..N map onto
        // vm.parentCandidateIds[i-1].
        if (vm) {
            // parentCandidateIds is a QList<i64>; QML's indexOf
            // works on it because it quacks like an Array.
            const pi = vm.parentCandidateIds.indexOf(initialParentId);
            parentBox.currentIndex = pi >= 0 ? pi + 1 : 0;
        } else {
            parentBox.currentIndex = 0;
        }

        estimateField.text = initialEstimatedText;
        elapsedField.text = initialElapsedText;

        loadRecurrenceFromRaw(initialRecurrenceRaw);
        fromCompletionBox.currentIndex = initialRepeatFrom === 1 ? 1 : 0;

        validation.text = "";
    }

    // Parse the raw RRULE into the editor's controls. Mirrors the
    // (simpler) subset that buildRecurrenceRule() writes back:
    // FREQ, INTERVAL, BYDAY. UNTIL / COUNT / BYMONTHDAY etc. are
    // parsed-but-dropped because the editor doesn't offer them;
    // saving an untouched recurrence would silently strip them.
    // That's documented in the dialog footer label.
    function loadRecurrenceFromRaw(rule) {
        freqBox.currentIndex = 0; // None
        intervalField.text = "1";
        for (let i = 0; i < 7; i++) {
            byDayBoxes.itemAt(i).checked = false;
        }
        if (!rule || rule.length === 0) {
            return;
        }
        const body = rule.startsWith("RRULE:") ? rule.slice(6) : rule;
        const parts = body.split(";");
        for (const part of parts) {
            const eq = part.indexOf("=");
            if (eq < 0) { continue; }
            const key = part.slice(0, eq).trim();
            const val = part.slice(eq + 1).trim();
            if (key === "FREQ") {
                const map = {
                    "DAILY": 1, "WEEKLY": 2, "MONTHLY": 3, "YEARLY": 4
                };
                freqBox.currentIndex = map[val] ?? 0;
            } else if (key === "INTERVAL") {
                intervalField.text = val;
            } else if (key === "BYDAY") {
                const days = val.split(",");
                const idx = { "MO": 0, "TU": 1, "WE": 2, "TH": 3, "FR": 4, "SA": 5, "SU": 6 };
                for (const d of days) {
                    // Strip leading sign/digit (positional prefix).
                    const code = d.replace(/^[-+]?\d+/, "");
                    if (idx[code] !== undefined) {
                        byDayBoxes.itemAt(idx[code]).checked = true;
                    }
                }
            }
        }
    }

    function buildRecurrenceRule() {
        const freqNames = ["", "DAILY", "WEEKLY", "MONTHLY", "YEARLY"];
        const freq = freqNames[freqBox.currentIndex] || "";
        if (freq === "") { return ""; }
        let rule = "FREQ=" + freq;
        const interval = parseInt(intervalField.text, 10);
        if (Number.isFinite(interval) && interval > 1) {
            rule += ";INTERVAL=" + interval;
        }
        if (freq === "WEEKLY") {
            const codes = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
            const picked = [];
            for (let i = 0; i < 7; i++) {
                if (byDayBoxes.itemAt(i).checked) { picked.push(codes[i]); }
            }
            if (picked.length > 0) {
                rule += ";BYDAY=" + picked.join(",");
            }
        }
        return rule;
    }

    function removeAlarmAt(index) {
        const next = workingAlarms.slice();
        next.splice(index, 1);
        workingAlarms = next;
    }

    function addBeforeDueMinutes(minutesText) {
        const m = parseInt(minutesText, 10);
        if (!Number.isFinite(m) || m < 0) {
            validation.text = qsTr("Minutes must be a non-negative number");
            return false;
        }
        // REL_END = 2; negative time = before.
        const timeMs = -m * 60 * 1000;
        const label = (m === 0)
            ? qsTr("At due")
            : qsTr("%1 minutes before due").arg(m);
        workingAlarms = workingAlarms.concat([{
            time: timeMs, type: 2, label: label
        }]);
        validation.text = "";
        return true;
    }

    function toggleTag(uid, nowChecked) {
        const i = selectedTagUids.indexOf(uid);
        if (nowChecked && i < 0) {
            selectedTagUids = selectedTagUids.concat([uid]);
        } else if (!nowChecked && i >= 0) {
            const next = selectedTagUids.slice();
            next.splice(i, 1);
            selectedTagUids = next;
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 10

        Label {
            text: qsTr("Title")
            opacity: 0.7
        }
        TextField {
            id: titleField
            Layout.fillWidth: true
            placeholderText: qsTr("Task title")
        }

        Label {
            text: qsTr("Notes (Markdown)")
            opacity: 0.7
        }
        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: 100

            TextArea {
                id: notesField
                wrapMode: TextArea.Wrap
                selectByMouse: true
                placeholderText: qsTr("Notes. Markdown renders in the detail pane.")
            }
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 12
            rowSpacing: 8

            Label {
                text: qsTr("Due")
                opacity: 0.7
            }
            TextField {
                id: dueField
                Layout.fillWidth: true
                placeholderText: qsTr("YYYY-MM-DD or YYYY-MM-DD HH:MM")
            }

            Label {
                text: qsTr("Hide until")
                opacity: 0.7
            }
            TextField {
                id: hideField
                Layout.fillWidth: true
                placeholderText: qsTr("Same format as Due; blank = visible now")
            }

            Label {
                text: qsTr("Priority")
                opacity: 0.7
            }
            ComboBox {
                id: priorityBox
                Layout.fillWidth: true
                // Index → tasks_core::models::Priority integer:
                //   0 = HIGH, 1 = MEDIUM, 2 = LOW, 3 = NONE
                model: [qsTr("High"), qsTr("Medium"), qsTr("Low"), qsTr("None")]
            }

            Label {
                text: qsTr("Estimate")
                opacity: 0.7
            }
            TextField {
                id: estimateField
                Layout.fillWidth: true
                placeholderText: qsTr("H:MM (e.g. 0:30, 1:15)")
            }

            Label {
                text: qsTr("Elapsed")
                opacity: 0.7
            }
            TextField {
                id: elapsedField
                Layout.fillWidth: true
                placeholderText: qsTr("H:MM")
            }

            Label {
                text: qsTr("Parent task")
                opacity: 0.7
            }
            ComboBox {
                id: parentBox
                Layout.fillWidth: true
                // Index 0 = top-level (no parent); 1..N map onto
                // parentCandidateIds.
                model: {
                    const labels = dialog.vm ? dialog.vm.parentCandidateLabels : [];
                    const out = [qsTr("(none — top-level)")];
                    for (let i = 0; i < labels.length; i++) {
                        out.push(labels[i] || qsTr("(untitled task)"));
                    }
                    return out;
                }
            }

            Label {
                text: qsTr("List")
                opacity: 0.7
            }
            ComboBox {
                id: calendarBox
                Layout.fillWidth: true
                // Index 0 is the synthetic "(no CalDAV list)" entry
                // (represents both "local task" and "don't change").
                // Indices 1..N map to vm.caldavCalendarUuids[i-1].
                model: {
                    const labels = dialog.vm ? dialog.vm.caldavCalendarLabels : [];
                    const out = [qsTr("(no CalDAV list)")];
                    for (let i = 0; i < labels.length; i++) {
                        out.push(labels[i]);
                    }
                    return out;
                }
                // If the DB has no CalDAV calendars, the picker is
                // still functional (single "(no list)" entry) but
                // offers no real choice — hide it to reduce clutter.
                enabled: dialog.vm && dialog.vm.caldavCalendarLabels.length > 0
            }

            Label {
                text: qsTr("Tags")
                opacity: 0.7
                Layout.alignment: Qt.AlignTop
            }
            // Scrollable flow of per-tag CheckBoxes. Each CheckBox's
            // checked state mirrors membership in selectedTagUids;
            // toggling mutates the array via toggleTag() so save
            // reads the final set straight out.
            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: 90
                clip: true

                Flow {
                    width: parent.width
                    spacing: 8

                    Repeater {
                        model: dialog.vm ? dialog.vm.tagUids.length : 0
                        CheckBox {
                            required property int index
                            text: dialog.vm ? dialog.vm.tagLabels[index] : ""
                            checked: dialog.selectedTagUids.indexOf(
                                dialog.vm.tagUids[index]) >= 0
                            onToggled: dialog.toggleTag(
                                dialog.vm.tagUids[index], checked)
                        }
                    }
                }
            }

            Label {
                text: qsTr("Location")
                opacity: 0.7
                Layout.alignment: Qt.AlignTop
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                ComboBox {
                    id: placeBox
                    Layout.fillWidth: true
                    model: {
                        const labels = dialog.vm ? dialog.vm.placeLabels : [];
                        const out = [qsTr("(no location)")];
                        for (let i = 0; i < labels.length; i++) {
                            out.push(labels[i]);
                        }
                        return out;
                    }
                }
                RowLayout {
                    enabled: placeBox.currentIndex > 0
                    spacing: 16
                    CheckBox {
                        id: arrivalBox
                        text: qsTr("Arrival")
                    }
                    CheckBox {
                        id: departureBox
                        text: qsTr("Departure")
                    }
                }
            }

            Label {
                text: qsTr("Reminders")
                opacity: 0.7
                Layout.alignment: Qt.AlignTop
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                Repeater {
                    model: dialog.workingAlarms.length
                    RowLayout {
                        required property int index
                        Layout.fillWidth: true
                        spacing: 8
                        Label {
                            Layout.fillWidth: true
                            text: dialog.workingAlarms[index].label
                            elide: Text.ElideRight
                        }
                        Button {
                            text: qsTr("Remove")
                            flat: true
                            onClicked: dialog.removeAlarmAt(index)
                        }
                    }
                }

                // Quick-add row. Tasks.org's full picker covers every
                // alarm type (relative/absolute/random/snooze) — the
                // desktop dialog starts with the most common case
                // (minutes before due) and grows from there.
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8
                    Label {
                        text: qsTr("Add:")
                        opacity: 0.6
                    }
                    TextField {
                        id: addReminderField
                        Layout.fillWidth: true
                        placeholderText: qsTr("minutes before due")
                        inputMethodHints: Qt.ImhDigitsOnly
                        onAccepted: {
                            if (dialog.addBeforeDueMinutes(text)) {
                                text = "";
                            }
                        }
                    }
                    Button {
                        text: qsTr("Add")
                        onClicked: {
                            if (dialog.addBeforeDueMinutes(addReminderField.text)) {
                                addReminderField.text = "";
                            }
                        }
                    }
                }
            }

            Label {
                text: qsTr("Repeats")
                opacity: 0.7
                Layout.alignment: Qt.AlignTop
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                RowLayout {
                    spacing: 8
                    ComboBox {
                        id: freqBox
                        Layout.fillWidth: true
                        model: [
                            qsTr("Never"),
                            qsTr("Daily"),
                            qsTr("Weekly"),
                            qsTr("Monthly"),
                            qsTr("Yearly"),
                        ]
                    }
                    Label {
                        text: qsTr("every")
                        opacity: 0.6
                        visible: freqBox.currentIndex > 0
                    }
                    TextField {
                        id: intervalField
                        visible: freqBox.currentIndex > 0
                        implicitWidth: 60
                        text: "1"
                        inputMethodHints: Qt.ImhDigitsOnly
                        horizontalAlignment: TextInput.AlignRight
                    }
                }

                // Weekly-only weekday picker.
                RowLayout {
                    spacing: 4
                    visible: freqBox.currentIndex === 2 // WEEKLY
                    Repeater {
                        id: byDayBoxes
                        model: 7
                        CheckBox {
                            required property int index
                            // Standard English weekday abbrev; the
                            // RRULE always emits MO,TU,WE,... so the
                            // mapping is stable even if the label is
                            // translated.
                            text: [qsTr("M"), qsTr("T"), qsTr("W"),
                                   qsTr("T"), qsTr("F"), qsTr("S"),
                                   qsTr("S")][index]
                        }
                    }
                }

                RowLayout {
                    visible: freqBox.currentIndex > 0
                    spacing: 8
                    Label {
                        text: qsTr("from")
                        opacity: 0.6
                    }
                    ComboBox {
                        id: fromCompletionBox
                        model: [qsTr("Due date"), qsTr("Completion")]
                    }
                }

                Label {
                    visible: freqBox.currentIndex > 0
                    text: qsTr("(COUNT/UNTIL rules from Android are dropped on save)")
                    opacity: 0.5
                    font.pointSize: Qt.application.font.pointSize - 1
                }
            }
        }

        // Shows the Rust-side parse error ("Due: month out of range")
        // after a failed Save so the user can correct and retry
        // without re-opening the dialog.
        Label {
            id: validation
            Layout.fillWidth: true
            color: Material.color(Material.Red)
            wrapMode: Text.Wrap
            visible: text.length > 0
        }
    }

    onAccepted: {
        if (!vm) {
            return;
        }
        // The Rust side re-parses and will refuse malformed dates;
        // the dialog closes optimistically here. If save fails the
        // status line surfaces the error. (Inline validation would
        // require duplicating the parser in QML, which is not worth
        // the rot risk for a one-shot dialog.)
        // Map the ComboBox selection back to a UUID. Index 0 is the
        // "(no CalDAV list)" synthetic entry — passing an empty
        // string tells the bridge not to touch caldav_tasks, which
        // matches "local task, leave it that way" semantics.
        // Defensive array bounds: even with the `currentIndex > 0`
        // guard, a stale ComboBox selection from before the model
        // changed could point past the end. `?? ""` papers over
        // undefined rather than relying on the bridge to surface
        // the bug as a cryptic status-line error.
        let uuid = "";
        const calIdx = calendarBox.currentIndex - 1;
        if (calIdx >= 0 && calIdx < vm.caldavCalendarUuids.length) {
            uuid = vm.caldavCalendarUuids[calIdx] ?? "";
        }
        // Unpack workingAlarms into parallel time/type arrays; the
        // bridge's Q_INVOKABLE can't take a JS object array across
        // FFI, so the two arrays travel separately and get zipped
        // back together on the Rust side.
        const alarmTimes = [];
        const alarmTypes = [];
        for (const a of workingAlarms) {
            alarmTimes.push(a.time);
            alarmTypes.push(a.type);
        }
        // Map the location picker back to a place UID.  Index 0 =
        // "(no location)" → empty string, which the bridge treats
        // as "clear this task's geofence".
        let placeUid = "";
        const placeIdx = placeBox.currentIndex - 1;
        if (placeIdx >= 0 && placeIdx < vm.placeUids.length) {
            placeUid = vm.placeUids[placeIdx] ?? "";
        }
        // Map the parent picker back to a task id. Index 0 = top-
        // level (id 0). The vm.parentCandidateIds array drops the
        // self-task, so any non-zero index is a valid other task.
        let parentId = 0;
        const parIdx = parentBox.currentIndex - 1;
        if (parIdx >= 0 && parIdx < vm.parentCandidateIds.length) {
            parentId = vm.parentCandidateIds[parIdx] ?? 0;
        }
        const rule = buildRecurrenceRule();
        // 0 = from due date, 1 = from completion — matches the
        // tasks.repeat_from column values.
        const repeatFrom = fromCompletionBox.currentIndex === 1 ? 1 : 0;
        vm.updateSelectedTask(
            titleField.text,
            notesField.text,
            dueField.text.trim(),
            hideField.text.trim(),
            priorityBox.currentIndex,
            uuid,
            selectedTagUids,
            alarmTimes,
            alarmTypes,
            placeUid,
            arrivalBox.checked,
            departureBox.checked,
            parentId,
            estimateField.text.trim(),
            elapsedField.text.trim(),
            rule,
            repeatFrom
        );
    }
}
