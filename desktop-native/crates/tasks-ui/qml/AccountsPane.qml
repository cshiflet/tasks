// "Accounts" tab of the Settings window.
//
// Lists the sync accounts the user has configured in this session
// and lets them add a new one. Two providers accept credentials
// inline today (CalDAV + EteSync) because they use server URL +
// username + password. Google Tasks and Microsoft To Do appear in
// the picker for awareness but are flagged "coming soon" — their
// OAuth flows require the browser-based PKCE dance, a tokio runtime
// on the bridge side, and OS-native token storage, all tracked in
// PLAN_UPDATES §11.
//
// Persistence is session-local: accounts live in the view model's
// in-memory list and disappear on restart. That matches the
// List tab's current preferences handling; a follow-up commit wires
// both to persistent storage.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ColumnLayout {
    id: pane
    spacing: 12

    required property QtObject vm

    // Keep the provider kind integers lined up with
    // `tasks_sync::ProviderKind` + the `KIND_*` constants in
    // bridge.rs. Order of entries in this array is the ComboBox
    // index and the value passed to `add_password_account`.
    // M-6: provider labels stay clean ("Google Tasks", not
    // "Google Tasks (coming soon)") — the disabled state of the
    // Sign-in button + the description below already convey the
    // gating; baking the suffix into the dropdown label was visual
    // noise.
    readonly property var providerKinds: [
        { index: 0, label: qsTr("CalDAV"), requiresOAuth: false,
          description: qsTr("Radicale, Nextcloud, Fastmail, iCloud, any RFC 4791 server.") },
        { index: 1, label: qsTr("Google Tasks"), requiresOAuth: true,
          description: qsTr("Browser-based sign-in for Google Tasks will land in a future release.") },
        { index: 2, label: qsTr("Microsoft To Do"), requiresOAuth: true,
          description: qsTr("Browser-based sign-in for Microsoft To Do will land in a future release.") },
        { index: 3, label: qsTr("EteSync"), requiresOAuth: false,
          description: qsTr("End-to-end encrypted sync. Use your EteSync server + login password.") },
    ]

    function kindDisplayName(kind) {
        for (let i = 0; i < providerKinds.length; i++) {
            if (providerKinds[i].index === kind) {
                return providerKinds[i].label;
            }
        }
        return qsTr("Unknown (%1)").arg(kind);
    }

    Label {
        text: qsTr("Sync accounts")
        font.bold: true
        font.pointSize: Qt.application.font.pointSize + 1
    }

    // Empty-state hint + the live list of configured accounts.
    Label {
        Layout.fillWidth: true
        visible: !pane.vm || pane.vm.accountLabels.length === 0
        text: qsTr("No sync accounts configured yet. Add one below to enable two-way sync " +
                   "once the sync engine is wired to the UI.")
        wrapMode: Text.Wrap
        opacity: 0.7
    }

    // Per-row: label, provider kind + server (if any), Remove button.
    // The ListView is clipped + scrollable so long account lists
    // don't push the add-form off the pane.
    Frame {
        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(220, Math.max(60, listView.contentHeight + 16))
        visible: pane.vm && pane.vm.accountLabels.length > 0
        padding: 4

        ListView {
            id: listView
            anchors.fill: parent
            clip: true
            model: pane.vm ? pane.vm.accountLabels.length : 0
            spacing: 4
            delegate: RowLayout {
                id: row
                required property int index
                width: listView.width
                spacing: 8

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Label {
                        text: pane.vm ? pane.vm.accountLabels[row.index] : ""
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Label {
                        text: {
                            if (!pane.vm) { return ""; }
                            const kind = pane.kindDisplayName(pane.vm.accountKinds[row.index]);
                            const user = pane.vm.accountUsernames[row.index] ?? "";
                            const server = pane.vm.accountServers[row.index] ?? "";
                            let parts = [kind];
                            if (user.length > 0) { parts.push(user); }
                            if (server.length > 0) { parts.push(server); }
                            return parts.join(" · ");
                        }
                        opacity: 0.65
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                        font.pointSize: Qt.application.font.pointSize - 1
                    }
                }

                Button {
                    text: qsTr("Remove")
                    flat: true
                    onClicked: if (pane.vm) pane.vm.removeAccount(row.index)
                }
            }
        }
    }

    // Divider between the accounts list and the add-account form.
    // Uses Material's foreground colour at low opacity so it renders
    // as a faint line in both themes — the old `rgba(0, 0, 0, 0.12)`
    // disappeared entirely on dark backgrounds.
    Rectangle {
        Layout.fillWidth: true
        height: 1
        color: Material.foreground
        opacity: 0.12
    }

    Label {
        text: qsTr("Add account")
        font.bold: true
    }

    // Add-account form. Fields that don't apply to the current
    // provider dim out rather than disappear so the layout stays
    // stable as the user flips the picker.
    GridLayout {
        Layout.fillWidth: true
        columns: 2
        columnSpacing: 12
        rowSpacing: 8

        Label {
            text: qsTr("Type")
            opacity: 0.7
        }
        ComboBox {
            id: kindBox
            Layout.fillWidth: true
            textRole: "label"
            valueRole: "index"
            model: pane.providerKinds
        }

        Label {
            text: qsTr("Label")
            opacity: 0.7
        }
        TextField {
            id: labelField
            Layout.fillWidth: true
            placeholderText: qsTr("Display name (e.g. \"Fastmail / alice\")")
        }

        // Credential fields collapse entirely for OAuth providers
        // (M-7); GridLayout skips invisible cells, so the form
        // shrinks rather than dimming-out a column of dead inputs.
        Label {
            text: qsTr("Server URL")
            opacity: 0.7
            visible: !pane.providerKinds[kindBox.currentIndex].requiresOAuth
        }
        TextField {
            id: serverField
            Layout.fillWidth: true
            visible: !pane.providerKinds[kindBox.currentIndex].requiresOAuth
            placeholderText: {
                const kind = pane.providerKinds[kindBox.currentIndex].index;
                if (kind === 0) { return qsTr("https://dav.example.com/dav/"); }
                if (kind === 3) { return qsTr("https://api.etebase.com"); }
                return "";
            }
        }

        Label {
            text: qsTr("Username")
            opacity: 0.7
            visible: serverField.visible
        }
        TextField {
            id: userField
            Layout.fillWidth: true
            visible: serverField.visible
            placeholderText: qsTr("username or email")
        }

        Label {
            text: qsTr("Password")
            opacity: 0.7
            visible: serverField.visible
        }
        TextField {
            id: passwordField
            Layout.fillWidth: true
            visible: serverField.visible
            echoMode: TextInput.Password
            placeholderText: {
                const kind = pane.providerKinds[kindBox.currentIndex].index;
                if (kind === 0) { return qsTr("server password or app-specific password"); }
                if (kind === 3) { return qsTr("your EteSync password (used to derive keys)"); }
                return "";
            }
        }
    }

    Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        opacity: 0.65
        font.pointSize: Qt.application.font.pointSize - 1
        text: pane.providerKinds[kindBox.currentIndex].description
    }

    RowLayout {
        Layout.fillWidth: true
        Item { Layout.fillWidth: true }
        Button {
            id: addButton
            text: pane.providerKinds[kindBox.currentIndex].requiresOAuth
                  ? qsTr("Sign in…")
                  : qsTr("Add account")
            highlighted: true
            enabled: !pane.providerKinds[kindBox.currentIndex].requiresOAuth
            ToolTip.visible: hovered && !enabled
            ToolTip.text: qsTr("Browser-based sign-in for this provider is pending and will land in a future release.")
            onClicked: {
                if (!pane.vm) { return; }
                const kind = pane.providerKinds[kindBox.currentIndex].index;
                pane.vm.addPasswordAccount(
                    kind,
                    labelField.text,
                    serverField.text,
                    userField.text,
                    passwordField.text);
                // Clear the form on a successful add. The bridge
                // refuses empty fields, so if any required field was
                // blank the failure shows up on the status bar and
                // the form keeps what the user typed so they can
                // correct it.
                if (pane.vm.accountLabels.length > 0
                    && pane.vm.accountLabels[pane.vm.accountLabels.length - 1]
                       === labelField.text.trim()) {
                    labelField.text = "";
                    serverField.text = "";
                    userField.text = "";
                    passwordField.text = "";
                }
            }
        }
    }

    // Soak up any remaining vertical space so the form sits near
    // the top when the pane is tall.
    Item { Layout.fillHeight: true }
}
