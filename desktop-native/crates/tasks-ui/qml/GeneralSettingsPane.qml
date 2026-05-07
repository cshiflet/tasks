// "General" tab of the Settings window.
//
// Holds the app-wide preferences that aren't tied to any one list:
// today, just the appearance override (Light / Dark / Follow OS).
// Future global toggles (e.g. font scale, default new-task list,
// telemetry opt-in) belong here too.
//
// Persistence rides on the bridge's Preferences blob — every
// change writes back via `viewModel.updateThemeMode(...)`, so a
// restart picks up the same value. List-specific preferences
// live in the "List defaults" tab; per-list overrides will land
// behind a right-click affordance on each sidebar entry.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ColumnLayout {
    id: pane
    spacing: 12

    required property QtObject vm
    // Reference to the top-level ApplicationWindow whose
    // `appearanceTheme` property drives Material.theme. Threaded in
    // by SettingsWindow.qml so the live binding flips immediately
    // when the user picks a value.
    required property var appWindow

    function loadFromVm() {
        if (!vm) { return; }
        themeBox.currentIndex = (vm.themeMode | 0).toString().length > 0
            ? Math.max(0, Math.min(2, vm.themeMode | 0))
            : 0;
        notificationsBox.checked = !!vm.notificationsEnabled;
    }

    Component.onCompleted: loadFromVm()

    GridLayout {
        Layout.fillWidth: true
        columns: 2
        columnSpacing: 12
        rowSpacing: 8

        Label {
            text: qsTr("Appearance")
            opacity: 0.7
        }
        CompactComboBox {
            id: themeBox
            Layout.fillWidth: true
            model: [
                qsTr("Follow system"),
                qsTr("Light"),
                qsTr("Dark"),
            ]
            onActivated: {
                if (!pane.vm) { return; }
                pane.vm.updateThemeMode(currentIndex);
                if (pane.appWindow) {
                    pane.appWindow.appearanceTheme = currentIndex;
                }
            }
        }

        // OS-level reminder notifications. Default-on for new
        // installs; the bridge re-arms or cancels the alarm
        // scheduler synchronously on each toggle so the user
        // doesn't need to restart for the change to land.
        Label {
            text: qsTr("Notifications")
            opacity: 0.7
        }
        CheckBox {
            id: notificationsBox
            Layout.fillWidth: true
            text: qsTr("Show OS notifications for task reminders")
            onToggled: {
                if (!pane.vm) { return; }
                pane.vm.updateNotificationsEnabled(checked);
            }
        }
    }

    Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        opacity: 0.6
        font.pointSize: Qt.application.font.pointSize - 1
        text: qsTr("These preferences apply to the whole application. " +
                   "List-specific defaults live on the next tab.")
    }

    // Push subsequent rows / future controls to the top; the
    // outer StackLayout stretches us.
    Item { Layout.fillHeight: true }
}
