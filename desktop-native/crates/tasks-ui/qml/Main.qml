// Three-pane layout for the Tasks.org native desktop client.
//
// Composition:
//   - SidebarPane   : filters + CalDAV calendars + saved filters
//   - TaskListPane  : active list, indented for subtasks
//   - TaskDetailPane: title, notes, due date, priority of the selected task
//
// The data source is the `TaskListViewModel` QObject defined in
// crates/tasks-ui/src/bridge.rs, registered in QML as
// `com.tasks.desktop.TaskListViewModel`.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Dialogs
import QtQuick.Layouts

import com.tasks.desktop

ApplicationWindow {
    id: root
    width: 1100
    height: 720
    visible: true
    title: qsTr("Tasks")

    // Auto-follow OS light/dark mode. When the user flips their system
    // theme, Qt updates Material.theme to match on the next paint.
    Material.theme: Material.System
    Material.accent: Material.Blue

    TaskListViewModel {
        id: viewModel
    }

    FileDialog {
        id: openDialog
        title: qsTr("Open Tasks database")
        // SQLite databases have no registered MIME type, so accept all
        // files and let `Database::open_read_only` verify the Room
        // identity hash.
        nameFilters: [
            qsTr("SQLite database (*.db *.sqlite *.sqlite3)"),
            qsTr("All files (*)")
        ]
        fileMode: FileDialog.OpenFile

        onAccepted: {
            // selectedFile is a file:// URL; strip the scheme so
            // Database::open_read_only gets a plain path.
            let path = selectedFile.toString();
            if (path.startsWith("file://")) {
                path = path.substring(7);
            }
            pathField.text = path;
            viewModel.openDatabase(path);
        }
    }

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            spacing: 8

            TextField {
                id: pathField
                Layout.fillWidth: true
                placeholderText: qsTr("Path to tasks.db (or click Browse…)")
                selectByMouse: true
                onAccepted: if (text.length > 0) viewModel.openDatabase(text)
            }
            Button {
                text: qsTr("Browse…")
                onClicked: openDialog.open()
            }
            Button {
                text: qsTr("Open")
                enabled: pathField.text.length > 0
                onClicked: viewModel.openDatabase(pathField.text)
            }
        }
    }

    footer: ToolBar {
        Label {
            anchors.fill: parent
            anchors.leftMargin: 8
            verticalAlignment: Text.AlignVCenter
            text: viewModel.status
            elide: Text.ElideRight
        }
    }

    SplitView {
        id: root_split
        anchors.fill: parent
        orientation: Qt.Horizontal

        SidebarPane {
            id: sidebar
            SplitView.preferredWidth: 240
            SplitView.minimumWidth: 180
            vm: viewModel
        }

        SplitView {
            orientation: Qt.Horizontal
            SplitView.fillWidth: true

            TaskListPane {
                id: listPane
                SplitView.preferredWidth: 420
                SplitView.minimumWidth: 280
                vm: viewModel
            }

            TaskDetailPane {
                id: detailPane
                SplitView.fillWidth: true
                SplitView.minimumWidth: 260
                vm: viewModel
            }
        }
    }
}
