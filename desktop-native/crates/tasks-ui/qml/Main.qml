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
            let path = urlToLocalFile(selectedFile);
            viewModel.openDatabase(path);
        }
    }

    FileDialog {
        id: importDialog
        title: qsTr("Import a Tasks.org JSON backup")
        nameFilters: [
            qsTr("Tasks.org JSON backup (*.json)"),
            qsTr("All files (*)")
        ]
        fileMode: FileDialog.OpenFile

        onAccepted: {
            viewModel.importJsonBackup(urlToLocalFile(selectedFile));
        }
    }

    // Turn a QML FileDialog.selectedFile (a `file://` URL) into a
    // native absolute path the Rust side can hand straight to
    // std::fs::open(). Three platform wrinkles:
    //
    //   1. URL encoding — spaces and other characters come back as
    //      `%20` etc. decodeURIComponent undoes that.
    //   2. Unix: "file:///home/user/foo" → "/home/user/foo".
    //      Strip the "file://" prefix (7 chars) and keep the leading
    //      slash intact.
    //   3. Windows: "file:///C:/Users/foo" → "C:/Users/foo".
    //      After stripping "file://" we're left with "/C:/...", but
    //      Windows path APIs reject the leading slash ("os error 123:
    //      filename, directory name, or volume label syntax is
    //      incorrect"), so drop it when a drive letter follows.
    //      Rust handles forward-slash-separated Windows paths fine.
    function urlToLocalFile(url) {
        let s = url.toString();
        if (!s.startsWith("file://")) {
            return s;
        }
        s = decodeURIComponent(s).substring(7);
        if (/^\/[A-Za-z]:/.test(s)) {
            s = s.substring(1);
        }
        return s;
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
            // DB path. Compact: smaller font, regular face (not
            // monospace — that ate the toolbar on narrow windows
            // and looked out of place against the rest of the
            // chrome). Path elides; the window title carries the
            // unelided form for users who want to verify.
            Label {
                Layout.fillWidth: true
                text: viewModel.dbPathDisplay.length > 0
                      ? viewModel.dbPathDisplay
                      : qsTr("(no database open)")
                elide: Text.ElideMiddle
                font.pointSize: Qt.application.font.pointSize - 1
                opacity: 0.7
            }
            Button {
                text: qsTr("Open different\u2026")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Browse for a different Tasks.org SQLite database")
                onClicked: openDialog.open()
            }
            Button {
                text: qsTr("Import backup")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Import a Tasks.org JSON backup file into the open database")
                onClicked: importDialog.open()
            }
            Button {
                text: qsTr("Settings…")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("List preferences, sync accounts")
                onClicked: {
                    settingsWindow.loadFromVm();
                    settingsWindow.visible = true;
                    settingsWindow.raise();
                    settingsWindow.requestActivate();
                }
            }
            Button {
                // Renamed from "Reset to default" to defuse the
                // false impression of a destructive reset (this
                // just opens the OS-default DB file the desktop
                // manages itself; nothing is wiped).
                text: qsTr("Open default database")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Open the default-located Tasks database for this OS")
                onClicked: viewModel.openDefaultDatabase()
            }
        }
    }

    // Settings is now a top-level Window (resizable, natively
    // decorated) holding tabs for List preferences + Accounts.
    // Hide-on-close preserves the selected tab and in-flight form
    // state between re-opens.
    SettingsWindow {
        id: settingsWindow
        vm: viewModel
    }

    footer: ToolBar {
        // Pin the status bar height so a long error string can't
        // grow the bar and shove the SplitView upward.
        implicitHeight: 28
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
