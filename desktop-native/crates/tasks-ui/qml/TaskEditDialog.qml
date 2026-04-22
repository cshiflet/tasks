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

        validation.text = "";
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
            }
            Label {
                Layout.fillWidth: true
                text: dialog.initialRecurrenceSummary.length > 0
                      ? dialog.initialRecurrenceSummary
                      : qsTr("(none — edit via Android for now)")
                opacity: 0.55
                elide: Text.ElideRight
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
        let uuid = "";
        if (calendarBox.currentIndex > 0) {
            uuid = vm.caldavCalendarUuids[calendarBox.currentIndex - 1];
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
        if (placeBox.currentIndex > 0 && vm) {
            placeUid = vm.placeUids[placeBox.currentIndex - 1];
        }
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
            departureBox.checked
        );
    }
}
