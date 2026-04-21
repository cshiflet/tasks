// Right-hand detail view for the currently-selected task.
// All fields come from the view model's `selected*` Q_PROPERTYs, which
// update when `selectTask(id)` is invoked from the list pane.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

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

        // Recurrence (RRULE). Shown verbatim — "FREQ=DAILY;INTERVAL=1"
        // isn't pretty, but hiding it entirely was worse. A Milestone 2
        // pass can port the Android client's RepeatRuleToString to
        // render "Every day" / "Every other Tuesday" etc.
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
                font.family: "monospace"
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
                text: qsTr("Delete")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Mark this task deleted (hides it from the active list)")
                onClicked: confirmDelete.open()
            }
        }
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
