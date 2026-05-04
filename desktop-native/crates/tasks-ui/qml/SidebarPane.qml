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
    Material.accent: Material.Blue
    required property QtObject vm

    // Wired to Main.qml's `Focus sidebar` action (Ctrl+F). Brings
    // keyboard focus to the ListView so arrow keys navigate between
    // filters.
    function focusList() {
        sidebarList.forceActiveFocus();
    }

    // Map a sidebar entry to its group key. Built-in / saved filters
    // are decided by the id prefix; list rows (`caldav:*`) consult
    // the parallel `sidebarAccountKinds` array to split LOCAL from
    // real CalDAV from Google Tasks / Microsoft To Do / Etebase /
    // etc. — every list lives in the `caldav_lists` table so the id
    // prefix is uniform, but the visual grouping should match the
    // account type the import / sync layer recorded.
    function _groupOfIndex(idx) {
        if (!root.vm) { return "other"; }
        const id = root.vm.sidebarIds[idx];
        if (!id) { return "other"; }
        if (id.startsWith("__")) { return "filters_builtin"; }
        if (id.startsWith("filter:")) { return "saved"; }
        if (id.startsWith("caldav:")) {
            const kinds = root.vm.sidebarAccountKinds;
            const k = (kinds && idx < kinds.length) ? (kinds[idx] | 0) : 0;
            // Mirrors `tasks_core::models::caldav::AccountType`.
            switch (k) {
                case 0:  return "caldav";        // CALDAV
                case 2:  return "local";         // LOCAL
                case 3:  return "opentasks";     // OPENTASKS
                case 4:  return "tasksorg";      // TASKS_ORG
                case 5:  return "etebase";       // ETEBASE
                case 6:  return "mstodo";        // MICROSOFT
                case 7:  return "gtasks";        // GOOGLE_TASKS
                default: return "caldav";
            }
        }
        return "other";
    }
    function _groupLabel(group) {
        switch (group) {
            case "filters_builtin": return qsTr("Quick filters");
            case "caldav":          return qsTr("CalDAV lists");
            case "local":           return qsTr("Local lists");
            case "gtasks":          return qsTr("Google Tasks");
            case "mstodo":          return qsTr("Microsoft To Do");
            case "etebase":         return qsTr("Etebase");
            case "tasksorg":        return qsTr("Tasks.org");
            case "opentasks":       return qsTr("OpenTasks");
            case "saved":           return qsTr("Saved filters");
            default:                return qsTr("Other");
        }
    }
    // Whether the named group is backed by a working sync engine in
    // this build. Everything except "local" + the built-in / saved
    // filters represents an external sync provider whose `tasks-sync`
    // path isn't wired into the bridge yet (PLAN_UPDATES §11), so
    // their lists are read-only views of the imported data. The
    // sidebar surfaces a "(not connected)" suffix on the section
    // header + lower opacity on each row so the user can tell at a
    // glance which lists won't refresh from a server.
    function _groupIsSync(group) {
        switch (group) {
            case "caldav":
            case "gtasks":
            case "mstodo":
            case "etebase":
            case "tasksorg":
            case "opentasks":
                return true;
            default:
                return false;
        }
    }
    function _groupConnected(group) {
        // Sync isn't wired yet for any provider — every external
        // group is "not connected" in this build. When the sync
        // bridge lands, swap this for a check against the running
        // SyncEngine's account status.
        return !_groupIsSync(group) ? true : false;
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
                    ? root._groupOfIndex(row.index)
                    : ""
                property bool _isSectionStart: {
                    if (!root.vm) { return false; }
                    if (row.index === 0) { return true; }
                    return root._groupOfIndex(row.index - 1) !== row.myGroup;
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
                        // "(not connected)" suffix for sync providers
                        // whose engine isn't running. Same hint shows
                        // up in the section header so a glance at
                        // either the header or a list row tells the
                        // user "this is offline/imported data".
                        Label {
                            visible: !root._groupConnected(row.myGroup)
                            text: qsTr("(not connected)")
                            font.italic: true
                            font.pointSize: Qt.application.font.pointSize - 2
                            opacity: 0.55
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
                    // Dim rows whose owning provider has no live sync
                    // backing them — the data is whatever the import
                    // captured and won't refresh until the sync engine
                    // is wired up.
                    opacity: root._groupConnected(row.myGroup) ? 1.0 : 0.55
                    highlighted: root.vm
                        && root.vm.activeFilterId === root.vm.sidebarIds[row.index]
                    onClicked: if (root.vm) root.vm.selectFilter(root.vm.sidebarIds[row.index])
                }
            }
        }
    }
}
