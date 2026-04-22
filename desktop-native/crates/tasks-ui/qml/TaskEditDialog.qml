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
        validation.text = "";
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
        vm.updateSelectedTask(
            titleField.text,
            notesField.text,
            dueField.text.trim(),
            hideField.text.trim(),
            priorityBox.currentIndex,
            uuid
        );
    }
}
