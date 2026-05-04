// Drop-in ComboBox replacement with theme-aware background.
//
// Same transparent-fill + themed-border trick CompactTextField.qml
// uses so dark-theme windows don't flash a Light-theme grey slab.
//
// Earlier revisions tried `topPadding/bottomPadding: 6` for
// density, but Material's contentItem positioning then shaved the
// descenders off `g` / `p` / `y` in dark mode on Linux. Bump both
// paddings to 8 so a full lineHeight + descender always fits, and
// pin a minimum implicitHeight so the borderless background can't
// collapse the box below readable size.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

ComboBox {
    id: control
    topPadding: 8
    bottomPadding: 8
    implicitHeight: Math.max(36, contentItem ? contentItem.implicitHeight + topPadding + bottomPadding : 36)

    background: Rectangle {
        color: "transparent"
        radius: 2
        border.width: control.activeFocus ? 2 : 1
        border.color: control.activeFocus
            ? control.Material.accentColor
            : control.Material.foreground
        opacity: control.activeFocus ? 1.0 : 0.45
    }
}
