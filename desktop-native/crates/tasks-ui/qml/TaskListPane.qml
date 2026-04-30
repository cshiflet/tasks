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

    // Wired to Main.qml's `New task` shortcut (Ctrl+N) — focuses
    // the quick-add field so the user can type immediately.
    function focusQuickAdd() {
        quickAdd.forceActiveFocus();
    }

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
                id: rowDelegate
                width: list.width
                highlighted: root.vm && root.vm.selectedId === root.vm.taskIds[index]
                onClicked: if (root.vm) root.vm.selectTask(root.vm.taskIds[index])

                // H-7: 3-px CalDAV list-color stripe down the row's
                // left edge so the user can scan list affiliation
                // without the picker. Empty (transparent) for local
                // tasks. Stamped from `task_list_colors[index]`
                // ARGB; alpha 0 means "no list".
                Rectangle {
                    anchors.left: parent.left
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    width: 3
                    visible: root.vm
                             && (root.vm.taskListColors[index] >>> 24) !== 0
                    // QML accepts `Qt.rgba(r,g,b,a)` floats 0..1; the
                    // i32 ARGB on the wire is unpacked manually.
                    color: {
                        if (!root.vm) { return "transparent"; }
                        const c = root.vm.taskListColors[index] | 0;
                        const a = ((c >>> 24) & 0xff) / 255.0;
                        const r = ((c >>> 16) & 0xff) / 255.0;
                        const g = ((c >>>  8) & 0xff) / 255.0;
                        const b = ( c         & 0xff) / 255.0;
                        return Qt.rgba(r, g, b, a);
                    }
                }

                contentItem: RowLayout {
                    spacing: 8

                    // Indent guide for subtasks (L-1).
                    Item {
                        implicitWidth: root.vm ? root.vm.indents[index] * 16 : 0
                        implicitHeight: 1
                        Rectangle {
                            visible: root.vm && root.vm.indents[index] > 0
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.bottom: parent.bottom
                            anchors.rightMargin: 4
                            width: 1
                            color: "gray"
                            opacity: 0.25
                        }
                    }

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

                    // Title + tag summary stack. The tags line only
                    // renders when the task has at least one tag,
                    // and elides on overflow so wide tag lists don't
                    // bleed into the due column.
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        Label {
                            Layout.fillWidth: true
                            text: root.vm ? root.vm.titles[index] : ""
                            elide: Text.ElideRight
                            font.strikeout: root.vm && root.vm.completedFlags[index]
                            opacity: root.vm && root.vm.completedFlags[index] ? 0.55 : 1.0
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: root.vm
                                     && root.vm.taskTagSummaries[index].length > 0
                            text: root.vm ? root.vm.taskTagSummaries[index] : ""
                            elide: Text.ElideRight
                            opacity: 0.55
                            font.pointSize: Qt.application.font.pointSize - 2
                        }
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
