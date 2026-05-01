// Drop-in ComboBox replacement with theme-aware background.
//
// Same transparent-fill + themed-border trick CompactTextField.qml
// uses so dark-theme windows don't flash a Light-theme grey slab.
// See CompactTextField.qml for the Qt 6.4 vs 6.5+ rationale on
// dropping `Material.containerStyle`.
//
// Earlier revisions also forced topPadding / bottomPadding to 6 to
// match the text fields, but Material's ComboBox positions its
// content + dropdown indicator from those paddings and clipped the
// label baseline at the bottom on Windows. Material's default
// padding is fine; only the background needed replacing.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

ComboBox {
    id: control
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
