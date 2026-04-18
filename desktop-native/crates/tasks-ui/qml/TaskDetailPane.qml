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

        Label {
            Layout.fillWidth: true
            Layout.fillHeight: true
            text: root.vm ? root.vm.selectedNotes : ""
            wrapMode: Text.Wrap
            textFormat: Text.MarkdownText
            visible: root.vm && root.vm.selectedNotes.length > 0
        }

        Item { Layout.fillHeight: true }
    }

    Label {
        anchors.centerIn: parent
        visible: !root.vm || root.vm.selectedId === 0
        text: qsTr("Select a task to see details")
        opacity: 0.5
    }
}
