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

ScrollView {
    id: pane
    // Pin the Material context so child Labels resolve against the
    // window's actual colour scheme. See ListSettingsPane.qml.
    Material.accent: Material.Blue
    // Pin the content's horizontal extent to the viewport so the
    // inner ColumnLayout doesn't blow out and produce a phantom
    // horizontal scrollbar.
    contentWidth: availableWidth
    clip: true
    // Always show the vertical scrollbar so it reserves its width
    // inside `availableWidth`; otherwise the overlay scrollbar
    // sits on top of the form on the right edge.
    ScrollBar.vertical.policy: ScrollBar.AlwaysOn
    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

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

    // Inner column lays out the form vertically and is what
    // actually scrolls when the content exceeds the ScrollView's
    // viewport. Pinning `width` to `pane.availableWidth` keeps
    // children sized to fit horizontally without producing a
    // horizontal scrollbar.
    ColumnLayout {
        id: column
        // Subtract the Material ScrollBar's typical visible width
        // (its `implicitWidth` plus a few px of breathing room) so
        // the inner controls never tuck under the always-on
        // vertical bar. 23 px clears the bar plus a small visual
        // gap; tighter values had the scrollbar resting right
        // against the password field's border.
        width: pane.availableWidth - 23
        spacing: 12

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
        // Floor bumped so a single-row account with the three-line
        // ColumnLayout (label / kind+server / sync-state badge) +
        // its two trailing buttons doesn't overflow the Frame.
        // contentHeight + 24 keeps the binding reactive when more
        // rows arrive or the badge appears.
        Layout.preferredHeight: Math.min(260, Math.max(96, listView.contentHeight + 24))
        visible: pane.vm && pane.vm.accountLabels.length > 0
        padding: 8

        ListView {
            id: listView
            anchors.fill: parent
            clip: true
            model: pane.vm ? pane.vm.accountLabels.length : 0
            spacing: 6
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
                    // Per-account sync state badge — populated from
                    // `account_sync_states[index]` ("Idle",
                    // "Syncing…", "Synced (N↓ / M↑)", "Failed: …").
                    // Hidden while idle so the row stays clean.
                    Label {
                        visible: pane.vm
                                 && pane.vm.accountSyncStates
                                 && pane.vm.accountSyncStates[row.index]
                                 && pane.vm.accountSyncStates[row.index] !== "Idle"
                        text: pane.vm ? pane.vm.accountSyncStates[row.index] : ""
                        opacity: 0.85
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                        font.pointSize: Qt.application.font.pointSize - 1
                        font.italic: true
                    }
                }

                // CalDAV / EteSync rows get a Sync now button. OAuth
                // providers (kind 1 / 2) hide it because their sign-in
                // path isn't wired yet.
                Button {
                    text: qsTr("Sync now")
                    flat: true
                    visible: pane.vm
                             && (pane.vm.accountKinds[row.index] === 0
                                 || pane.vm.accountKinds[row.index] === 3)
                    onClicked: {
                        if (!pane.vm) { return; }
                        pane.vm.syncAccount(pane.vm.accountUuids[row.index]);
                    }
                }

                // CalDAV / EteSync rows get a "New list" button that
                // pops a dialog asking for a name + creates the
                // calendar on the server. Hidden for OAuth providers
                // until their sign-in flow lands.
                Button {
                    text: qsTr("New list")
                    flat: true
                    visible: pane.vm
                             && (pane.vm.accountKinds[row.index] === 0
                                 || pane.vm.accountKinds[row.index] === 3)
                    onClicked: if (pane.vm) newListDialog.openFor(row.index)
                }

                Button {
                    text: qsTr("Edit")
                    flat: true
                    onClicked: if (pane.vm) editDialog.openFor(row.index)
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
        CompactComboBox {
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
        CompactTextField {
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
        CompactTextField {
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
        CompactTextField {
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
        CompactTextField {
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
        // Test runs `provider.connect()` against the entered creds
        // without persisting — the result lands on the status bar.
        // Hidden for OAuth providers because their sign-in flow
        // isn't a credentials-only test.
        Button {
            text: qsTr("Test")
            flat: true
            visible: !pane.providerKinds[kindBox.currentIndex].requiresOAuth
            onClicked: {
                if (!pane.vm) { return; }
                pane.vm.testAccountConnection(
                    pane.providerKinds[kindBox.currentIndex].index,
                    serverField.text,
                    userField.text,
                    passwordField.text);
            }
        }
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

    // Inline edit dialog. Pre-fills with the row's current values;
    // an empty password field on save preserves the existing one.
    Dialog {
        id: editDialog
        modal: true
        title: qsTr("Edit account")
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Save | Dialog.Cancel
        // Stash so onAccepted can pass it back to the bridge.
        property string targetUuid: ""

        function openFor(idx) {
            if (!pane.vm) { return; }
            editDialog.targetUuid = pane.vm.accountUuids[idx] ?? "";
            editLabel.text = pane.vm.accountLabels[idx] ?? "";
            editServer.text = pane.vm.accountServers[idx] ?? "";
            editUser.text = pane.vm.accountUsernames[idx] ?? "";
            editPassword.text = "";
            open();
        }

        contentItem: ColumnLayout {
            spacing: 8
            implicitWidth: 380

            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: 12
                rowSpacing: 8

                Label { text: qsTr("Label"); opacity: 0.7 }
                CompactTextField { id: editLabel; Layout.fillWidth: true }

                Label { text: qsTr("Server URL"); opacity: 0.7 }
                CompactTextField { id: editServer; Layout.fillWidth: true }

                Label { text: qsTr("Username"); opacity: 0.7 }
                CompactTextField { id: editUser; Layout.fillWidth: true }

                Label { text: qsTr("Password"); opacity: 0.7 }
                CompactTextField {
                    id: editPassword
                    Layout.fillWidth: true
                    echoMode: TextInput.Password
                    placeholderText: qsTr("(leave blank to keep current)")
                }
            }
        }

        onAccepted: {
            if (!pane.vm || editDialog.targetUuid.length === 0) { return; }
            pane.vm.updatePasswordAccount(
                editDialog.targetUuid,
                editLabel.text,
                editServer.text,
                editUser.text,
                editPassword.text);
        }
    }

    // New-list dialog. Asks for a display name; creates the
    // calendar on the account's server then triggers a sync so the
    // row lands in caldav_lists and the sidebar refreshes.
    Dialog {
        id: newListDialog
        modal: true
        title: qsTr("Create list")
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        property string targetUuid: ""
        property string targetLabel: ""

        function openFor(idx) {
            if (!pane.vm) { return; }
            newListDialog.targetUuid = pane.vm.accountUuids[idx] ?? "";
            newListDialog.targetLabel = pane.vm.accountLabels[idx] ?? "";
            newListField.text = "";
            open();
        }

        contentItem: ColumnLayout {
            spacing: 8
            implicitWidth: 320

            Label {
                Layout.fillWidth: true
                text: newListDialog.targetLabel.length > 0
                      ? qsTr("Create a new list on \"%1\".").arg(newListDialog.targetLabel)
                      : qsTr("Create a new list.")
                opacity: 0.7
                wrapMode: Text.Wrap
            }
            CompactTextField {
                id: newListField
                Layout.fillWidth: true
                placeholderText: qsTr("List name (e.g. \"Inbox\" or \"Groceries\")")
            }
        }

        onAccepted: {
            if (!pane.vm
                || newListDialog.targetUuid.length === 0
                || newListField.text.trim().length === 0) {
                return;
            }
            // 0 = no colour (bridge converts to None for the
            // provider). A future revision can pop a colour swatch.
            pane.vm.createAccountCalendar(
                newListDialog.targetUuid,
                newListField.text,
                0);
        }
    }
    }   // close inner ColumnLayout
}       // close ScrollView
