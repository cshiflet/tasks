// Sidebar listing built-in filters, every configured account, and
// custom filters.
//
// Rows come from the bridge's parallel `sidebarLabels`/`sidebarIds`
// /`sidebarGroups`/`sidebarAccountKinds` arrays. The id prefix
// determines how each row is rendered:
//   `__…__`            — built-in filter (All / Today / Recent)
//   `account:<uuid>`   — account section header (collapse target)
//   `caldav:<uuid>`    — list under an account
//   `filter:<id>`      — saved custom filter
// `sidebarGroups[i]` is the group key the collapse logic uses —
// "filters_builtin", "saved", or `account:<uuid>` (so each
// account's own header + its lists share one collapse state).
// Right-clicking an account header opens a menu with New list /
// Sync now / Edit / Remove.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

Pane {
    id: root
    padding: 0
    // Belt-and-braces theme propagation; see TaskDetailPane.qml.
    Material.accent: Material.Blue
    required property QtObject vm

    // Wired to Main.qml's `Focus sidebar` action (Ctrl+F). Brings
    // keyboard focus to the ListView so arrow keys navigate between
    // filters.
    function focusList() {
        sidebarList.forceActiveFocus();
    }

    // Read the bridge-emitted group key for a row. Each row's group
    // is one of "filters_builtin", "saved", or `account:<uuid>`.
    function _groupOfIndex(idx) {
        if (!root.vm) { return ""; }
        const groups = root.vm.sidebarGroups;
        return (groups && idx < groups.length) ? groups[idx] : "";
    }
    // Static label for the well-known section keys. Account-keyed
    // groups use the row's own label as the header text — see the
    // delegate below.
    function _groupLabel(group) {
        if (group === "filters_builtin") { return qsTr("Quick filters"); }
        if (group === "saved")           { return qsTr("Saved filters"); }
        return "";
    }
    // Strip the `account:` prefix off a group key to recover the
    // owning `caldav_accounts.cda_uuid`. Returns "" for non-account
    // groups; callers gate the right-click menu on the result.
    function _accountUuidOf(group) {
        if (group && group.startsWith("account:")) {
            return group.slice("account:".length);
        }
        return "";
    }

    // Per-group collapsed flags. Defaults to expanded; toggling
    // reassigns the whole object so the bindings on each row re-evaluate.
    property var collapsedGroups: ({})

    function _isCollapsed(group) {
        return collapsedGroups[group] === true;
    }

    function _toggleGroup(group) {
        const next = Object.assign({}, collapsedGroups);
        next[group] = !next[group];
        collapsedGroups = next;
    }

    // Material-palette swatch picker reused by every list row's
    // right-click → Choose colour… action. Click a swatch to
    // commit; "Default" clears the colour back to neutral grey.
    Dialog {
        id: colorPicker
        modal: true
        title: qsTr("Choose colour")
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Cancel
        property string targetUuid: ""
        property string targetLabel: ""
        // Material 500-level swatches across the standard palette,
        // plus a leading "default / no colour" sentinel that maps to
        // 0 on the bridge side.
        readonly property var swatches: [
            { name: qsTr("Default"), argb: 0 },
            { name: "Red",         argb: 0xffd32f2f },
            { name: "Pink",        argb: 0xffe91e63 },
            { name: "Purple",      argb: 0xff9c27b0 },
            { name: "Deep Purple", argb: 0xff673ab7 },
            { name: "Indigo",      argb: 0xff3f51b5 },
            { name: "Blue",        argb: 0xff1976d2 },
            { name: "Cyan",        argb: 0xff00bcd4 },
            { name: "Teal",        argb: 0xff009688 },
            { name: "Green",       argb: 0xff4caf50 },
            { name: "Lime",        argb: 0xffcddc39 },
            { name: "Yellow",      argb: 0xfffbc02d },
            { name: "Orange",      argb: 0xfff57c00 },
            { name: "Deep Orange", argb: 0xffe64a19 },
            { name: "Brown",       argb: 0xff795548 },
            { name: "Grey",        argb: 0xff757575 },
            { name: "Blue Grey",   argb: 0xff607d8b }
        ]

        function openFor(label, uuid) {
            colorPicker.targetLabel = label;
            colorPicker.targetUuid = uuid;
            open();
        }

        contentItem: ColumnLayout {
            spacing: 8
            implicitWidth: 320
            Label {
                Layout.fillWidth: true
                opacity: 0.7
                wrapMode: Text.Wrap
                text: colorPicker.targetLabel.length > 0
                      ? qsTr("Colour for \"%1\".").arg(colorPicker.targetLabel)
                      : qsTr("Choose a colour.")
            }
            GridLayout {
                Layout.fillWidth: true
                columns: 6
                rowSpacing: 6
                columnSpacing: 6

                Repeater {
                    model: colorPicker.swatches
                    delegate: Rectangle {
                        required property var modelData
                        Layout.preferredWidth: 36
                        Layout.preferredHeight: 36
                        radius: 18
                        // Convert the i32 ARGB into a Qt color. 0 →
                        // a transparent ring with an "x" so the user
                        // can tell which swatch clears the colour.
                        property int argb: modelData.argb | 0
                        color: {
                            if (argb === 0) { return "transparent"; }
                            const a = ((argb >>> 24) & 0xff) / 255.0;
                            const r = ((argb >>> 16) & 0xff) / 255.0;
                            const g = ((argb >>>  8) & 0xff) / 255.0;
                            const b = ( argb         & 0xff) / 255.0;
                            return Qt.rgba(r, g, b, a);
                        }
                        border.width: argb === 0 ? 1 : 0
                        border.color: Material.foreground
                        Label {
                            visible: parent.argb === 0
                            anchors.centerIn: parent
                            text: "✕"
                            opacity: 0.6
                        }
                        ToolTip.visible: hover.hovered
                        ToolTip.text: modelData.name
                        HoverHandler { id: hover }
                        TapHandler {
                            onTapped: {
                                if (!root.vm
                                    || colorPicker.targetUuid.length === 0) {
                                    return;
                                }
                                root.vm.updateListColor(
                                    colorPicker.targetUuid,
                                    parent.argb | 0);
                                colorPicker.close();
                            }
                        }
                    }
                }
            }
        }
    }

    // Inline "New list" dialog reused by every account header's
    // right-click → New list... action. Lives at the pane root so
    // the per-row TapHandler can openFor() into it.
    Dialog {
        id: newListInline
        modal: true
        title: qsTr("Create list")
        anchors.centerIn: Overlay.overlay
        standardButtons: Dialog.Ok | Dialog.Cancel
        property string accountUuid: ""
        property string accountLabel: ""

        function openFor(label, uuid) {
            newListInline.accountLabel = label;
            newListInline.accountUuid = uuid;
            newListField.text = "";
            open();
        }

        contentItem: ColumnLayout {
            spacing: 8
            implicitWidth: 320
            Label {
                Layout.fillWidth: true
                opacity: 0.7
                wrapMode: Text.Wrap
                text: newListInline.accountLabel.length > 0
                      ? qsTr("Create a list on \"%1\".").arg(newListInline.accountLabel)
                      : qsTr("Create a list.")
            }
            CompactTextField {
                id: newListField
                Layout.fillWidth: true
                placeholderText: qsTr("List name (e.g. \"Inbox\" or \"Groceries\")")
            }
        }

        onAccepted: {
            if (!root.vm
                || newListInline.accountUuid.length === 0
                || newListField.text.trim().length === 0) {
                return;
            }
            root.vm.createAccountCalendar(
                newListInline.accountUuid,
                newListField.text,
                0);
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        ListView {
            id: sidebarList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            boundsBehavior: Flickable.StopAtBounds
            model: root.vm ? root.vm.sidebarLabels.length : 0
            spacing: 0

            delegate: Column {
                id: row
                required property int index
                width: sidebarList.width

                property string myGroup: root.vm ? root._groupOfIndex(row.index) : ""
                // sidebarIds / sidebarLabels are parallel QStringLists
                // rebuilt by the bridge whenever the sidebar shape
                // changes (account add/remove, list create, sync
                // refresh). The ListView delegate keeps its `index`
                // through that rebuild but for one frame the array
                // length and the count may disagree — guard the
                // lookup so an out-of-range read coalesces to "" and
                // QML doesn't warn "Unable to assign [undefined] to
                // QString".
                property string myId: (root.vm
                    && row.index < root.vm.sidebarIds.length)
                    ? root.vm.sidebarIds[row.index]
                    : ""
                property string myLabel: (root.vm
                    && row.index < root.vm.sidebarLabels.length)
                    ? root.vm.sidebarLabels[row.index]
                    : ""
                // True when the row is itself an account-section
                // header (id `account:<uuid>`). These render as a
                // tinted strip and toggle their group's collapse
                // state on click; right-click pops the per-account
                // context menu (New list / Sync now / Edit / Remove).
                property bool _isAccountHeader: row.myId.startsWith("account:")
                // True for built-in / saved-filter rows that need
                // their well-known section header injected above
                // them whenever the previous row was in a different
                // group.
                property bool _isStaticSectionStart: {
                    if (!row.myGroup || row._isAccountHeader) { return false; }
                    if (row.index === 0) { return true; }
                    return root._groupOfIndex(row.index - 1) !== row.myGroup;
                }
                property bool _groupCollapsed: root._isCollapsed(row.myGroup)

                // Static section header (Quick filters / Saved
                // filters). Account groups don't need this — the
                // account-header row IS the strip.
                ItemDelegate {
                    visible: row._isStaticSectionStart
                    width: row.width
                    implicitHeight: visible ? 22 : 0
                    height: implicitHeight
                    topPadding: 0
                    bottomPadding: 0
                    leftPadding: 8
                    rightPadding: 8
                    onClicked: root._toggleGroup(row.myGroup)
                    background: Rectangle {
                        color: Material.foreground
                        opacity: 0.08
                    }
                    contentItem: RowLayout {
                        spacing: 6
                        Canvas {
                            Layout.preferredWidth: 12
                            Layout.preferredHeight: 12
                            rotation: row._groupCollapsed ? -90 : 0
                            opacity: 0.75
                            Behavior on rotation {
                                NumberAnimation { duration: 120 }
                            }
                            onPaint: {
                                const ctx = getContext("2d");
                                ctx.reset();
                                ctx.lineWidth = 1.6;
                                ctx.lineCap = "round";
                                ctx.lineJoin = "round";
                                ctx.strokeStyle = Material.foreground;
                                ctx.beginPath();
                                ctx.moveTo(2, 4);
                                ctx.lineTo(width / 2, height - 4);
                                ctx.lineTo(width - 2, 4);
                                ctx.stroke();
                            }
                        }
                        Label {
                            Layout.fillWidth: true
                            text: root._groupLabel(row.myGroup)
                            font.bold: true
                            font.pointSize: Qt.application.font.pointSize - 1
                            opacity: 0.75
                            elide: Text.ElideRight
                        }
                    }
                }

                // Account-section header. Displays the account's own
                // label as the strip text and supports right-click
                // → New list / Sync now / Edit / Remove.
                ItemDelegate {
                    id: accountHeader
                    visible: row._isAccountHeader
                    width: row.width
                    // Header height grows from 26 to 32 to give the
                    // per-account sync button a more clickable target
                    // without scrunching the icon. The collapse arrow
                    // and label both centre vertically inside the
                    // RowLayout, so the extra ~6 px is invisible aside
                    // from a slightly larger hit area for the button.
                    implicitHeight: visible ? 32 : 0
                    height: implicitHeight
                    topPadding: 0
                    bottomPadding: 0
                    leftPadding: 8
                    rightPadding: 8
                    onClicked: root._toggleGroup(row.myGroup)
                    background: Rectangle {
                        color: Material.foreground
                        opacity: 0.10
                    }
                    // True for the built-in Local account (non-sync,
                    // can't sync — hide the sync button there).
                    readonly property bool _isLocalAccount:
                        root._accountUuidOf(row.myGroup) === "local-default"
                    contentItem: RowLayout {
                        spacing: 6
                        Canvas {
                            Layout.preferredWidth: 12
                            Layout.preferredHeight: 12
                            rotation: row._groupCollapsed ? -90 : 0
                            opacity: 0.75
                            Behavior on rotation {
                                NumberAnimation { duration: 120 }
                            }
                            onPaint: {
                                const ctx = getContext("2d");
                                ctx.reset();
                                ctx.lineWidth = 1.6;
                                ctx.lineCap = "round";
                                ctx.lineJoin = "round";
                                ctx.strokeStyle = Material.foreground;
                                ctx.beginPath();
                                ctx.moveTo(2, 4);
                                ctx.lineTo(width / 2, height - 4);
                                ctx.lineTo(width - 2, 4);
                                ctx.stroke();
                            }
                        }
                        Label {
                            Layout.fillWidth: true
                            text: row.myLabel
                            font.bold: true
                            font.pointSize: Qt.application.font.pointSize - 1
                            elide: Text.ElideRight
                        }
                        // Per-account sync affordance, mirrors the
                        // command-bar Sync button. Hidden for the
                        // built-in Local account (nothing to sync)
                        // and for any in-flight sync of *this*
                        // account (the icon spins while the bridge's
                        // accountSyncStates entry is "Syncing…").
                        // Right-click → Sync now still works as the
                        // keyboard-equivalent path for completeness.
                        ToolButton {
                            id: rowSyncButton
                            visible: !accountHeader._isLocalAccount
                            Layout.preferredWidth: 28
                            Layout.preferredHeight: 28
                            ToolTip.visible: hovered
                            ToolTip.text: qsTr("Sync %1").arg(row.myLabel)
                            // Stop propagation so the click doesn't
                            // also collapse/expand the section.
                            onClicked: {
                                if (!root.vm) { return; }
                                root.vm.syncAccount(
                                    root._accountUuidOf(row.myGroup));
                            }
                            // Map this row's account to its index in
                            // the bridge's parallel accounts arrays
                            // so we can read its current sync state.
                            // accountSyncStates is indexed alongside
                            // accountUuids; both populated from the
                            // same load_password_accounts pass.
                            readonly property int _accountIndex: {
                                if (!root.vm || !root.vm.accountUuids) {
                                    return -1;
                                }
                                const u = root._accountUuidOf(row.myGroup);
                                for (let i = 0; i < root.vm.accountUuids.length; i++) {
                                    if (root.vm.accountUuids[i] === u) {
                                        return i;
                                    }
                                }
                                return -1;
                            }
                            readonly property bool _syncing:
                                _accountIndex >= 0
                                && root.vm
                                && root.vm.accountSyncStates
                                && root.vm.accountSyncStates[_accountIndex] === "Syncing…"
                            contentItem: Canvas {
                                id: rowSyncIcon
                                implicitWidth: 18
                                implicitHeight: 18
                                RotationAnimation on rotation {
                                    running: rowSyncButton._syncing
                                    from: 0; to: 360
                                    duration: 900
                                    loops: Animation.Infinite
                                }
                                onPaint: {
                                    // Same two-arc refresh glyph the
                                    // command-bar button uses, scaled
                                    // down for the sidebar row height.
                                    const ctx = getContext("2d");
                                    ctx.reset();
                                    ctx.lineWidth = 1.4;
                                    ctx.lineCap = "round";
                                    ctx.lineJoin = "round";
                                    ctx.strokeStyle = rowSyncButton.Material.foreground;
                                    ctx.fillStyle = rowSyncButton.Material.foreground;
                                    const cx = width / 2;
                                    const cy = height / 2;
                                    const r = Math.min(width, height) / 2 - 2;
                                    ctx.beginPath();
                                    ctx.arc(cx, cy, r, -Math.PI * 0.4, Math.PI * 0.4);
                                    ctx.stroke();
                                    ctx.beginPath();
                                    ctx.arc(cx, cy, r, Math.PI * 0.6, Math.PI * 1.4);
                                    ctx.stroke();
                                    const head = 2.5;
                                    const xR = cx + r * Math.cos(Math.PI * 0.4);
                                    const yR = cy + r * Math.sin(Math.PI * 0.4);
                                    ctx.beginPath();
                                    ctx.moveTo(xR, yR);
                                    ctx.lineTo(xR - head, yR - head * 0.6);
                                    ctx.lineTo(xR + head * 0.4, yR - head);
                                    ctx.closePath();
                                    ctx.fill();
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
                    }

                    // Right-click context menu. Wired to the same
                    // bridge invokables the Accounts pane uses.
                    TapHandler {
                        acceptedButtons: Qt.RightButton
                        onTapped: accountMenu.popup()
                    }
                    Menu {
                        id: accountMenu
                        MenuItem {
                            text: qsTr("New list…")
                            onTriggered: newListInline.openFor(row.myLabel,
                                                              root._accountUuidOf(row.myGroup))
                        }
                        MenuItem {
                            text: qsTr("Sync now")
                            onTriggered: {
                                if (!root.vm) { return; }
                                root.vm.syncAccount(root._accountUuidOf(row.myGroup));
                            }
                        }
                        // "Remove account" deliberately lives only in
                        // Settings → Accounts, not here: removing an
                        // account from a quick right-click is too
                        // easy to do by accident.
                    }
                }

                // Selectable list / filter row. Skipped for account-
                // header indexes (the header IS that index's only
                // visible content).
                ItemDelegate {
                    id: listRow
                    width: row.width
                    visible: !row._groupCollapsed && !row._isAccountHeader
                    implicitHeight: visible ? 28 : 0
                    height: implicitHeight
                    topPadding: 0
                    bottomPadding: 0
                    // Indent list rows so they read as children of
                    // their account header.
                    leftPadding: row.myGroup.startsWith("account:") ? 24 : 16
                    highlighted: root.vm && root.vm.activeFilterId === row.myId
                    onClicked: if (root.vm) root.vm.selectFilter(row.myId)

                    // Custom contentItem so we can paint the colour
                    // dot before the label. Stock `text:` would
                    // bypass this layout.
                    property int rowColor: {
                        if (!root.vm) { return 0; }
                        const cs = root.vm.sidebarColors;
                        return (cs && row.index < cs.length) ? (cs[row.index] | 0) : 0;
                    }
                    contentItem: RowLayout {
                        spacing: 8
                        Rectangle {
                            // Dot only visible for `caldav:` rows
                            // with a non-zero colour. Built-ins keep
                            // a clean text-only look.
                            visible: row.myId.startsWith("caldav:")
                            Layout.preferredWidth: 10
                            Layout.preferredHeight: 10
                            radius: 5
                            color: {
                                const c = listRow.rowColor;
                                const a = ((c >>> 24) & 0xff) / 255.0;
                                if (a === 0) { return "#9e9e9e"; }
                                const r = ((c >>> 16) & 0xff) / 255.0;
                                const g = ((c >>>  8) & 0xff) / 255.0;
                                const b = ( c         & 0xff) / 255.0;
                                return Qt.rgba(r, g, b, 1.0);
                            }
                            // Faint outline so a light dot stays
                            // visible against a light row bg.
                            border.width: 1
                            border.color: Material.foreground
                            opacity: ((listRow.rowColor >>> 24) & 0xff) === 0 ? 0.45 : 1.0
                        }
                        Label {
                            Layout.fillWidth: true
                            text: row.myLabel
                            elide: Text.ElideRight
                        }
                    }

                    // Right-click opens a per-list menu — currently
                    // just "Choose colour…" (icon support lands when
                    // the icon-font work does). Only meaningful for
                    // `caldav:` rows; built-ins / saved filters
                    // don't surface the menu.
                    TapHandler {
                        enabled: row.myId.startsWith("caldav:")
                        acceptedButtons: Qt.RightButton
                        onTapped: listMenu.popup()
                    }
                    Menu {
                        id: listMenu
                        MenuItem {
                            text: qsTr("Choose colour…")
                            onTriggered: {
                                colorPicker.openFor(
                                    row.myLabel,
                                    row.myId.startsWith("caldav:")
                                        ? row.myId.slice("caldav:".length)
                                        : "");
                            }
                        }
                    }
                }
            }
        }
    }
}
