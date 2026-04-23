// Calendar popup for date text fields in the task edit dialog.
//
// Sits as a small button next to a TextField; clicking opens a
// month grid with prev/next navigation. Picking a date writes
// `YYYY-MM-DD` into the target field, preserving any trailing
// ` HH:MM` so the user doesn't lose the time portion of a
// dated-with-time value.
//
// The authoritative store is still the TextField — this is a
// convenience overlay. Users who prefer typing continue to type.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

Item {
    id: root

    // The TextField this picker writes into. Must expose `.text`
    // in the `YYYY-MM-DD [HH:MM]` format the Rust `parse_due_input`
    // accepts; empty string means "no date set".
    required property TextField target

    implicitWidth: button.implicitWidth
    implicitHeight: button.implicitHeight

    // Today-by-default; overwritten on every popup-open from the
    // target field so the user sees the currently-chosen month.
    property date displayedMonth: new Date()

    function _pad(n) { return n < 10 ? "0" + n : "" + n; }

    function _seedFromTarget() {
        const txt = (target && target.text) ? target.text.trim() : "";
        const m = txt.match(/^(\d{4})-(\d{2})-(\d{2})/);
        if (m) {
            const y = parseInt(m[1], 10);
            const mo = parseInt(m[2], 10) - 1;
            const d = parseInt(m[3], 10);
            if (y && !isNaN(mo) && d) {
                root.displayedMonth = new Date(y, mo, d);
                return;
            }
        }
        root.displayedMonth = new Date();
    }

    function _writeDate(date) {
        // Preserve any " HH:MM" suffix the user already typed so
        // picking a date doesn't clobber the time part.
        const existing = (target && target.text) ? target.text.trim() : "";
        const timeMatch = existing.match(/\s+(\d{1,2}:\d{2})\s*$/);
        const datePart = date.getFullYear() + "-"
            + root._pad(date.getMonth() + 1) + "-"
            + root._pad(date.getDate());
        target.text = timeMatch ? (datePart + " " + timeMatch[1]) : datePart;
    }

    Button {
        id: button
        anchors.fill: parent
        flat: true
        text: "📅"
        padding: 4
        ToolTip.visible: hovered
        ToolTip.text: qsTr("Pick a date")
        onClicked: {
            root._seedFromTarget();
            popup.open();
        }
    }

    Popup {
        id: popup
        // Anchor below the button; flip above if close to bottom.
        x: button.width - width
        y: button.height + 4
        width: 280
        padding: 8
        modal: false
        focus: true

        contentItem: ColumnLayout {
            spacing: 8

            // Month navigation row.
            RowLayout {
                Layout.fillWidth: true
                spacing: 4

                Button {
                    text: "‹"
                    flat: true
                    onClicked: {
                        const d = new Date(root.displayedMonth);
                        d.setMonth(d.getMonth() - 1);
                        root.displayedMonth = d;
                    }
                }
                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    text: Qt.formatDate(root.displayedMonth, "MMMM yyyy")
                    font.bold: true
                }
                Button {
                    text: "›"
                    flat: true
                    onClicked: {
                        const d = new Date(root.displayedMonth);
                        d.setMonth(d.getMonth() + 1);
                        root.displayedMonth = d;
                    }
                }
            }

            DayOfWeekRow {
                Layout.fillWidth: true
                month: root.displayedMonth.getMonth()
                year: root.displayedMonth.getFullYear()
            }

            MonthGrid {
                id: grid
                Layout.fillWidth: true
                month: root.displayedMonth.getMonth()
                year: root.displayedMonth.getFullYear()
                onClicked: (date) => {
                    root._writeDate(date);
                    popup.close();
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Button {
                    text: qsTr("Clear")
                    flat: true
                    onClicked: {
                        if (root.target) { root.target.text = ""; }
                        popup.close();
                    }
                }
                Item { Layout.fillWidth: true }
                Button {
                    text: qsTr("Today")
                    flat: true
                    onClicked: {
                        root._writeDate(new Date());
                        popup.close();
                    }
                }
            }
        }
    }
}
