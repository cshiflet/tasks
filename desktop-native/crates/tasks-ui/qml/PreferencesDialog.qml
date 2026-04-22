// User-editable query preferences.
//
// Maps onto the flags in `tasks_core::query::QueryPreferences` that
// the task-list query builder honours. Saving calls
// `vm.updatePreferences(...)` which rewrites the bridge's live
// copy and reloads the active filter.
//
// Session-local for now; a QSettings persistence pass is a
// follow-up (see README roadmap).
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: dialog
    modal: true
    title: qsTr("Preferences")
    standardButtons: Dialog.Cancel | Dialog.Ok
    implicitWidth: 420

    required property QtObject vm

    function loadFromVm() {
        if (!vm) {
            return;
        }
        // Map the sort_mode integer → ComboBox index.
        // 0=AUTO, 1=ALPHA, 2=DUE, 3=IMPORTANCE, 4=MODIFIED,
        // 5=CREATED, 8=START. The ComboBox model only lists the
        // modes that make sense in a desktop picker (skipping
        // AstridOrder / CalDAV-native / manual / GTasks etc.).
        const mapping = [0, 1, 2, 3, 4, 5, 8];
        const idx = mapping.indexOf(vm.prefSortMode);
        sortBox.currentIndex = idx >= 0 ? idx : 0;
        ascendingBox.checked = vm.prefSortAscending;
        showCompletedBox.checked = vm.prefShowCompleted;
        showHiddenBox.checked = vm.prefShowHidden;
        completedAtBottomBox.checked = vm.prefCompletedAtBottom;
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        GridLayout {
            columns: 2
            columnSpacing: 12
            rowSpacing: 8
            Layout.fillWidth: true

            Label {
                text: qsTr("Sort by")
                opacity: 0.7
            }
            ComboBox {
                id: sortBox
                Layout.fillWidth: true
                model: [
                    qsTr("Automatic (due + importance)"),
                    qsTr("Alphabetical"),
                    qsTr("Due date"),
                    qsTr("Priority"),
                    qsTr("Modified"),
                    qsTr("Created"),
                    qsTr("Start date"),
                ]
            }

            Label {
                text: qsTr("Direction")
                opacity: 0.7
            }
            ComboBox {
                id: ascendingBox
                property bool checked: true
                Layout.fillWidth: true
                model: [qsTr("Ascending"), qsTr("Descending")]
                currentIndex: checked ? 0 : 1
                onCurrentIndexChanged: checked = (currentIndex === 0)
            }
        }

        CheckBox {
            id: showCompletedBox
            text: qsTr("Show completed tasks")
        }
        CheckBox {
            id: completedAtBottomBox
            text: qsTr("Completed tasks at the bottom")
            enabled: showCompletedBox.checked
        }
        CheckBox {
            id: showHiddenBox
            text: qsTr("Show hidden (future hide-until) tasks")
        }
    }

    onAccepted: {
        if (!vm) { return; }
        const mapping = [0, 1, 2, 3, 4, 5, 8];
        const sortMode = mapping[sortBox.currentIndex] ?? 0;
        vm.updatePreferences(
            sortMode,
            ascendingBox.checked,
            showCompletedBox.checked,
            showHiddenBox.checked,
            completedAtBottomBox.checked);
    }
}
