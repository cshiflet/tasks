// "Data" tab of the Settings window.
//
// One-stop shop for import / export / backup actions. Today the
// only entry is JSON import (the format Tasks.org's Android client
// produces from Settings → Backups → Export JSON); JSON export
// and CSV import will land here too.
//
// The bridge invokable is `viewModel.importJsonBackup(path)`,
// shared with the File-menu shortcut. The action lives here rather
// than on the command bar because import is rare enough that it
// shouldn't claim toolbar real estate next to the Sync button.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: pane
    spacing: 12

    required property QtObject vm

    FileDialog {
        id: importDialog
        title: qsTr("Import a Tasks.org JSON backup")
        nameFilters: [
            qsTr("Tasks.org JSON backup (*.json)"),
            qsTr("All files (*)"),
        ]
        fileMode: FileDialog.OpenFile
        onAccepted: {
            if (!pane.vm) { return; }
            pane.vm.importJsonBackup(urlToLocalFile(selectedFile));
        }
    }

    // Same path-massage as Main.qml's FileDialog. Duplicated rather
    // than threaded in because QML doesn't have a great cross-file
    // helper-function mechanism and the function is six lines.
    function urlToLocalFile(url) {
        let s = url.toString();
        if (!s.startsWith("file://")) { return s; }
        s = decodeURIComponent(s).substring(7);
        if (/^\/[A-Za-z]:/.test(s)) { s = s.substring(1); }
        return s;
    }

    Label {
        text: qsTr("Import")
        font.bold: true
    }
    Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        opacity: 0.7
        font.pointSize: Qt.application.font.pointSize - 1
        text: qsTr("Bring tasks, lists, places, and tags from a "
                 + "Tasks.org JSON backup into the currently-open "
                 + "database. The Android client produces these via "
                 + "Settings → Backups → Export JSON.")
    }
    RowLayout {
        spacing: 8
        Button {
            text: qsTr("Import JSON backup…")
            onClicked: importDialog.open()
        }
    }

    Item { Layout.fillHeight: true }
}
