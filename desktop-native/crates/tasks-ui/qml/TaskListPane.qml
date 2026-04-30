// Middle pane showing the currently-filtered task list.
//
// Rows are indented by `vm.indents[index]` so subtasks nest under their
// parent, mirroring the Android list renderer. Completed tasks are shown
// struck-through. Tapping a row selects it for the detail pane.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Pane {
    id: root
    padding: 0
    required property QtObject vm

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Label {
            Layout.leftMargin: 12
            Layout.topMargin: 8
            Layout.bottomMargin: 4
            text: root.vm ? qsTr("%1 task(s)").arg(root.vm.count) : ""
            font.bold: true
            opacity: 0.7
        }

        // Quick-add row. Enter-to-submit creates a new task under
        // the currently-active filter (CalDAV list if selected,
        // otherwise local). The previous flanking "Add" button was
        // redundant with Enter and ate horizontal space — dropped.
        TextField {
            id: quickAdd
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Layout.bottomMargin: 4
            placeholderText: qsTr("Add a task… (press Enter to create)")
            onAccepted: {
                if (root.vm && text.trim().length > 0) {
                    root.vm.addNewTask(text);
                    text = "";
                }
            }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.vm ? root.vm.count : 0

            delegate: ItemDelegate {
                width: list.width
                highlighted: root.vm && root.vm.selectedId === root.vm.taskIds[index]
                onClicked: if (root.vm) root.vm.selectTask(root.vm.taskIds[index])

                contentItem: RowLayout {
                    spacing: 8

                    // Indent guide for subtasks.
                    Item {
                        implicitWidth: root.vm ? root.vm.indents[index] * 16 : 0
                        implicitHeight: 1
                    }

                    // Completion toggle. We return the *current* check
                    // state from nextCheckState so QML doesn't flip
                    // its own internal `checked` (which would break
                    // the binding to completedFlags); the actual flip
                    // happens via the view model round trip, which
                    // fires the property change and re-evaluates this
                    // CheckBox's `checked` binding.
                    CheckBox {
                        id: completeBox
                        padding: 0
                        checked: root.vm && root.vm.completedFlags[index]
                        nextCheckState: function() {
                            if (root.vm) {
                                root.vm.toggleTaskCompletion(
                                    root.vm.taskIds[index],
                                    !completeBox.checked);
                            }
                            return completeBox.checked
                                ? Qt.Checked
                                : Qt.Unchecked;
                        }
                    }

                    PriorityDot {
                        priority: root.vm ? root.vm.priorities[index] : 3
                    }

                    Label {
                        Layout.fillWidth: true
                        text: root.vm ? root.vm.titles[index] : ""
                        elide: Text.ElideRight
                        font.strikeout: root.vm && root.vm.completedFlags[index]
                        opacity: root.vm && root.vm.completedFlags[index] ? 0.55 : 1.0
                    }

                    Label {
                        text: root.vm ? root.vm.dueLabels[index] : ""
                        opacity: 0.65
                        font.pointSize: Qt.application.font.pointSize - 1
                    }
                }
            }
        }
    }
}
