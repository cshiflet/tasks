// "General" tab of the Settings window.
//
// Holds the app-wide preferences that aren't tied to any one list:
// today appearance, OS notifications, and the credential storage
// tier (keychain / encrypted file / in-memory). Future global
// toggles belong here too.
//
// Persistence rides on the bridge's Preferences blob — every
// change writes back via the matching update_* invokable, so a
// restart picks up the same values. List-specific preferences
// live in the "List defaults" tab.
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
        // Credential storage radios — pinned by `credentialStorageChoice`
        // (the user's request) rather than `credentialStorageTier`
        // (what auto-probe actually landed on). The "(active: …)" line
        // below the radios surfaces the gap when the two differ.
        const choice = vm.credentialStorageChoice || "auto";
        if (choice === "in_memory") {
            credAutoRadio.checked = false;
            credInMemoryRadio.checked = true;
        } else {
            credAutoRadio.checked = true;
            credInMemoryRadio.checked = false;
        }
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

    // ---------- Credential storage ----------
    //
    // Radios for the user's *requested* storage tier; the bridge
    // probes the actual one at runtime and surfaces it via
    // `credentialStorageTier`. Switching to in-memory triggers a
    // confirmation dialog because it discards persistent
    // credentials and forces re-sign-in on every launch.

    Rectangle {
        Layout.fillWidth: true
        height: 1
        color: Material.foreground
        opacity: 0.12
    }

    Label {
        text: qsTr("Credential storage")
        font.bold: true
    }
    Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        opacity: 0.7
        font.pointSize: Qt.application.font.pointSize - 1
        text: qsTr("Where the app keeps your sync-account passwords " +
                   "and OAuth tokens. Switching tiers migrates existing " +
                   "credentials into the new store and removes them from " +
                   "the old one.")
    }

    ColumnLayout {
        spacing: 4
        RadioButton {
            id: credAutoRadio
            text: qsTr("Auto — best available (OS keychain → encrypted file)")
            checked: true
            onToggled: {
                if (!checked || !pane.vm) { return; }
                if (pane.vm.credentialStorageChoice === "auto") { return; }
                pane.vm.updateCredentialStorageChoice("auto");
            }
        }
        RadioButton {
            id: credInMemoryRadio
            text: qsTr("In-memory only — re-sign-in required every launch")
            onToggled: {
                if (!checked || !pane.vm) { return; }
                if (pane.vm.credentialStorageChoice === "in_memory") { return; }
                inMemoryConfirm.open();
            }
        }
        // Master-password row — placeholder. The bridge ignores
        // this value today (probe falls back to "auto") and the
        // radio is disabled to make the trade-off visible without
        // letting users land in a half-implemented mode.
        RadioButton {
            text: qsTr("Master password (coming soon)")
            enabled: false
            opacity: 0.55
            ToolTip.visible: hovered
            ToolTip.text: qsTr("Encrypted file unlocked with a passphrase you set. " +
                               "Lands with the next batch.")
        }
    }
    Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        opacity: 0.6
        font.pointSize: Qt.application.font.pointSize - 1
        text: pane.vm
            ? qsTr("Active: %1").arg(_describeTier(pane.vm.credentialStorageTier))
            : ""
    }

    function _describeTier(name) {
        if (name === "keychain") {
            return qsTr("OS keychain");
        }
        if (name === "encrypted_file") {
            return qsTr("encrypted file (machine-id key)");
        }
        if (name === "in_memory") {
            return qsTr("in-memory only — credentials lost on exit");
        }
        return qsTr("(unknown: %1)").arg(name);
    }

    // Confirmation dialog shown when the user picks the in-memory
    // radio. Cancelling reverts the radio to whatever the bridge
    // currently reports.
    Dialog {
        id: inMemoryConfirm
        title: qsTr("Switch to in-memory credential storage?")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Yes | Dialog.Cancel
        Label {
            wrapMode: Text.Wrap
            width: 480
            text: qsTr("Switching to in-memory will remove your saved credentials " +
                       "from %1 and they will be lost when the program exits. You " +
                       "will need to re-sign in to every sync account on the next " +
                       "launch.\n\nContinue?")
                .arg(pane.vm ? pane._describeTier(pane.vm.credentialStorageTier) : "")
        }
        onAccepted: {
            if (!pane.vm) { return; }
            pane.vm.updateCredentialStorageChoice("in_memory");
        }
        onRejected: {
            // Revert the radio.
            pane.loadFromVm();
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
