// First-cut QML shell for the native desktop client.
//
// Presents a file path field, an Open button, and a list of the currently
// active task titles. The view model lives in bridge.rs; all data access
// goes through tasks-core::Database, which opens the SQLite file read-only.
//
// Subsequent slices will split this into a three-pane layout
// (SidebarPane.qml / TaskListPane.qml / TaskDetailPane.qml) and drop a
// proper QAbstractListModel in for per-row roles.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import com.tasks.desktop

ApplicationWindow {
    id: root
    width: 900
    height: 600
    visible: true
    title: qsTr("Tasks")

    TaskListViewModel {
        id: viewModel
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 8

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            TextField {
                id: pathField
                Layout.fillWidth: true
                placeholderText: qsTr("Path to tasks.db (Android sync file)")
                selectByMouse: true
            }

            Button {
                text: qsTr("Open")
                enabled: pathField.text.length > 0
                onClicked: viewModel.openDatabase(pathField.text)
            }
        }

        Label {
            Layout.fillWidth: true
            text: viewModel.status.length > 0
                  ? viewModel.status
                  : qsTr("Enter the path to an Android Tasks database to view its active tasks.")
            wrapMode: Text.WordWrap
            color: palette.mid
        }

        Label {
            Layout.fillWidth: true
            text: viewModel.count === 1
                  ? qsTr("%1 active task").arg(viewModel.count)
                  : qsTr("%1 active tasks").arg(viewModel.count)
            font.bold: true
        }

        ListView {
            id: taskList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: viewModel.titles
            boundsBehavior: Flickable.StopAtBounds
            delegate: ItemDelegate {
                width: taskList.width
                text: modelData
            }
        }
    }
}
