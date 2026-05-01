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

    // Pick a readable text colour for a chip background. ARGB
    // input; uses the perceived-luminance formula (Rec. 601 Y'),
    // threshold at 150/255 — keeps text contrast above ~4.5:1 for
    // typical CalDAV palette colours.
    function chipTextFor(argb) {
        const c = argb | 0;
        const r = (c >>> 16) & 0xff;
        const g = (c >>>  8) & 0xff;
        const b =  c         & 0xff;
        const lum = (r * 0.299) + (g * 0.587) + (b * 0.114);
        return lum > 150 ? "black" : "white";
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
        CompactTextField {
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

                    // Priority is encoded by colouring the checkbox
                    // itself. Material's stock indicator only tints
                    // its filled (checked) state from Material.accent
                    // and falls back to the theme foreground for the
                    // unchecked outline — which means every unchecked
                    // box rendered the same gray. Override the
                    // `indicator` with a custom Rectangle so the
                    // outline + filled state both use the priority
                    // colour. Mapping matches the Android palette:
                    //   HIGH   #d32f2f  red
                    //   MEDIUM #f57c00  orange
                    //   LOW    #1976d2  blue
                    //   NONE   neutral grey
                    CheckBox {
                        id: completeBox
                        padding: 0
                        checked: root.vm && root.vm.completedFlags[index]
                        property color priorityColor: {
                            const p = root.vm ? root.vm.priorities[index] : 3;
                            switch (p) {
                                case 0: return "#d32f2f";
                                case 1: return "#f57c00";
                                case 2: return "#1976d2";
                                default: return "#9e9e9e";
                            }
                        }
                        indicator: Rectangle {
                            implicitWidth: 18
                            implicitHeight: 18
                            x: completeBox.leftPadding
                            y: parent.height / 2 - height / 2
                            radius: 3
                            border.color: completeBox.priorityColor
                            border.width: 2
                            color: completeBox.checked
                                   ? completeBox.priorityColor
                                   : "transparent"
                            // Checkmark on the filled state.
                            Label {
                                anchors.centerIn: parent
                                text: "\u{2713}"           // ✓
                                color: "white"
                                font.bold: true
                                font.pointSize: 11
                                visible: completeBox.checked
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

                    // Title + per-tag chip strip. The chip row wraps
                    // across multiple lines so wide tag lists don't
                    // bleed into the due column. Each chip's background
                    // is the tag's assigned colour; text contrast is
                    // chosen by luminance.
                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 2

                        Label {
                            Layout.fillWidth: true
                            text: root.vm ? root.vm.titles[index] : ""
                            elide: Text.ElideRight
                            font.strikeout: root.vm && root.vm.completedFlags[index]
                            opacity: root.vm && root.vm.completedFlags[index] ? 0.55 : 1.0
                        }
                        Flow {
                            Layout.fillWidth: true
                            visible: root.vm
                                     && root.vm.taskTagUidLists[index].length > 0
                            spacing: 4
                            Repeater {
                                model: root.vm
                                    ? root.vm.taskTagUidLists[index]
                                        .split(",").filter(function (s) { return s.length > 0; })
                                    : []
                                Label {
                                    required property string modelData
                                    property int tagIdx: {
                                        if (!root.vm) { return -1; }
                                        const uids = root.vm.tagUids;
                                        for (let i = 0; i < uids.length; i++) {
                                            if (uids[i] === modelData) { return i; }
                                        }
                                        return -1;
                                    }
                                    property int tagColor: {
                                        if (tagIdx < 0 || !root.vm) { return 0; }
                                        const colors = root.vm.tagColors;
                                        return (tagIdx < colors.length)
                                            ? (colors[tagIdx] | 0)
                                            : 0;
                                    }
                                    text: {
                                        if (tagIdx < 0 || !root.vm) { return modelData; }
                                        return root.vm.tagLabels[tagIdx] || modelData;
                                    }
                                    font.pointSize: Qt.application.font.pointSize - 2
                                    font.bold: true
                                    leftPadding: 5
                                    rightPadding: 5
                                    topPadding: 1
                                    bottomPadding: 1
                                    color: {
                                        const a = (tagColor >>> 24) & 0xff;
                                        if (a === 0) { return Material.foreground; }
                                        return root.chipTextFor(tagColor);
                                    }
                                    background: Rectangle {
                                        radius: 4
                                        color: {
                                            const a = ((tagColor >>> 24) & 0xff) / 255.0;
                                            if (a === 0) { return "#9e9e9e"; }
                                            const r = ((tagColor >>> 16) & 0xff) / 255.0;
                                            const g = ((tagColor >>>  8) & 0xff) / 255.0;
                                            const b = ( tagColor         & 0xff) / 255.0;
                                            return Qt.rgba(r, g, b, 1.0);
                                        }
                                        opacity: ((tagColor >>> 24) & 0xff) === 0 ? 0.4 : 1.0
                                    }
                                }
                            }
                        }
                    }

                    // Right column: due date on top, CalDAV list
                    // pill underneath. Both right-aligned so the
                    // column edges line up across rows.
                    ColumnLayout {
                        Layout.alignment: Qt.AlignVCenter | Qt.AlignRight
                        spacing: 2

                        Label {
                            Layout.alignment: Qt.AlignRight
                            text: root.vm ? root.vm.dueLabels[index] : ""
                            opacity: 0.65
                            font.pointSize: Qt.application.font.pointSize - 1
                        }

                        // CalDAV list pill, coloured to match
                        // `cdl_color` on the list. Hidden for local
                        // tasks (no list = empty name + colour 0).
                        // Text colour follows the bg's luminance so
                        // pale backgrounds (yellow / cyan / pastels)
                        // stay readable.
                        Label {
                            Layout.alignment: Qt.AlignRight
                            Layout.maximumWidth: 120
                            visible: root.vm
                                     && root.vm.taskListNames[index].length > 0
                            text: root.vm ? root.vm.taskListNames[index] : ""
                            color: root.vm
                                ? root.chipTextFor(root.vm.taskListColors[index] | 0)
                                : "white"
                            font.pointSize: Qt.application.font.pointSize - 2
                            font.bold: true
                            leftPadding: 6
                            rightPadding: 6
                            topPadding: 1
                            bottomPadding: 1
                            elide: Text.ElideRight
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
                    }
                }
            }
        }
    }
}
