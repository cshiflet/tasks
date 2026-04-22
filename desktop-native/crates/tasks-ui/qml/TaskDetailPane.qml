// Right-hand detail view for the currently-selected task.
// All fields come from the view model's `selected*` Q_PROPERTYs, which
// update when `selectTask(id)` is invoked from the list pane.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import com.tasks.desktop

Pane {
    id: root
    padding: 16
    required property QtObject vm

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

    TaskEditDialog {
        id: editDialog
        anchors.centerIn: parent
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

    Label {
        anchors.centerIn: parent
        visible: !root.vm || root.vm.selectedId === 0
        text: qsTr("Select a task to see details")
        opacity: 0.5
    }
}
