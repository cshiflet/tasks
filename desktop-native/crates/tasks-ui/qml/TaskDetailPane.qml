// Right-hand detail view for the currently-selected task.
// All fields come from the view model's `selected*` Q_PROPERTYs, which
// update when `selectTask(id)` is invoked from the list pane.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

import com.tasks.desktop

Pane {
    id: root
    padding: 16
    // Belt-and-braces: each pane explicitly follows the OS theme.
    // The ApplicationWindow at the root sets the same value, but
    // Pane's own background-resolver sometimes falls back to its
    // (light) default if the attached-property chain is broken by
    // an intervening QObject — pin it here so this pane reliably
    // matches the rest of the window.
    Material.theme: Material.System
    Material.accent: Material.Blue
    required property QtObject vm

    // Wired to Main.qml's `Edit selected task…` action (F2).
    // Opens the same dialog the toolbar Edit… button does.
    function openEditForSelected() {
        if (!root.vm || root.vm.selectedId === 0) { return; }
        editButton.clicked();
    }
    // Wired to Main.qml's `Delete selected task` action (Delete).
    // Opens the two-step confirm; the user still has to click OK.
    function requestDelete() {
        if (!root.vm || root.vm.selectedId === 0) { return; }
        confirmDelete.open();
    }

    // Look up a label from a parallel-array (uids, labels) Q_PROPERTY pair.
    // Returns the empty string when not found, so the rendered Label can
    // bind directly to it.
    function _labelFor(uid, uids, labels) {
        if (!uid || !uids || !labels) { return ""; }
        for (let i = 0; i < uids.length; i++) {
            if (uids[i] === uid) { return labels[i] || ""; }
        }
        return "";
    }
    function _selectedListName() {
        if (!root.vm) { return ""; }
        return _labelFor(
            root.vm.selectedCaldavCalendarUuid,
            root.vm.caldavCalendarUuids,
            root.vm.caldavCalendarLabels);
    }
    function _selectedPlaceName() {
        if (!root.vm) { return ""; }
        return _labelFor(
            root.vm.selectedPlaceUid,
            root.vm.placeUids,
            root.vm.placeLabels);
    }
    function _selectedParentTitle() {
        if (!root.vm || root.vm.selectedParentId === 0) { return ""; }
        const ids = root.vm.parentCandidateIds;
        const labels = root.vm.parentCandidateLabels;
        for (let i = 0; i < ids.length; i++) {
            if (ids[i] === root.vm.selectedParentId) {
                return labels[i] || qsTr("(untitled task)");
            }
        }
        return "";
    }
    // Pick a readable text colour for a chip background. See
    // TaskListPane.qml::chipTextFor — copy of the same Rec. 601
    // luminance threshold so pale list colours stay legible.
    function chipTextFor(argb) {
        const c = argb | 0;
        const r = (c >>> 16) & 0xff;
        const g = (c >>>  8) & 0xff;
        const b =  c         & 0xff;
        const lum = (r * 0.299) + (g * 0.587) + (b * 0.114);
        return lum > 150 ? "black" : "white";
    }

    function _selectedTagNames() {
        if (!root.vm) { return []; }
        const out = [];
        const uids = root.vm.tagUids;
        const labels = root.vm.tagLabels;
        const colors = root.vm.tagColors;
        for (let i = 0; i < root.vm.selectedTagUids.length; i++) {
            const u = root.vm.selectedTagUids[i];
            for (let j = 0; j < uids.length; j++) {
                if (uids[j] === u) {
                    out.push({
                        name: labels[j] || u,
                        color: (colors && j < colors.length) ? (colors[j] | 0) : 0,
                    });
                    break;
                }
            }
        }
        return out;
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 12
        visible: root.vm && root.vm.selectedId > 0

        Label {
            Layout.fillWidth: true
            text: root.vm ? root.vm.selectedTitle : ""
            font.pointSize: Qt.application.font.pointSize + 4
            font.bold: true
            wrapMode: Text.WordWrap
            font.strikeout: root.vm && root.vm.selectedCompleted
        }

        // Metadata strip: priority, list, parent, location, reminder
        // count. Each chip-like Label only renders if there's a value
        // worth showing, so an unannotated task collapses cleanly to
        // nothing.
        Flow {
            Layout.fillWidth: true
            spacing: 8

            // Priority pill, coloured to match the priority. Hidden
            // when there's no priority (NONE = 3) so the pill clutter
            // tracks the task's actual data.
            Label {
                visible: root.vm && root.vm.selectedPriority < 3
                text: {
                    if (!root.vm) { return ""; }
                    switch (root.vm.selectedPriority) {
                        case 0: return qsTr("High");
                        case 1: return qsTr("Medium");
                        case 2: return qsTr("Low");
                        default: return "";
                    }
                }
                opacity: 0.95
                // White text is fine on red / blue but borderline
                // on the medium-orange + grey palette swatches.
                // Run them through the same luminance helper so
                // the pill stays readable on every value.
                property color bgColor: {
                    if (!root.vm) { return "#9e9e9e"; }
                    switch (root.vm.selectedPriority) {
                        case 0: return "#d32f2f";  // red
                        case 1: return "#f57c00";  // orange
                        case 2: return "#1976d2";  // blue
                        default: return "#9e9e9e"; // grey
                    }
                }
                color: {
                    const c = bgColor;
                    const lum = (c.r * 0.299 + c.g * 0.587 + c.b * 0.114) * 255;
                    return lum > 150 ? "black" : "white";
                }
                font.pointSize: Qt.application.font.pointSize - 1
                font.bold: true
                leftPadding: 8
                rightPadding: 8
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    radius: 4
                    color: parent.bgColor
                }
            }

            // CalDAV list pill — coloured background matching the
            // list's `cdl_color`. Text colour follows the bg's
            // perceived luminance so pale list palettes stay
            // readable. No leading icon glyph (Windows lacked a
            // font fallback for the previous 📋 codepoint).
            Label {
                visible: root._selectedListName().length > 0
                text: root._selectedListName()
                color: root.vm
                    ? root.chipTextFor(root.vm.selectedCaldavCalendarColor | 0)
                    : "white"
                font.pointSize: Qt.application.font.pointSize - 1
                font.bold: true
                leftPadding: 8
                rightPadding: 8
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    radius: 4
                    color: {
                        if (!root.vm) { return "#9e9e9e"; }
                        const c = root.vm.selectedCaldavCalendarColor | 0;
                        const a = ((c >>> 24) & 0xff) / 255.0;
                        if (a === 0) { return "#9e9e9e"; }
                        const r = ((c >>> 16) & 0xff) / 255.0;
                        const g = ((c >>>  8) & 0xff) / 255.0;
                        const b = ( c         & 0xff) / 255.0;
                        return Qt.rgba(r, g, b, 1.0);
                    }
                }
            }
            // Parent task pill — neutral chip prefixed with a plain
            // ASCII arrow rather than a Unicode arrow that some
            // fonts on Windows fall back to a tofu glyph for.
            Label {
                visible: root._selectedParentTitle().length > 0
                text: qsTr("Parent: %1").arg(root._selectedParentTitle())
                opacity: 0.75
                font.pointSize: Qt.application.font.pointSize - 1
                leftPadding: 6
                rightPadding: 6
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    color: Material.foreground
                    opacity: 0.08
                    radius: 4
                }
            }
            // Location pill (with arrival/departure trigger hint).
            Label {
                visible: root._selectedPlaceName().length > 0
                text: {
                    let trig = "";
                    if (root.vm && root.vm.selectedPlaceArrival && root.vm.selectedPlaceDeparture) {
                        trig = qsTr(" (arrival + departure)");
                    } else if (root.vm && root.vm.selectedPlaceArrival) {
                        trig = qsTr(" (arrival)");
                    } else if (root.vm && root.vm.selectedPlaceDeparture) {
                        trig = qsTr(" (departure)");
                    }
                    return qsTr("Place: %1%2").arg(root._selectedPlaceName()).arg(trig);
                }
                opacity: 0.75
                font.pointSize: Qt.application.font.pointSize - 1
                leftPadding: 6
                rightPadding: 6
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    color: Material.foreground
                    opacity: 0.08
                    radius: 4
                }
            }
            // Reminder count pill. ToolTip is bound through a
            // dedicated HoverHandler since Label has no `hovered`
            // property of its own.
            Label {
                id: reminderPill
                visible: root.vm && root.vm.selectedAlarmLabels.length > 0
                text: qsTr("Reminders: %1")
                          .arg(root.vm ? root.vm.selectedAlarmLabels.length : 0)
                opacity: 0.75
                font.pointSize: Qt.application.font.pointSize - 1
                leftPadding: 6
                rightPadding: 6
                topPadding: 2
                bottomPadding: 2
                background: Rectangle {
                    color: Material.foreground
                    opacity: 0.08
                    radius: 4
                }
                HoverHandler { id: reminderHover }
                ToolTip.visible: reminderHover.hovered
                                 && root.vm
                                 && root.vm.selectedAlarmLabels.length > 0
                ToolTip.text: root.vm ? root.vm.selectedAlarmLabels.join("\n") : ""
            }
        }

        // Tags row — wraps to multiple lines when there are many.
        Flow {
            Layout.fillWidth: true
            spacing: 6
            visible: root._selectedTagNames().length > 0

            Repeater {
                model: root._selectedTagNames()
                Label {
                    required property var modelData
                    text: modelData.name
                    font.pointSize: Qt.application.font.pointSize - 1
                    leftPadding: 6
                    rightPadding: 6
                    topPadding: 2
                    bottomPadding: 2
                    color: {
                        const c = modelData.color | 0;
                        const a = (c >>> 24) & 0xff;
                        if (a === 0) {
                            // Untinted tag — keep theme foreground.
                            return Material.foreground;
                        }
                        return root.chipTextFor(c);
                    }
                    background: Rectangle {
                        radius: 8
                        color: {
                            const c = modelData.color | 0;
                            const a = ((c >>> 24) & 0xff) / 255.0;
                            if (a === 0) {
                                // No assigned colour — keep the prior
                                // faint Blue badge so the chip is still
                                // visible against the pane background.
                                return Material.color(Material.Blue, Material.Shade400);
                            }
                            const r = ((c >>> 16) & 0xff) / 255.0;
                            const g = ((c >>>  8) & 0xff) / 255.0;
                            const b = ( c         & 0xff) / 255.0;
                            return Qt.rgba(r, g, b, 1.0);
                        }
                        opacity: {
                            const c = modelData.color | 0;
                            return ((c >>> 24) & 0xff) === 0 ? 0.18 : 1.0;
                        }
                    }
                }
            }
        }

        RowLayout {
            spacing: 8
            visible: root.vm && root.vm.selectedDueLabel.length > 0
            Label {
                text: qsTr("Due:")
                opacity: 0.6
            }
            Label {
                text: root.vm ? root.vm.selectedDueLabel : ""
            }
        }

        // Recurrence summary. The view model hands us a humanised
        // phrase ("Every other week on Mon, Wed") produced by
        // `tasks_core::recurrence::humanize_rrule`, already tagged
        // with "(from completion)" when `tasks.repeat_from` says so.
        // Rules we can't parse show through verbatim.
        RowLayout {
            spacing: 8
            visible: root.vm && root.vm.selectedRecurrence.length > 0
            Label {
                text: qsTr("Repeats:")
                opacity: 0.6
            }
            Label {
                Layout.fillWidth: true
                text: root.vm ? root.vm.selectedRecurrence : ""
                elide: Text.ElideRight
            }
        }

        Label {
            Layout.fillWidth: true
            Layout.fillHeight: true
            text: root.vm ? root.vm.selectedNotes : ""
            wrapMode: Text.Wrap
            textFormat: Text.MarkdownText
            visible: root.vm && root.vm.selectedNotes.length > 0
        }

        Item { Layout.fillHeight: true }

        // Bottom action row. Delete is a soft-delete — the row is
        // flagged `deleted = now_ms` and disappears from the active
        // list, but the record stays so a future "Trash" view or an
        // undo path can surface it again.
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            Item { Layout.fillWidth: true }
            Button {
                id: editButton
                text: qsTr("Edit…")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Change title, notes, due/hide dates, or priority")
                onClicked: {
                    if (!root.vm) {
                        return;
                    }
                    editDialog.initialTitle = root.vm.selectedTitle;
                    editDialog.initialNotes = root.vm.selectedNotes;
                    editDialog.initialDueText = root.vm.selectedDueLabel;
                    editDialog.initialHideUntilText =
                        root.vm.selectedHideUntilLabel;
                    editDialog.initialPriority = root.vm.selectedPriority;
                    editDialog.initialRecurrenceSummary =
                        root.vm.selectedRecurrence;
                    editDialog.initialCaldavUuid =
                        root.vm.selectedCaldavCalendarUuid;
                    editDialog.initialPlaceUid =
                        root.vm.selectedPlaceUid;
                    editDialog.initialPlaceArrival =
                        root.vm.selectedPlaceArrival;
                    editDialog.initialPlaceDeparture =
                        root.vm.selectedPlaceDeparture;
                    editDialog.initialParentId =
                        root.vm.selectedParentId;
                    editDialog.initialEstimatedText =
                        root.vm.selectedEstimatedText;
                    editDialog.initialElapsedText =
                        root.vm.selectedElapsedText;
                    editDialog.initialRecurrenceRaw =
                        root.vm.selectedRecurrenceRaw;
                    editDialog.initialRepeatFrom =
                        root.vm.selectedRepeatFrom;
                    editDialog.loadFromSelection();
                    editDialog.open();
                }
            }
            Button {
                text: qsTr("Delete")
                ToolTip.visible: hovered
                ToolTip.text: qsTr("Mark this task deleted (hides it from the active list)")
                onClicked: confirmDelete.open()
            }
        }
    }

    // Slide-out edit pane. Anchored to fill the detail pane;
    // animates `x` from `parent.width` (off-screen right) to 0
    // when `editDialog.open()` is called from the Edit button.
    // It sits on top of the rest of the pane via a higher `z`,
    // so the underlying chrome is hidden while editing.
    TaskEditDialog {
        id: editDialog
        vm: root.vm
    }

    // Two-step confirm for delete. Cheap insurance until an undo
    // stack lands.
    //
    // `implicitWidth` is pinned explicitly so the wrap-mode Label
    // inside doesn't circularly drive the Dialog's own implicit
    // width. Without that pin, Qt logs "Binding loop detected for
    // property 'implicitWidth'" every time the dialog opens.
    Dialog {
        id: confirmDelete
        anchors.centerIn: parent
        modal: true
        title: qsTr("Delete task?")
        standardButtons: Dialog.Cancel | Dialog.Ok
        implicitWidth: 360

        Label {
            width: parent.width
            text: qsTr("Remove “%1” from the active list?")
                  .arg(root.vm ? root.vm.selectedTitle : "")
            wrapMode: Text.Wrap
        }
        onAccepted: if (root.vm) root.vm.deleteSelectedTask()
    }

    // Empty-state column. Slightly larger title with a one-line
    // hint so an unselected pane reads as deliberately blank, not
    // broken.
    ColumnLayout {
        anchors.centerIn: parent
        spacing: 4
        visible: !root.vm || root.vm.selectedId === 0

        Label {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("No task selected")
            font.pointSize: Qt.application.font.pointSize + 2
            font.bold: true
            opacity: 0.55
        }
        Label {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Pick a row from the list to see its details here.")
            opacity: 0.45
            font.pointSize: Qt.application.font.pointSize - 1
        }
    }
}
