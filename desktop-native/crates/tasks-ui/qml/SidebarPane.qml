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
                property string myId: root.vm ? root.vm.sidebarIds[row.index] : ""
                property string myLabel: root.vm ? root.vm.sidebarLabels[row.index] : ""
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
                    implicitHeight: visible ? 26 : 0
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
                        MenuSeparator {}
                        MenuItem {
                            text: qsTr("Remove account…")
                            onTriggered: {
                                if (!root.vm) { return; }
                                // Find the account index by uuid;
                                // removeAccount takes an index.
                                const target = root._accountUuidOf(row.myGroup);
                                const uuids = root.vm.accountUuids;
                                for (let i = 0; i < uuids.length; i++) {
                                    if (uuids[i] === target) {
                                        root.vm.removeAccount(i);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                // Selectable list / filter row. Skipped for account-
                // header indexes (the header IS that index's only
                // visible content).
                ItemDelegate {
                    width: row.width
                    visible: !row._groupCollapsed && !row._isAccountHeader
                    implicitHeight: visible ? 28 : 0
                    height: implicitHeight
                    topPadding: 0
                    bottomPadding: 0
                    // Indent list rows so they read as children of
                    // their account header.
                    leftPadding: row.myGroup.startsWith("account:") ? 24 : 16
                    text: row.myLabel
                    highlighted: root.vm && root.vm.activeFilterId === row.myId
                    onClicked: if (root.vm) root.vm.selectFilter(row.myId)
                }
            }
        }
    }
}
