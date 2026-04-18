// Sidebar listing built-in filters, CalDAV calendars, and custom filters.
// Rows come from the view model's parallel `sidebarLabels` / `sidebarIds`
// properties. Selecting a row calls `selectFilter(id)`, which re-queries
// the DB and refreshes the task list + detail panes.
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
            text: qsTr("Filters")
            font.bold: true
            opacity: 0.7
        }

        ListView {
            id: sidebarList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.vm ? root.vm.sidebarLabels.length : 0

            delegate: ItemDelegate {
                width: sidebarList.width
                text: root.vm ? root.vm.sidebarLabels[index] : ""
                highlighted: root.vm && root.vm.activeFilterId === root.vm.sidebarIds[index]
                onClicked: if (root.vm) root.vm.selectFilter(root.vm.sidebarIds[index])
            }
        }
    }
}
