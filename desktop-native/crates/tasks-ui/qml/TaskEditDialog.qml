// Modal edit form for a single task.
//
// The caller (TaskDetailPane) seeds `initialTitle`, `initialNotes`,
// `initialDueText`, `initialHideUntilText`, and `initialPriority`
// from the view model's `selected*` properties, then calls `open()`.
// Saving calls `vm.updateSelectedTask(...)` — the Rust side
// re-parses the date text, writes the UPDATE, reloads the active
// filter, and refreshes the detail pane. Cancel discards without
// touching the DB.
//
// Window vs Dialog: this is a top-level ApplicationWindow, not a
// Controls.Dialog, so the OS window manager supplies native
// move/resize/close affordances for free. The previous Dialog-based
// version had a pinned implicitWidth/implicitHeight that cut off
// the lower half of the form on compact displays; the Window
// version wraps its content in a ScrollView and sets a reasonable
// minimum size, so the user can shrink or grow the window to fit.
//
// Recurrence editing is limited to FREQ / INTERVAL / BYDAY /
// repeat-from; COUNT / UNTIL round-trip unparsed and surface a
// footer warning when the incoming rule carries one.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ApplicationWindow {
    id: dialog
    title: qsTr("Edit task")
    // Sized so the common fields fit without scrolling on a
    // typical desktop; users can resize, since this is a real
    // window. Minimums keep the form legible on shrink.
    width: 620
    height: 680
    minimumWidth: 480
    minimumHeight: 420
    // Hide-on-close rather than destroy so the TaskDetailPane's
    // `TaskEditDialog { id: editDialog }` reference stays valid for
    // the next open.
    visible: false
    // Application-modal so the main window is blocked until the
    // edit finishes. The user can still move/resize this window,
    // which the old Dialog couldn't do.
    modality: Qt.ApplicationModal
    // `Qt.Dialog` + the visibility flags give the window manager
    // the "this is a dialog" hints (centered on open, no taskbar
    // entry on some desktops) without locking out move/resize.
    flags: Qt.Dialog | Qt.WindowTitleHint | Qt.WindowSystemMenuHint
           | Qt.WindowCloseButtonHint | Qt.WindowMinMaxButtonsHint
    // Match the main window's theme so a dark-mode desktop doesn't
    // pop a light edit window.
    Material.theme: Material.System
    Material.accent: Material.Blue

    // Compatibility shims so call sites written against the old
    // Dialog-based API (`editDialog.open()`) continue to work.
    function open() {
        show();
        raise();
        requestActivate();
    }
    function accept() {
        if (saveChanges()) {
            close();
        }
    }
    function reject() {
        close();
    }

    // H-2: keyboard shortcuts. Window-level Shortcut blocks rather
    // than per-control Keys handlers so they fire regardless of
    // which TextField has focus. ApplicationModal ensures these
    // only trigger when this window is active.
    Shortcut {
        sequence: "Escape"
        onActivated: dialog.reject()
    }
    Shortcut {
        sequences: ["Ctrl+Return", "Ctrl+Enter"]
        onActivated: dialog.accept()
    }

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
    // M-12: live tag filter. Bound to the filter TextField above
    // the Tags Flow; CheckBoxes hide themselves when their label
    // doesn't contain the substring (case-insensitive).
    property string tagFilter: ""

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

    // Outer layout: scroll view for the form + a pinned footer
    // holding validation + Cancel/Save buttons.
    ColumnLayout {
        id: outerLayout
        anchors.fill: parent
        anchors.margins: 12
        spacing: 10

        ScrollView {
            id: scroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            // Pin content width to the viewport so child RowLayouts
            // / GridLayouts don't blow out horizontally and produce
            // a phantom horizontal scrollbar alongside the vertical
            // one.
            contentWidth: availableWidth
            // Keep the vertical scrollbar pinned so it reserves its
            // width inside `availableWidth` — with the default
            // `AsNeeded` policy the bar is an overlay and sits on
            // top of the form, hiding the right edge of fields (the
            // UI#4 report). Horizontal bar stays off; we never want
            // that one.
            ScrollBar.vertical.policy: ScrollBar.AlwaysOn
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

            ColumnLayout {
                width: scroll.availableWidth
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
        // Notes field no longer has its own ScrollView — the outer
        // one handles vertical scrolling for the whole form. A
        // preferred height keeps the editor tall enough to be
        // useful on first open.
        TextArea {
            id: notesField
            Layout.fillWidth: true
            Layout.preferredHeight: 140
            wrapMode: TextArea.Wrap
            selectByMouse: true
            placeholderText: qsTr("Notes. Markdown renders in the detail pane.")
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 12
            rowSpacing: 8

            // ---------- Section: WHEN ----------
            Label {
                Layout.columnSpan: 2
                Layout.topMargin: 4
                text: qsTr("When")
                font.bold: true
                font.pointSize: Qt.application.font.pointSize - 1
                opacity: 0.55
            }

            Label {
                text: qsTr("Due")
                opacity: 0.7
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 4
                TextField {
                    id: dueField
                    Layout.fillWidth: true
                    placeholderText: qsTr("YYYY-MM-DD or YYYY-MM-DD HH:MM")
                }
                DatePickerButton {
                    target: dueField
                }
            }

            Label {
                text: qsTr("Hide until")
                opacity: 0.7
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 4
                TextField {
                    id: hideField
                    Layout.fillWidth: true
                    placeholderText: qsTr("Same format as Due; blank = visible now")
                }
                DatePickerButton {
                    target: hideField
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
                            // Two-letter abbreviations so Mon/Tue
                            // and Sat/Sun aren't visually identical
                            // (was M/T/W/T/F/S/S — ambiguous). RRULE
                            // emission stays MO,TU,WE,... regardless.
                            text: [qsTr("Mo"), qsTr("Tu"), qsTr("We"),
                                   qsTr("Th"), qsTr("Fr"), qsTr("Sa"),
                                   qsTr("Su")][index]
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

                // Only show when the incoming rule actually carries
                // a COUNT or UNTIL that this editor would silently
                // strip on save.
                Label {
                    visible: freqBox.currentIndex > 0
                             && /(^|;)(COUNT|UNTIL)=/.test(
                                 dialog.initialRecurrenceRaw)
                    text: qsTr("Note: saving will drop the COUNT / UNTIL on the existing rule.")
                    color: Material.color(Material.Red)
                    opacity: 0.9
                    font.pointSize: Qt.application.font.pointSize - 1
                    wrapMode: Text.Wrap
                }
            }

            // ---------- Section: WHAT ----------
            Label {
                Layout.columnSpan: 2
                Layout.topMargin: 12
                text: qsTr("What")
                font.bold: true
                font.pointSize: Qt.application.font.pointSize - 1
                opacity: 0.55
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
                // Index 0 is the synthetic "Local task" entry
                // (represents both "local task" and "don't change").
                // Indices 1..N map to vm.caldavCalendarUuids[i-1].
                model: {
                    const labels = dialog.vm ? dialog.vm.caldavCalendarLabels : [];
                    const out = [qsTr("Local task — no CalDAV list")];
                    for (let i = 0; i < labels.length; i++) {
                        out.push(labels[i]);
                    }
                    return out;
                }
                enabled: dialog.vm && dialog.vm.caldavCalendarLabels.length > 0
            }

            Label {
                text: qsTr("Tags")
                opacity: 0.7
                Layout.alignment: Qt.AlignTop
            }
            // Tags column: a small filter TextField above a
            // scrollable Flow of CheckBoxes (M-12 — large tag lists
            // were previously a treasure hunt in a 90 px scroll).
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                TextField {
                    id: tagFilterField
                    Layout.fillWidth: true
                    placeholderText: qsTr("Filter tags…")
                    visible: dialog.vm && dialog.vm.tagUids.length > 6
                    onTextChanged: dialog.tagFilter = text
                }
                ScrollView {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 140
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
                                visible: {
                                    if (dialog.tagFilter.length === 0) { return true; }
                                    const t = (dialog.vm
                                        ? dialog.vm.tagLabels[index]
                                        : "").toLowerCase();
                                    return t.indexOf(dialog.tagFilter.toLowerCase()) >= 0;
                                }
                            }
                        }
                    }
                }
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

            // ---------- Section: ALERTS ----------
            Label {
                Layout.columnSpan: 2
                Layout.topMargin: 12
                text: qsTr("Alerts")
                font.bold: true
                font.pointSize: Qt.application.font.pointSize - 1
                opacity: 0.55
            }

            Label {
                text: qsTr("Reminders")
                opacity: 0.7
                Layout.alignment: Qt.AlignTop
            }
            // M-5: reminders are minutes-relative-to-due; without a
            // due date there's nothing to count from. Disable the
            // whole section + show a hint.
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                enabled: dueField.text.trim().length > 0

                Label {
                    visible: dueField.text.trim().length === 0
                    text: qsTr("Set a due date to add reminders.")
                    opacity: 0.55
                    font.pointSize: Qt.application.font.pointSize - 1
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }

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

            // ---------- Section: TRACKING ----------
            Label {
                Layout.columnSpan: 2
                Layout.topMargin: 12
                text: qsTr("Tracking")
                font.bold: true
                font.pointSize: Qt.application.font.pointSize - 1
                opacity: 0.55
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
                // Closes the inner GridLayout that started near the
                // top of the form (the one that holds Due / Hide /
                // Priority / Estimate / Elapsed / Parent / List /
                // Tags / Location / Reminders / Repeats).
            }
                // Closes the inner ColumnLayout that the form lives
                // in (child of the ScrollView).
            }
        }

        // Shows the Rust-side parse error ("Due: month out of range")
        // after a failed Save so the user can correct and retry
        // without re-opening the dialog. Lives outside the ScrollView
        // so it always stays visible at the bottom of the window.
        Label {
            id: validation
            Layout.fillWidth: true
            color: Material.color(Material.Red)
            wrapMode: Text.Wrap
            visible: text.length > 0
        }

        // Footer: Cancel on the left (flat, secondary action),
        // Save on the right (highlighted primary). Mirrors the
        // previous Dialog.standardButtons ordering.
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            Item { Layout.fillWidth: true }
            Button {
                text: qsTr("Cancel")
                flat: true
                onClicked: dialog.reject()
            }
            Button {
                text: qsTr("Save")
                highlighted: true
                onClicked: dialog.accept()
            }
        }
    }

    // Called by the footer Save button (and by `accept()` from the
    // compatibility shim at the top). Returns true on success so
    // the caller knows whether to close the window; callers that
    // want to keep the window open on validation failure can act on
    // the return value.
    function saveChanges() {
        if (!vm) {
            return false;
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
