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
    title: viewModel.dbPathDisplay.length > 0
           ? qsTr("Tasks — %1").arg(viewModel.dbPathDisplay)
           : qsTr("Tasks")

    // Auto-follow OS light/dark mode. When the user flips their system
    // theme, Qt updates Material.theme to match on the next paint.
    Material.theme: Material.System
    Material.accent: Material.Blue

    TaskListViewModel {
        id: viewModel
    }

    // On first launch the default DB doesn't exist yet; openDefaultDatabase
    // creates it at the OS-appropriate data path (see
    // tasks_core::db::default_db_path) and loads it read-only.
    Component.onCompleted: viewModel.openDefaultDatabase()

    FileDialog {
        id: openDialog
        title: qsTr("Open a Tasks database")
        // SQLite databases have no registered MIME type, so accept
        // all files and let `Database::open_read_only` verify the
        // Room identity hash before committing to anything.
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
            viewModel.openDatabase(path);
        }
    }

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            spacing: 8

            // Read-only display of the current DB. Users who want to
            // point the viewer at a different file (e.g. an Android
            // export) use the Open… button.
            Label {
                Layout.fillWidth: true
                text: viewModel.dbPathDisplay.length > 0
                      ? viewModel.dbPathDisplay
                      : qsTr("(no database open)")
                elide: Text.ElideMiddle
                font.family: "monospace"
                opacity: 0.75
            }
            Button {
                text: qsTr("Open different\u2026")
                onClicked: openDialog.open()
            }
            Button {
                text: qsTr("Reset to default")
                onClicked: viewModel.openDefaultDatabase()
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
