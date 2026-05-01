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

    // ---------- M-13: menu bar ----------
    //
    // Native top menu bar (promoted to the macOS global menu bar
    // automatically). On Linux/Windows it renders inline above the
    // toolbar. The Action objects are reused below by the toolbar
    // buttons + the standalone `Shortcut`s, so the same handler
    // fires whether the user clicks the button, picks the menu item,
    // or taps the shortcut.
    //
    // `MenuItem` itself has no `shortcut` property in Qt 6.6's
    // QtQuick.Controls — the shortcut is owned by the bound
    // `Action`, which the MenuItem renders as a label. So Quit gets
    // its own Action below alongside the others.
    menuBar: MenuBar {
        Menu {
            title: qsTr("&File")
            MenuItem { action: openDifferentAction }
            MenuItem { action: openDefaultAction }
            MenuSeparator {}
            MenuItem { action: importBackupAction }
            MenuSeparator {}
            MenuItem { action: quitAction }
        }
        Menu {
            title: qsTr("&Edit")
            MenuItem { action: newTaskAction }
            MenuItem { action: editSelectedAction }
            MenuItem { action: deleteSelectedAction }
        }
        Menu {
            title: qsTr("&View")
            MenuItem { action: focusFilterAction }
            MenuItem { action: openSettingsAction }
        }
        Menu {
            title: qsTr("&Help")
            MenuItem { action: aboutAction }
        }
    }

    // ---------- H-1: shared Actions + global shortcuts ----------
    Action {
        id: openDifferentAction
        text: qsTr("Open different…")
        shortcut: StandardKey.Open
        onTriggered: openDialog.open()
    }
    Action {
        id: openDefaultAction
        text: qsTr("Open default database")
        onTriggered: viewModel.openDefaultDatabase()
    }
    Action {
        id: importBackupAction
        text: qsTr("Import backup…")
        onTriggered: importDialog.open()
    }
    Action {
        id: openSettingsAction
        text: qsTr("Settings…")
        shortcut: StandardKey.Preferences
        onTriggered: {
            settingsWindow.loadFromVm();
            settingsWindow.visible = true;
            settingsWindow.raise();
            settingsWindow.requestActivate();
        }
    }
    Action {
        id: newTaskAction
        text: qsTr("New task")
        // Ctrl+N is the universal "new" gesture; on macOS Qt
        // auto-translates the modifier to Cmd.
        shortcut: "Ctrl+N"
        onTriggered: listPane.focusQuickAdd()
    }
    Action {
        id: editSelectedAction
        text: qsTr("Edit selected task…")
        shortcut: "F2"
        enabled: viewModel.selectedId > 0
        onTriggered: detailPane.openEditForSelected()
    }
    Action {
        id: deleteSelectedAction
        text: qsTr("Delete selected task")
        shortcut: "Delete"
        enabled: viewModel.selectedId > 0
        onTriggered: detailPane.requestDelete()
    }
    Action {
        id: focusFilterAction
        text: qsTr("Focus sidebar")
        shortcut: "Ctrl+F"
        onTriggered: sidebar.focusList()
    }
    Action {
        id: quitAction
        text: qsTr("Quit")
        shortcut: StandardKey.Quit
        onTriggered: Qt.quit()
    }
    Action {
        id: aboutAction
        text: qsTr("About Tasks Desktop")
        onTriggered: aboutDialog.open()
    }

    // ---------- H-5: transient toast surface ----------
    //
    // The bottom status-bar Label is easy to miss because the eye
    // is on the active pane during an action. This Popup mirrors
    // the latest non-empty status message at the top of the window
    // for `_toastDurationMs`, then auto-hides. The status bar
    // continues to carry the latest text persistently for users
    // who do glance down.
    //
    // Heuristic to keep noise down: skip messages that are pure
    // "N task(s) in view" reload chatter — the UI already shows
    // the count in the list pane header.
    readonly property int _toastDurationMs: 4000

    Connections {
        target: viewModel
        function onStatusChanged() {
            const msg = viewModel.status;
            if (msg.length === 0) { return; }
            if (/^\d+ task\(s\) in view$/.test(msg)) { return; }
            toastLabel.text = msg;
            toastPopup.open();
            toastTimer.restart();
        }
    }

    Popup {
        id: toastPopup
        x: (root.width - width) / 2
        y: 8
        padding: 10
        modal: false
        focus: false
        closePolicy: Popup.NoAutoClose
        Material.elevation: 6

        background: Rectangle {
            color: Material.background
            radius: 6
            border.color: Material.foreground
            border.width: 0
            opacity: 0.95
        }
        contentItem: RowLayout {
            spacing: 12
            Label {
                id: toastLabel
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                elide: Text.ElideRight
                maximumLineCount: 3
            }
            // H-6: undo button visible only while the bridge has a
            // pinned last-deleted row. Clicking it both restores
            // the task and dismisses the toast.
            Button {
                visible: viewModel.lastDeletedId > 0
                text: qsTr("Undo")
                flat: true
                highlighted: true
                onClicked: {
                    viewModel.restoreLastDeleted();
                    toastPopup.close();
                    toastTimer.stop();
                }
            }
        }

        // Fade-in / fade-out via the popup's built-in transitions.
        enter: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 180 } }
        exit: Transition { NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 220 } }

        onClosed: {
            // When the toast hides, drop any stale undo state so
            // the button doesn't reappear next time the popup opens
            // for an unrelated message.
            if (viewModel.lastDeletedId > 0) {
                viewModel.clearLastDeleted();
            }
        }
    }

    Timer {
        id: toastTimer
        interval: root._toastDurationMs
        repeat: false
        onTriggered: toastPopup.close()
    }

    // Lightweight About dialog wired from the Help menu.
    Dialog {
        id: aboutDialog
        title: qsTr("About Tasks Desktop")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Close
        Label {
            text: qsTr(
                "Tasks Desktop — native Rust + Qt 6 client for Tasks.org.\n\n"
              + "Read-only viewer + local writes; sync providers in progress.\n"
              + "See desktop-native/README.md for build + roadmap.")
            wrapMode: Text.Wrap
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
            // H-4: free-text substring search across title + notes.
            // Updates on every keystroke (the bridge debounces by
            // suppressing reloads when the trimmed text is
            // unchanged). Clearing the field returns to the active
            // sidebar filter.
            TextField {
                id: searchField
                Layout.preferredWidth: 220
                placeholderText: qsTr("Search tasks…")
                ToolTip.visible: hovered && text.length === 0
                ToolTip.text: qsTr("Search title + notes (substring match)")
                onTextChanged: viewModel.setSearchQuery(text)
                Keys.onEscapePressed: { text = ""; }
            }
            Button {
                action: openDifferentAction
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Browse for a different Tasks.org SQLite database")
            }
            Button {
                action: importBackupAction
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Import a Tasks.org JSON backup file into the open database")
            }
            Button {
                action: openSettingsAction
                ToolTip.visible: hovered
                ToolTip.text: qsTr("List preferences, sync accounts")
            }
            Button {
                // Renamed from "Reset to default" to defuse the
                // false impression of a destructive reset (this
                // just opens the OS-default DB file the desktop
                // manages itself; nothing is wiped).
                action: openDefaultAction
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Open the default-located Tasks database for this OS")
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
