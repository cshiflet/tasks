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
                        property bool recurring: root.vm
                            && root.vm.recurringFlags[index]
                        property color priorityColor: {
                            const p = root.vm ? root.vm.priorities[index] : 3;
                            switch (p) {
                                case 0: return "#d32f2f";
                                case 1: return "#f57c00";
                                case 2: return "#1976d2";
                                default: return "#9e9e9e";
                            }
                        }
                        indicator: Item {
                            implicitWidth: 18
                            implicitHeight: 18
                            x: completeBox.leftPadding
                            y: parent.height / 2 - height / 2

                            // Square checkbox for non-recurring tasks.
                            // Border + fill follow the priority colour
                            // so unchecked rows still hint at priority.
                            Rectangle {
                                visible: !completeBox.recurring
                                anchors.fill: parent
                                radius: 3
                                border.color: completeBox.priorityColor
                                border.width: 2
                                color: completeBox.checked
                                       ? completeBox.priorityColor
                                       : "transparent"
                                Label {
                                    anchors.centerIn: parent
                                    text: "\u{2713}"           // ✓
                                    color: "white"
                                    font.bold: true
                                    font.pointSize: 11
                                    visible: completeBox.checked
                                }
                            }

                            // Squared two-arrow loop for recurring
                            // tasks — each arrow follows two sides of
                            // a rounded square (two right-angle
                            // turns) and ends with an arrowhead
                            // pointing at the other arrow's tail.
                            // Painted via Canvas so we don't depend
                            // on an icon font; stroke + arrowheads
                            // use the priority colour.
                            Canvas {
                                id: recurIcon
                                visible: completeBox.recurring
                                anchors.fill: parent
                                antialiasing: true
                                onPaint: {
                                    const ctx = getContext("2d");
                                    ctx.reset();
                                    const w = width;
                                    const h = height;
                                    const inset = 2;
                                    const x0 = inset;
                                    const y0 = inset;
                                    const x1 = w - inset;
                                    const y1 = h - inset;
                                    // Where each arrow ends — about
                                    // 60% along the third (vertical)
                                    // segment, leaving room for the
                                    // arrowhead and a visible gap
                                    // before the other arrow's tail.
                                    const stop = 0.55;
                                    const yT = y0 + (y1 - y0) * (1 - stop);  // top arrow head Y
                                    const yB = y1 - (y1 - y0) * (1 - stop);  // bottom arrow head Y
                                    ctx.lineWidth = 2;
                                    ctx.lineCap = "round";
                                    ctx.lineJoin = "round";
                                    ctx.strokeStyle = completeBox.priorityColor;
                                    ctx.fillStyle = completeBox.priorityColor;

                                    // Arrow 1 — tail just below the
                                    // top-left corner, traces left
                                    // edge up, across the top, down
                                    // the right edge to ~60%.
                                    ctx.beginPath();
                                    ctx.moveTo(x0, y1 - 0.5);
                                    ctx.lineTo(x0, y0);          // up the left
                                    ctx.lineTo(x1, y0);          // across the top
                                    ctx.lineTo(x1, yT);          // down the right (partial)
                                    ctx.stroke();

                                    // Arrow 2 — tail just above the
                                    // bottom-right corner, traces
                                    // right edge down, across the
                                    // bottom, up the left edge to
                                    // ~60%.
                                    ctx.beginPath();
                                    ctx.moveTo(x1, y0 + 0.5);
                                    ctx.lineTo(x1, y1);          // down the right
                                    ctx.lineTo(x0, y1);          // across the bottom
                                    ctx.lineTo(x0, yB);          // up the left (partial)
                                    ctx.stroke();

                                    // Arrowheads. Each is a small
                                    // filled triangle pointed at the
                                    // other arrow's tail.
                                    const head = 3.5;
                                    // Top-half arrow ends on the right
                                    // edge pointing down — toward the
                                    // tail of arrow 2 (top of right
                                    // edge).
                                    ctx.beginPath();
                                    ctx.moveTo(x1, yT + head);
                                    ctx.lineTo(x1 - head * 0.7, yT - head * 0.3);
                                    ctx.lineTo(x1 + head * 0.7, yT - head * 0.3);
                                    ctx.closePath();
                                    ctx.fill();
                                    // Bottom-half arrow ends on the
                                    // left edge pointing up — toward
                                    // the tail of arrow 1 (bottom of
                                    // left edge).
                                    ctx.beginPath();
                                    ctx.moveTo(x0, yB - head);
                                    ctx.lineTo(x0 - head * 0.7, yB + head * 0.3);
                                    ctx.lineTo(x0 + head * 0.7, yB + head * 0.3);
                                    ctx.closePath();
                                    ctx.fill();

                                    // Centre dot when checked — same
                                    // affordance the round version
                                    // had so completed-state is still
                                    // legible.
                                    if (completeBox.checked) {
                                        ctx.beginPath();
                                        ctx.arc(w / 2, h / 2, 2.5, 0, 2 * Math.PI);
                                        ctx.fill();
                                    }
                                }
                                Connections {
                                    target: completeBox
                                    function onCheckedChanged() { recurIcon.requestPaint(); }
                                    function onPriorityColorChanged() { recurIcon.requestPaint(); }
                                }
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
