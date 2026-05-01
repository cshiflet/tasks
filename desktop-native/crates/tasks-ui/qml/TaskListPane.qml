// Middle pane showing the currently-filtered task list.
//
// Rows are indented by `vm.indents[index]` so subtasks nest under their
// parent, mirroring the Android list renderer. Completed tasks are shown
// struck-through. Tapping a row selects it for the detail pane.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

Pane {
    id: root
    padding: 0
    // Belt-and-braces theme propagation; see TaskDetailPane.qml.
    Material.theme: Material.System
    Material.accent: Material.Blue
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

                // List affiliation is shown as an inline chip in the
                // contentItem below, not as a left-edge stripe — keeps
                // each row's visual chrome consistent with the
                // metadata strip in the detail pane.

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

                    // Priority is encoded via the checkbox's tint
                    // (Material.accent), not a separate dot. Mapping
                    // matches the Android palette:
                    //   HIGH   #d32f2f  red
                    //   MEDIUM #f57c00  orange
                    //   LOW    #1976d2  blue
                    //   NONE   default Material accent
                    CheckBox {
                        id: completeBox
                        padding: 0
                        checked: root.vm && root.vm.completedFlags[index]
                        Material.accent: {
                            const p = root.vm ? root.vm.priorities[index] : 3;
                            switch (p) {
                                case 0: return "#d32f2f";
                                case 1: return "#f57c00";
                                case 2: return "#1976d2";
                                default: return Material.color(Material.Blue);
                            }
                        }
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

                    // CalDAV list pill, coloured to match
                    // `cdl_color` on the list. Hidden for local
                    // tasks (no list = empty name + colour 0).
                    Label {
                        visible: root.vm
                                 && root.vm.taskListNames[index].length > 0
                        text: root.vm ? root.vm.taskListNames[index] : ""
                        color: "white"
                        font.pointSize: Qt.application.font.pointSize - 2
                        font.bold: true
                        leftPadding: 6
                        rightPadding: 6
                        topPadding: 1
                        bottomPadding: 1
                        elide: Text.ElideRight
                        Layout.maximumWidth: 120
                        background: Rectangle {
                            radius: 4
                            color: {
                                if (!root.vm) { return "#9e9e9e"; }
                                const c = root.vm.taskListColors[index] | 0;
                                const a = ((c >>> 24) & 0xff) / 255.0;
                                if (a === 0) { return "#9e9e9e"; }
                                const r = ((c >>> 16) & 0xff) / 255.0;
                                const g = ((c >>>  8) & 0xff) / 255.0;
                                const b = ( c         & 0xff) / 255.0;
                                return Qt.rgba(r, g, b, 1.0);
                            }
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
