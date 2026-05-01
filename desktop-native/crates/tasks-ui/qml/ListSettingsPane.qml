// "List" tab of the Settings window.
//
// Hosts the query preferences that used to live in the standalone
// PreferencesDialog (sort mode + direction, show completed/hidden,
// completed at the bottom). Mirrors the pre-tabbed UI's semantics:
// a Save button at the bottom writes into the bridge via
// `updatePreferences(...)` which re-runs the active filter.
//
// Session-local for now — a QSettings persistence pass is a
// follow-up (see PLAN_UPDATES §8).
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ColumnLayout {
    id: pane
    spacing: 16
    // Pin the Material context so child Labels (which default to
    // `Material.foreground` for their colour) resolve against the
    // window's actual colour scheme. Without this anchor a
    // ColumnLayout's children sometimes fall back to a hard-coded
    // light-theme black on a dark-themed Settings window.
    Material.theme: Material.System
    Material.accent: Material.Blue

    required property QtObject vm

    // Called by SettingsWindow.loadFromVm() right before show(), so
    // every re-open starts from the bridge's current preferences
    // rather than the widget's stale local state.
    function loadFromVm() {
        if (!vm) {
            return;
        }
        // Map the sort_mode integer → ComboBox index. Matches the
        // original PreferencesDialog mapping:
        //   0=AUTO, 1=ALPHA, 2=DUE, 3=IMPORTANCE, 4=MODIFIED,
        //   5=CREATED, 8=START.
        const mapping = [0, 1, 2, 3, 4, 5, 8];
        const idx = mapping.indexOf(vm.prefSortMode);
        sortBox.currentIndex = idx >= 0 ? idx : 0;
        directionBox.currentIndex = vm.prefSortAscending ? 0 : 1;
        showCompletedBox.checked = vm.prefShowCompleted;
        showHiddenBox.checked = vm.prefShowHidden;
        completedAtBottomBox.checked = vm.prefCompletedAtBottom;
    }

    Component.onCompleted: loadFromVm()

    GridLayout {
        columns: 2
        columnSpacing: 12
        rowSpacing: 8
        Layout.fillWidth: true

        Label {
            text: qsTr("Sort by")
            opacity: 0.7
        }
        CompactComboBox {
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
        CompactComboBox {
            id: directionBox
            Layout.fillWidth: true
            model: [qsTr("Ascending"), qsTr("Descending")]
        }
    }

    CheckBox {
        id: showCompletedBox
        text: qsTr("Show completed tasks")
        // Uncheck the dependent toggle too so a subsequent re-open
        // doesn't surface a disabled-but-checked control.
        onCheckedChanged: if (!checked) { completedAtBottomBox.checked = false; }
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

    // Push the Save button to the bottom of the pane regardless of
    // the pane's full height — the outer StackLayout stretches us.
    Item { Layout.fillHeight: true }

    RowLayout {
        Layout.fillWidth: true
        Item { Layout.fillWidth: true }
        Button {
            text: qsTr("Save list preferences")
            highlighted: true
            onClicked: {
                if (!pane.vm) { return; }
                const mapping = [0, 1, 2, 3, 4, 5, 8];
                const sortMode = mapping[sortBox.currentIndex] ?? 0;
                pane.vm.updatePreferences(
                    sortMode,
                    directionBox.currentIndex === 0,
                    showCompletedBox.checked,
                    showHiddenBox.checked,
                    completedAtBottomBox.checked);
            }
        }
    }
}
