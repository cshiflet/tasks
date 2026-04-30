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
                property bool _isSectionStart: {
                    if (!root.vm) { return false; }
                    const myId = root.vm.sidebarIds[row.index];
                    if (row.index === 0) { return true; }
                    const prevId = root.vm.sidebarIds[row.index - 1];
                    return root._groupOf(prevId) !== root._groupOf(myId);
                }

                Label {
                    visible: row._isSectionStart
                    text: root.vm
                        ? root._groupLabel(root._groupOf(root.vm.sidebarIds[row.index]))
                        : ""
                    leftPadding: 12
                    topPadding: row.index === 0 ? 8 : 12
                    bottomPadding: 4
                    font.bold: true
                    font.pointSize: Qt.application.font.pointSize - 1
                    opacity: 0.55
                }

                ItemDelegate {
                    width: row.width
                    text: root.vm ? root.vm.sidebarLabels[row.index] : ""
                    highlighted: root.vm
                        && root.vm.activeFilterId === root.vm.sidebarIds[row.index]
                    onClicked: if (root.vm) root.vm.selectFilter(root.vm.sidebarIds[row.index])
                }
            }
        }
    }
}
