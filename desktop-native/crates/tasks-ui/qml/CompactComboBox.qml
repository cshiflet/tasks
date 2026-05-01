// Drop-in ComboBox replacement with reduced vertical padding and
// theme-aware background.
//
// Mirrors CompactTextField.qml so form rows (text + combo + text)
// line up at the same height, and uses the same transparent-fill
// + themed-border trick so dark-theme windows don't flash a
// Light-theme grey slab. See CompactTextField.qml for the Qt 6.4
// vs 6.5+ rationale on dropping `Material.containerStyle`.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

ComboBox {
    id: control
    topPadding: 6
    bottomPadding: 6
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
