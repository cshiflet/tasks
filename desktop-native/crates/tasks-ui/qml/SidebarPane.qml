// Sidebar listing built-in filters, CalDAV calendars, and custom filters.
// Rows come from the view model's parallel `sidebarLabels` / `sidebarIds`
// properties. Selecting a row calls `selectFilter(id)`, which re-queries
// the DB and refreshes the task list + detail panes.
//
// The bridge serves entries grouped by kind via the `sidebarIds` prefix:
//   `__…__`     — built-in filters (All / Today / Recent)
//   `caldav:…`  — CalDAV calendars
//   `filter:…`  — saved custom filters
// A small section header is injected above the first row of each
// group so the user can see at a glance what they're choosing
// between (C-2 fix — was previously one flat list).
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

    // Wired to Main.qml's `Focus sidebar` action (Ctrl+F). Brings
    // keyboard focus to the ListView so arrow keys navigate between
    // filters.
    function focusList() {
        sidebarList.forceActiveFocus();
    }

    // Map a sidebar id prefix to its group key. Unknowns fall under
    // "Other" so a future bridge extension surfaces explicitly.
    function _groupOf(id) {
        if (!id) { return "other"; }
        if (id.startsWith("__")) { return "filters_builtin"; }
        if (id.startsWith("caldav:")) { return "caldav"; }
        if (id.startsWith("filter:")) { return "saved"; }
        return "other";
    }
    function _groupLabel(group) {
        switch (group) {
            case "filters_builtin": return qsTr("Quick filters");
            case "caldav":          return qsTr("CalDAV lists");
            case "saved":           return qsTr("Saved filters");
            default:                return qsTr("Other");
        }
    }

    // Per-group collapsed flags. Defaults to expanded; toggling
    // reassigns the whole object so the bindings on each row re-evaluate.
    property var collapsedGroups: ({})

    function _isCollapsed(group) {
        return collapsedGroups[group] === true;
    }

    function _toggleGroup(group) {
        const next = Object.assign({}, collapsedGroups);
        next[group] = !next[group];
        collapsedGroups = next;
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        ListView {
            id: sidebarList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.vm ? root.vm.sidebarLabels.length : 0
            spacing: 0

            delegate: Column {
                id: row
                required property int index
                width: sidebarList.width

                // Section header when the group switches (or for the
                // very first row).
                property string myGroup: root.vm
                    ? root._groupOf(root.vm.sidebarIds[row.index])
                    : ""
                property bool _isSectionStart: {
                    if (!root.vm) { return false; }
                    if (row.index === 0) { return true; }
                    const prevId = root.vm.sidebarIds[row.index - 1];
                    return root._groupOf(prevId) !== row.myGroup;
                }
                property bool _groupCollapsed: root._isCollapsed(row.myGroup)

                // Tinted, clickable section header. The chevron on
                // the left rotates to indicate collapsed/expanded;
                // the whole row toggles. Uses Material.foreground at
                // low opacity for the tint so it adapts to both
                // light and dark themes.
                ItemDelegate {
                    id: header
                    visible: row._isSectionStart
                    width: row.width
                    // Override Material's 48 px touch-target floor —
                    // Material.touchTarget pads ItemDelegate to 48 px
                    // regardless of topPadding, so a header strip at
                    // "3 + 3 padding" still rendered ~48 px tall.
                    // Setting implicitHeight directly drives the row
                    // size and the contentItem fits inside it.
                    implicitHeight: visible ? 22 : 0
                    height: implicitHeight
                    topPadding: 0
                    bottomPadding: 0
                    leftPadding: 8
                    rightPadding: 8
                    onClicked: root._toggleGroup(row.myGroup)

                    background: Rectangle {
                        color: Material.foreground
                        opacity: 0.08
                    }

                    contentItem: RowLayout {
                        spacing: 6

                        // Painted chevron — no Unicode triangle in
                        // sight. The literal triangle glyphs (and the
                        // ▶ / ▼ escape variants) both showed
                        // as "â¾"-style mojibake on the user's Windows
                        // build, presumably because Qt's font-fallback
                        // picked a typeface without the BMP geometric-
                        // shapes block. Drawing two line segments via
                        // Canvas dodges every font-coverage and file-
                        // encoding question, and a 90° rotation
                        // animates the expand/collapse transition.
                        Canvas {
                            id: chevron
                            Layout.preferredWidth: 12
                            Layout.preferredHeight: 12
                            rotation: row._groupCollapsed ? -90 : 0
                            opacity: 0.75
                            Behavior on rotation {
                                NumberAnimation { duration: 120 }
                            }
                            onPaint: {
                                const ctx = getContext("2d");
                                ctx.reset();
                                ctx.lineWidth = 1.6;
                                ctx.lineCap = "round";
                                ctx.lineJoin = "round";
                                ctx.strokeStyle = header.Material.foreground;
                                ctx.beginPath();
                                ctx.moveTo(2, 4);
                                ctx.lineTo(width / 2, height - 4);
                                ctx.lineTo(width - 2, 4);
                                ctx.stroke();
                            }
                        }
                        Label {
                            Layout.fillWidth: true
                            text: root.vm ? root._groupLabel(row.myGroup) : ""
                            font.bold: true
                            font.pointSize: Qt.application.font.pointSize - 1
                            opacity: 0.75
                            elide: Text.ElideRight
                        }
                    }
                }

                ItemDelegate {
                    width: row.width
                    visible: !row._groupCollapsed
                    // Same reasoning as the header — Material would
                    // otherwise pin every row at the 48 px touch
                    // target regardless of topPadding.
                    implicitHeight: visible ? 28 : 0
                    height: implicitHeight
                    topPadding: 0
                    bottomPadding: 0
                    text: root.vm ? root.vm.sidebarLabels[row.index] : ""
                    highlighted: root.vm
                        && root.vm.activeFilterId === root.vm.sidebarIds[row.index]
                    onClicked: if (root.vm) root.vm.selectFilter(root.vm.sidebarIds[row.index])
                }
            }
        }
    }
}
