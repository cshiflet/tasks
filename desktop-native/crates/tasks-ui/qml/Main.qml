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
import QtQuick.Window

import com.tasks.desktop

ApplicationWindow {
    id: root
    width: 1100
    height: 720
    visible: true
    title: qsTr("Tasks")

    // Theme override exposed via Settings → General → Appearance.
    // Three states map to Material's enum:
    //   0 = follow OS (Material.System)  — default
    //   1 = light                          (Material.Light)
    //   2 = dark                           (Material.Dark)
    //
    // Persisted on the bridge side: `viewModel.themeMode` is loaded
    // from `<config_dir>/tasks-desktop/preferences.json` on
    // construction and saved on every change via
    // `updateThemeMode(...)`, so a restart picks up the same value.
    // The local `appearanceTheme` shadow lets the toggle paint
    // immediately even if a binding-loop check would refuse a direct
    // self-binding to the view-model property.
    property int appearanceTheme: viewModel.themeMode | 0

    Material.theme: appearanceTheme === 1
                    ? Material.Light
                    : appearanceTheme === 2
                        ? Material.Dark
                        : Material.System
    Material.accent: Material.Blue
    // Override Qt 6 Material's hard-coded AllUppercase casing on
    // Buttons / TabButtons. Set on the ApplicationWindow root so
    // every child control inherits MixedCase via Qt's font-
    // inheritance chain (control.font reads from parent.font when
    // not explicitly overridden, and Material's contentItem hooks
    // up `font: control.font` verbatim).
    font.capitalization: Font.MixedCase

    TaskListViewModel {
        id: viewModel
    }

    // The desktop manages its own database exclusively at the
    // OS-default data path. There's no UI to point at a different
    // file — `openDefaultDatabase` creates the file on first launch
    // and reopens it on every subsequent run.
    //
    // Window geometry: bridge seeds `windowWidth`/`windowHeight`
    // from the saved Preferences blob. A positive saved width/height
    // overrides the default 1100x720; positive x/y restore the prior
    // top-left corner (0/0 is the "no saved position" sentinel —
    // leave the window manager to place the window). Maximised wins
    // over an explicit size: the unmaximise gesture falls back to the
    // most recent unmaximised width/height on the bridge side.
    Component.onCompleted: {
        if (viewModel.windowWidth > 0) { root.width = viewModel.windowWidth; }
        if (viewModel.windowHeight > 0) { root.height = viewModel.windowHeight; }
        if (viewModel.windowX > 0 && viewModel.windowY > 0) {
            root.x = viewModel.windowX;
            root.y = viewModel.windowY;
        }
        if (viewModel.windowMaximized) {
            root.visibility = Window.Maximized;
        }
        viewModel.openDefaultDatabase();
        // Surface the credential-storage disclosure if the user
        // hasn't acknowledged it yet for the active non-keychain
        // tier, OR if the tier is in_memory (which always
        // re-prompts because it discards credentials on exit).
        // Defer one tick so the ApplicationWindow has settled
        // and the dialog opens with sensible centring.
        Qt.callLater(_maybeShowCredentialDisclosure);
    }

    function _maybeShowCredentialDisclosure() {
        if (!viewModel) { return; }
        const tier = viewModel.credentialStorageTier;
        if (tier === "keychain") { return; }
        if (tier === "in_memory") {
            credentialDisclosure.open();
            return;
        }
        if (tier === "encrypted_file" && !viewModel.credentialStorageAcknowledged) {
            credentialDisclosure.open();
        }
    }

    // Pre-flight credential-storage disclosure. Fires on launch
    // when the active tier isn't the OS keychain. For
    // encrypted-file, dismissing flips the acknowledgment flag
    // so the dialog stays put on subsequent launches. For
    // in-memory, the dialog re-pops every launch because the
    // limitation persists across launches.
    Dialog {
        id: credentialDisclosure
        title: qsTr("Credential storage")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok
        contentItem: ColumnLayout {
            // Pinning width here (instead of Layout.maximumWidth on
            // each Label) sidesteps the Dialog's implicitWidth
            // binding loop — the contentItem's geometry is the
            // dialog's only width input, so a fixed value is
            // unambiguous.
            width: 540
            spacing: 8
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                font.bold: true
                text: viewModel.credentialStorageTier === "in_memory"
                    ? qsTr("Credentials are stored in memory only.")
                    : qsTr("Credentials are stored in an encrypted file under your config directory.")
            }
            Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: viewModel.credentialStorageTier === "in_memory"
                    ? qsTr("Sync passwords and OAuth tokens you save during this session " +
                           "will be discarded when the program exits. You will need to " +
                           "re-sign in to every account on the next launch.\n\n" +
                           "Switch to \"Auto\" or \"Master password\" in Settings → " +
                           "General to enable persistence.")
                    : qsTr("The encryption key is derived from a per-machine identifier; " +
                           "anyone with read access to your home directory and the " +
                           "machine-id file can decrypt the credentials. The OS keychain " +
                           "(libsecret on Linux, Keychain on macOS, Credential Manager " +
                           "on Windows) is more secure when available — install / " +
                           "configure one of those to upgrade automatically.\n\n" +
                           "You can also pick \"Master password\" in Settings → " +
                           "General once that mode lands in a future release.")
            }
        }
        onAccepted: {
            if (viewModel.credentialStorageTier !== "in_memory") {
                viewModel.acknowledgeCredentialStorage();
            }
        }
    }

    // Persist on close only — drag/resize fires too often to write
    // through to disk on every event, and we don't want the prefs
    // file to churn during normal interaction.
    onClosing: {
        viewModel.saveWindowGeometry(
            root.width, root.height, root.x, root.y,
            root.visibility === Window.Maximized);
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
    // native absolute path. Three platform wrinkles:
    //
    //   1. URL encoding — spaces and other characters come back as
    //      `%20` etc. decodeURIComponent undoes that.
    //   2. Unix: "file:///home/user/foo" → "/home/user/foo".
    //      Strip the "file://" prefix (7 chars) and keep the leading
    //      slash intact.
    //   3. Windows: "file:///C:/Users/foo" → "C:/Users/foo".
    //      After stripping "file://" we're left with "/C:/...", but
    //      Windows path APIs reject the leading slash, so drop it
    //      when a drive letter follows.
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
    // Hidden by default; toggled via the hamburger button in the
    // command bar (`menuVisible`). When promoted to the macOS
    // global menu bar Qt always shows it regardless of the
    // visibility flag, which is the right behaviour there.
    //
    // The default Material `MenuBarItem` has tall vertical
    // padding (~14 px top + bottom). Override it on the delegate
    // so the menu strip is denser — matches the command bar
    // height below.
    property bool menuVisible: false

    menuBar: MenuBar {
        id: menuBar
        visible: root.menuVisible
        height: visible ? implicitHeight : 0
        delegate: MenuBarItem {
            // Fold the top + bottom padding so the strip fits in
            // ~28 px instead of ~44 px. Material picks up topPadding
            // / bottomPadding for both the touch target and the
            // implicit height, so this drives the row height too.
            topPadding: 4
            bottomPadding: 4
        }

        // Each Menu's `delegate` property only wraps rows added via
        // the actionsModel / addAction APIs — explicit MenuItem
        // children bypass it. Use CompactMenuItem.qml instances
        // directly so the padding override actually lands.
        Menu {
            title: qsTr("&File")
            CompactMenuItem { action: importBackupAction }
            MenuSeparator {}
            CompactMenuItem { action: quitAction }
        }
        Menu {
            title: qsTr("&Edit")
            CompactMenuItem { action: newTaskAction }
            CompactMenuItem { action: editSelectedAction }
            CompactMenuItem { action: deleteSelectedAction }
        }
        Menu {
            title: qsTr("&View")
            CompactMenuItem { action: focusFilterAction }
            CompactMenuItem { action: openSettingsAction }
        }
        Menu {
            title: qsTr("&Help")
            CompactMenuItem { action: aboutAction }
        }
    }

    // ---------- H-1: shared Actions + global shortcuts ----------
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
    // Ctrl+R kicks off a manual sync across every configured
    // account. Same handler the toolbar's Sync button calls; the
    // shortcut just gives keyboard users a one-handed equivalent
    // and matches the "refresh" muscle memory from browsers.
    Action {
        id: syncAllAction
        text: qsTr("Sync now")
        shortcut: "Ctrl+R"
        onTriggered: viewModel.syncAllAccounts()
    }

    // ---------- Transient status auto-clear ----------
    //
    // The status bar is the only surface for ad-hoc messages — the
    // earlier top-of-window toast popup was dropped because the
    // user explicitly asked for status text to live at the bottom-
    // right of the window, not in a centred pop-up. Auto-clear keeps
    // stale errors from sitting in the bar forever; while undo is
    // available the timer pauses so the user has time to act on it.
    readonly property int _statusAutoclearMs: 6000
    Connections {
        target: viewModel
        function onStatusChanged() {
            statusClearTimer.stop();
            if (viewModel.status.length === 0) { return; }
            if (viewModel.lastDeletedId > 0) { return; }
            statusClearTimer.restart();
        }
    }
    Timer {
        id: statusClearTimer
        interval: root._statusAutoclearMs
        repeat: false
        onTriggered: viewModel.status = "";
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

    // Browser-launch fallback for OAuth sign-in. When the bridge
    // can't open the system browser (kiosk / WSL without a
    // DESKTOP env / xdg-open absent), it sets `oauthManualUrl` to
    // the auth URL. We pop a dialog here with a Copy button so
    // the user can paste it into a browser of their choice. The
    // loopback receiver is already listening on 127.0.0.1, so
    // pasting the URL anywhere with network access to localhost
    // (same machine) completes the flow normally. Auto-dismisses
    // when the bridge clears the property (loopback resolved or
    // timed out).
    Dialog {
        id: oauthManualDialog
        title: qsTr("Open this URL to sign in")
        modal: true
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Cancel
        visible: viewModel.oauthManualUrl.length > 0
        ColumnLayout {
            spacing: 12
            Label {
                Layout.maximumWidth: 520
                wrapMode: Text.Wrap
                text: viewModel.oauthManualLabel.length > 0
                      ? qsTr("Couldn't launch a browser automatically. Open this URL to sign in to %1, then return here.").arg(viewModel.oauthManualLabel)
                      : qsTr("Couldn't launch a browser automatically. Open this URL to sign in, then return here.")
            }
            TextField {
                id: oauthManualUrlField
                Layout.fillWidth: true
                Layout.preferredWidth: 520
                readOnly: true
                selectByMouse: true
                text: viewModel.oauthManualUrl
            }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                Button {
                    text: qsTr("Copy URL")
                    onClicked: {
                        oauthManualUrlField.selectAll();
                        oauthManualUrlField.copy();
                    }
                }
            }
        }
    }

    // Single command-bar row in the Win11 / Edge / Files style.
    // The DB path is in the window title where it belongs; the
    // toolbar holds only the actions a user reaches for during a
    // session. Search is the prominent left-side affordance;
    // Import + Settings are right-aligned icon-only buttons with
    // tooltips. Rare File / Edit / View actions live in the menu
    // bar (toggled via the hamburger button on the left) or
    // behind their keyboard shortcuts.
    header: ToolBar {
        implicitHeight: 44
        // Material's ToolBar paints a saturated accent-colour
        // background by default — too loud against the rest of
        // the dark chrome. Strip it so the bar inherits the
        // window's themed background; a thin separator below
        // prevents the toolbar from blending into the SplitView.
        Material.background: "transparent"

        Rectangle {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            height: 1
            color: Material.foreground
            opacity: 0.10
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 6
            anchors.rightMargin: 6
            spacing: 6

            // Hamburger toggle: hides / reveals the menu bar above.
            // Standard Win11 sites the menu trigger as the first
            // command-bar item; the menu starts hidden.
            ToolButton {
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                text: "\u{2630}"                 // ☰ trigram
                font.pointSize: Qt.application.font.pointSize + 4
                ToolTip.visible: hovered
                ToolTip.text: root.menuVisible
                              ? qsTr("Hide menu")
                              : qsTr("Show menu")
                onClicked: root.menuVisible = !root.menuVisible
            }

            // Search field. Magnifier glyph rendered as an inline
            // prefix via leftPadding + an anchored Label so we
            // don't need an icon font. Esc clears the field, which
            // restores the active sidebar filter via the bridge's
            // empty-query branch. Height is pinned a few pixels
            // shorter than the bar so the rounded edges sit inside
            // the chrome — the default TextField wants to be 40 px
            // tall and would clip the bar otherwise.
            TextField {
                id: searchField
                Layout.fillWidth: true
                Layout.maximumWidth: 480
                Layout.preferredHeight: 32
                leftPadding: 32
                placeholderText: qsTr("Search tasks…")
                onTextChanged: viewModel.setSearchQuery(text)
                Keys.onEscapePressed: { text = ""; }
                // See CompactTextField.qml — Material's default
                // filled container locks to a Light-theme grey on
                // Windows even when the rest of the window resolves
                // dark. Override the background outright so it
                // paints transparently with a themed border in
                // either theme. (`Material.containerStyle` would do
                // this cleanly but only landed in Qt 6.5.)
                background: Rectangle {
                    color: "transparent"
                    radius: 2
                    border.width: searchField.activeFocus ? 2 : 1
                    border.color: searchField.activeFocus
                        ? searchField.Material.accentColor
                        : searchField.Material.foreground
                    opacity: searchField.activeFocus ? 1.0 : 0.45
                }
                // Painted magnifier — same approach the sidebar
                // chevrons use (see SidebarPane.qml). The U+1F50D 🔍
                // emoji needed a colour-emoji font that Linux hosts
                // don't ship by default, leaving this slot rendering
                // as an empty box. A circle + handle drawn via
                // Canvas works on every OS without extra fonts.
                Canvas {
                    id: magnifier
                    width: 14
                    height: 14
                    anchors.left: parent.left
                    anchors.leftMargin: 10
                    anchors.verticalCenter: parent.verticalCenter
                    opacity: 0.55
                    onPaint: {
                        const ctx = getContext("2d");
                        ctx.reset();
                        ctx.lineWidth = 1.6;
                        ctx.lineCap = "round";
                        ctx.strokeStyle = searchField.Material.foreground;
                        // Glass — circle in the upper-left.
                        ctx.beginPath();
                        ctx.arc(5.5, 5.5, 4, 0, 2 * Math.PI);
                        ctx.stroke();
                        // Handle — diagonal toward the lower-right.
                        ctx.beginPath();
                        ctx.moveTo(8.5, 8.5);
                        ctx.lineTo(12.5, 12.5);
                        ctx.stroke();
                    }
                }
            }

            Item { Layout.fillWidth: true }   // flexible spacer

            // ToolButtons render the glyph through the `text`
            // property when `display` is at its default
            // `TextOnly` — the previous version used
            // `display: IconOnly` which strips text rendering, so
            // the buttons rendered as transparent click targets.
            //
            // Manual Sync button — fans out a sync_account dispatch
            // against every non-OAuth account. The actual cycle
            // runs on a worker thread; status flows through the
            // bottom status bar ("Syncing <label>…" → "<label>:
            // Done").
            ToolButton {
                id: syncAllButton
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Sync all accounts")
                onClicked: viewModel.syncAllAccounts()
                // True while any account's state is "Syncing…".
                // The icon spins while this is high; lands snapping
                // back to its rest angle when the last in-flight
                // account flips back to "Done" / "Idle" / an error.
                readonly property bool syncInFlight: {
                    if (!viewModel.accountSyncStates) { return false; }
                    for (let i = 0; i < viewModel.accountSyncStates.length; i++) {
                        if (viewModel.accountSyncStates[i] === "Syncing…") {
                            return true;
                        }
                    }
                    return false;
                }
                contentItem: Canvas {
                    id: syncIcon
                    implicitWidth: 18
                    implicitHeight: 18
                    // Spin while a sync is in flight. Counter-clockwise
                    // matches the painted arrowhead direction and the
                    // browser-refresh muscle memory.
                    RotationAnimation on rotation {
                        running: syncAllButton.syncInFlight
                        from: 0; to: 360
                        duration: 900
                        loops: Animation.Infinite
                    }
                    onPaint: {
                        // Two arrows forming a circular refresh
                        // glyph — same Canvas approach as the
                        // recurring-task indicator, scaled up.
                        const ctx = getContext("2d");
                        ctx.reset();
                        ctx.lineWidth = 1.6;
                        ctx.lineCap = "round";
                        ctx.lineJoin = "round";
                        ctx.strokeStyle = syncAllButton.Material.foreground;
                        ctx.fillStyle = syncAllButton.Material.foreground;
                        const cx = width / 2;
                        const cy = height / 2;
                        const r = Math.min(width, height) / 2 - 2;
                        // Two ~150° arcs leaving small gaps for the
                        // arrowheads at the right and left.
                        ctx.beginPath();
                        ctx.arc(cx, cy, r, -Math.PI * 0.4, Math.PI * 0.4);
                        ctx.stroke();
                        ctx.beginPath();
                        ctx.arc(cx, cy, r, Math.PI * 0.6, Math.PI * 1.4);
                        ctx.stroke();
                        const head = 3.2;
                        // Right arrowhead — points down.
                        const xR = cx + r * Math.cos(Math.PI * 0.4);
                        const yR = cy + r * Math.sin(Math.PI * 0.4);
                        ctx.beginPath();
                        ctx.moveTo(xR, yR);
                        ctx.lineTo(xR - head, yR - head * 0.6);
                        ctx.lineTo(xR + head * 0.4, yR - head);
                        ctx.closePath();
                        ctx.fill();
                        // Left arrowhead — points up.
                        const xL = cx + r * Math.cos(Math.PI * 1.4);
                        const yL = cy + r * Math.sin(Math.PI * 1.4);
                        ctx.beginPath();
                        ctx.moveTo(xL, yL);
                        ctx.lineTo(xL + head, yL + head * 0.6);
                        ctx.lineTo(xL - head * 0.4, yL + head);
                        ctx.closePath();
                        ctx.fill();
                    }
                }
            }
            ToolButton {
                action: openSettingsAction
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                text: "\u{2699}"              // ⚙ gear
                font.pointSize: Qt.application.font.pointSize + 4
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Settings")
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
        appWindow: root
    }

    footer: ToolBar {
        // Pin the status bar height so a long error string can't
        // grow the bar and shove the SplitView upward.
        implicitHeight: 26
        // Same color treatment as the header — no Material accent.
        Material.background: "transparent"

        Rectangle {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            height: 1
            color: Material.foreground
            opacity: 0.10
        }
        // Status messages live at the bottom-right per the user's
        // direction; the Undo button sits to the right of the
        // status text whenever the bridge has a pinned
        // last-deleted task. Clearing the undo state also clears
        // the status string so the bar empties cleanly.
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 8
            anchors.rightMargin: 8
            spacing: 8

            Item { Layout.fillWidth: true }   // pushes content right
            Label {
                text: viewModel.status
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignRight
                font.pointSize: Qt.application.font.pointSize - 1
                opacity: 0.7
            }
            Button {
                visible: viewModel.lastDeletedId > 0
                text: qsTr("Undo")
                flat: true
                highlighted: true
                topPadding: 0
                bottomPadding: 0
                leftPadding: 8
                rightPadding: 8
                onClicked: {
                    viewModel.restoreLastDeleted();
                    viewModel.clearLastDeleted();
                    viewModel.status = "";
                }
            }
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
