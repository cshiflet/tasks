// Right-hand detail view for the currently-selected task.
// All fields come from the view model's `selected*` Q_PROPERTYs, which
// update when `selectTask(id)` is invoked from the list pane.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

import com.tasks.desktop

Pane {
    id: root
    padding: 16
    // Belt-and-braces: each pane explicitly follows the OS theme.
    // The ApplicationWindow at the root sets the same value, but
    // Pane's own background-resolver sometimes falls back to its
    // (light) default if the attached-property chain is broken by
    // an intervening QObject — pin it here so this pane reliably
    // matches the rest of the window.
    Material.theme: Material.System
    Material.accent: Material.Blue
    required property QtObject vm

    // Wired to Main.qml's `Edit selected task…` action (F2).
    // Opens the same dialog the toolbar Edit… button does.
    function openEditForSelected() {
        if (!root.vm || root.vm.selectedId === 0) { return; }
        editButton.clicked();
    }
    // Wired to Main.qml's `Delete selected task` action (Delete).
    // Opens the two-step confirm; the user still has to click OK.
    function requestDelete() {
        if (!root.vm || root.vm.selectedId === 0) { return; }
        confirmDelete.open();
    }

    // Look up a label from a parallel-array (uids, labels) Q_PROPERTY pair.
    // Returns the empty string when not found, so the rendered Label can
    // bind directly to it.
    function _labelFor(uid, uids, labels) {
        if (!uid || !uids || !labels) { return ""; }
        for (let i = 0; i < uids.length; i++) {
            if (uids[i] === uid) { return labels[i] || ""; }
        }
        return "";
    }
    function _selectedListName() {
        if (!root.vm) { return ""; }
        return _labelFor(
            root.vm.selectedCaldavCalendarUuid,
            root.vm.caldavCalendarUuids,
            root.vm.caldavCalendarLabels);
    }
    function _selectedPlaceName() {
        if (!root.vm) { return ""; }
        return _labelFor(
            root.vm.selectedPlaceUid,
            root.vm.placeUids,
            root.vm.placeLabels);
    }
    function _selectedParentTitle() {
        if (!root.vm || root.vm.selectedParentId === 0) { return ""; }
        const ids = root.vm.parentCandidateIds;
        const labels = root.vm.parentCandidateLabels;
        for (let i = 0; i < ids.length; i++) {
            if (ids[i] === root.vm.selectedParentId) {
                return labels[i] || qsTr("(untitled task)");
            }
        }
        return "";
    }
    function _selectedTagNames() {
        if (!root.vm) { return []; }
        const out = [];
        const uids = root.vm.tagUids;
        const labels = root.vm.tagLabels;
        for (let i = 0; i < root.vm.selectedTagUids.length; i++) {
            const u = root.vm.selectedTagUids[i];
            for (let j = 0; j < uids.length; j++) {
                if (uids[j] === u) {
                    out.push(labels[j] || u);
                    break;
                }
            }
        }
        return out;
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 12
        visible: root.vm && root.vm.selectedId > 0

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            PriorityDot {
                priority: root.vm ? root.vm.selectedPriority : 3
            }
            Label {
                Layout.fillWidth: true
                text: root.vm ? root.vm.selectedTitle : ""
                font.pointSize: Qt.application.font.pointSize + 4
                font.bold: true
                wrapMode: Text.WordWrap
                font.strikeout: root.vm && root.vm.selectedCompleted
            }
        }

        // Metadata strip: list, parent, location, reminder count.
        // Each chip-like Label only renders if there's a value, so an
        // unannotated task collapses cleanly to nothing.
        Flow {
            Layout.fillWidth: true
            spacing: 8

            // CalDAV list pill.
            Label {
                visible: root._selectedListName().length > 0
                text: qsTr("📋 %1").arg(root._selectedListName())
                opacity: 0.75
                font.pointSize: Qt.application.font.pointSize - 1
                leftPadding: 6
                rightPadding: 6
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    color: Material.foreground
                    opacity: 0.08
                    radius: 4
                }
            }
            // Parent task pill.
            Label {
                visible: root._selectedParentTitle().length > 0
                text: qsTr("↳ %1").arg(root._selectedParentTitle())
                opacity: 0.75
                font.pointSize: Qt.application.font.pointSize - 1
                leftPadding: 6
                rightPadding: 6
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    color: Material.foreground
                    opacity: 0.08
                    radius: 4
                }
            }
            // Location pill (with arrival/departure trigger hint).
            Label {
                visible: root._selectedPlaceName().length > 0
                text: {
                    let trig = "";
                    if (root.vm && root.vm.selectedPlaceArrival && root.vm.selectedPlaceDeparture) {
                        trig = qsTr(" (arrival + departure)");
                    } else if (root.vm && root.vm.selectedPlaceArrival) {
                        trig = qsTr(" (arrival)");
                    } else if (root.vm && root.vm.selectedPlaceDeparture) {
                        trig = qsTr(" (departure)");
                    }
                    return qsTr("📍 %1%2").arg(root._selectedPlaceName()).arg(trig);
                }
                opacity: 0.75
                font.pointSize: Qt.application.font.pointSize - 1
                leftPadding: 6
                rightPadding: 6
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    color: Material.foreground
                    opacity: 0.08
                    radius: 4
                }
            }
            // Reminder count pill. ToolTip is bound through a
            // dedicated HoverHandler since Label has no `hovered`
            // property of its own.
            Label {
                id: reminderPill
                visible: root.vm && root.vm.selectedAlarmLabels.length > 0
                text: qsTr("⏰ %1").arg(root.vm ? root.vm.selectedAlarmLabels.length : 0)
                opacity: 0.75
                font.pointSize: Qt.application.font.pointSize - 1
                leftPadding: 6
                rightPadding: 6
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    color: Material.foreground
                    opacity: 0.08
                    radius: 4
                }
                HoverHandler { id: reminderHover }
                ToolTip.visible: reminderHover.hovered
                                 && root.vm
                                 && root.vm.selectedAlarmLabels.length > 0
                ToolTip.text: root.vm ? root.vm.selectedAlarmLabels.join("\n") : ""
            }
        }

        // Tags row — wraps to multiple lines when there are many.
        Flow {
            Layout.fillWidth: true
            spacing: 6
            visible: root._selectedTagNames().length > 0

            Repeater {
                model: root._selectedTagNames()
                Label {
                    required property string modelData
                    text: modelData
                    font.pointSize: Qt.application.font.pointSize - 1
                    leftPadding: 6
                    rightPadding: 6
                    topPadding: 2
                    bottomPadding: 2
                    background: Rectangle {
                        color: Material.color(Material.Blue, Material.Shade400)
                        opacity: 0.18
                        radius: 8
                    }
                }
            }
        }

        RowLayout {
            spacing: 8
            visible: root.vm && root.vm.selectedDueLabel.length > 0
            Label {
                text: qsTr("Due:")
                opacity: 0.6
            }
            Label {
                text: root.vm ? root.vm.selectedDueLabel : ""
            }
        }

        // Recurrence summary. The view model hands us a humanised
        // phrase ("Every other week on Mon, Wed") produced by
        // `tasks_core::recurrence::humanize_rrule`, already tagged
        // with "(from completion)" when `tasks.repeat_from` says so.
        // Rules we can't parse show through verbatim.
        RowLayout {
            spacing: 8
            visible: root.vm && root.vm.selectedRecurrence.length > 0
            Label {
                text: qsTr("Repeats:")
                opacity: 0.6
            }
            Label {
                Layout.fillWidth: true
                text: root.vm ? root.vm.selectedRecurrence : ""
                elide: Text.ElideRight
            }
        }

        Label {
            Layout.fillWidth: true
            Layout.fillHeight: true
            text: root.vm ? root.vm.selectedNotes : ""
            wrapMode: Text.Wrap
            textFormat: Text.MarkdownText
            visible: root.vm && root.vm.selectedNotes.length > 0
        }

        Item { Layout.fillHeight: true }

        // Bottom action row. Delete is a soft-delete — the row is
        // flagged `deleted = now_ms` and disappears from the active
        // list, but the record stays so a future "Trash" view or an
        // undo path can surface it again.
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            Item { Layout.fillWidth: true }
            Button {
                id: editButton
                text: qsTr("Edit…")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Change title, notes, due/hide dates, or priority")
                onClicked: {
                    if (!root.vm) {
                        return;
                    }
                    editDialog.initialTitle = root.vm.selectedTitle;
                    editDialog.initialNotes = root.vm.selectedNotes;
                    editDialog.initialDueText = root.vm.selectedDueLabel;
                    editDialog.initialHideUntilText =
                        root.vm.selectedHideUntilLabel;
                    editDialog.initialPriority = root.vm.selectedPriority;
                    editDialog.initialRecurrenceSummary =
                        root.vm.selectedRecurrence;
                    editDialog.initialCaldavUuid =
                        root.vm.selectedCaldavCalendarUuid;
                    editDialog.initialPlaceUid =
                        root.vm.selectedPlaceUid;
                    editDialog.initialPlaceArrival =
                        root.vm.selectedPlaceArrival;
                    editDialog.initialPlaceDeparture =
                        root.vm.selectedPlaceDeparture;
                    editDialog.initialParentId =
                        root.vm.selectedParentId;
                    editDialog.initialEstimatedText =
                        root.vm.selectedEstimatedText;
                    editDialog.initialElapsedText =
                        root.vm.selectedElapsedText;
                    editDialog.initialRecurrenceRaw =
                        root.vm.selectedRecurrenceRaw;
                    editDialog.initialRepeatFrom =
                        root.vm.selectedRepeatFrom;
                    editDialog.loadFromSelection();
                    editDialog.open();
                }
            }
            Button {
                text: qsTr("Delete")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Mark this task deleted (hides it from the active list)")
                onClicked: confirmDelete.open()
            }
        }
    }

    // The edit dialog is a top-level ApplicationWindow now, so it
    // manages its own position + size. No anchor to the pane.
    TaskEditDialog {
        id: editDialog
        vm: root.vm
    }

    // Two-step confirm for delete. Cheap insurance until an undo
    // stack lands.
    //
    // `implicitWidth` is pinned explicitly so the wrap-mode Label
    // inside doesn't circularly drive the Dialog's own implicit
    // width. Without that pin, Qt logs "Binding loop detected for
    // property 'implicitWidth'" every time the dialog opens.
    Dialog {
        id: confirmDelete
        anchors.centerIn: parent
        modal: true
        title: qsTr("Delete task?")
        standardButtons: Dialog.Cancel | Dialog.Ok
        implicitWidth: 360

        Label {
            width: parent.width
            text: qsTr("Remove “%1” from the active list?")
                  .arg(root.vm ? root.vm.selectedTitle : "")
            wrapMode: Text.Wrap
        }
        onAccepted: if (root.vm) root.vm.deleteSelectedTask()
    }

    // Empty-state column. Slightly larger title with a one-line
    // hint so an unselected pane reads as deliberately blank, not
    // broken.
    ColumnLayout {
        anchors.centerIn: parent
        spacing: 4
        visible: !root.vm || root.vm.selectedId === 0

        Label {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("No task selected")
            font.pointSize: Qt.application.font.pointSize + 2
            font.bold: true
            opacity: 0.55
        }
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Pick a row from the list to see its details here.")
            opacity: 0.45
            font.pointSize: Qt.application.font.pointSize - 1
        }
    }
}
