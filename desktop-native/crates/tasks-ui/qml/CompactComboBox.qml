// Drop-in ComboBox replacement with a theme-aware background and a
// tight contentItem that gives the displayText the full vertical
// room of the control instead of nesting it inside Material's
// internal TextField — that nested editor reserves a hidden
// underline + line-padding block that shaved descenders off
// `g`/`p`/`y` even at sane outer padding values.
//
// Same transparent-fill + themed-border trick CompactTextField.qml
// uses so dark-theme windows don't flash a Light-theme grey slab.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

ComboBox {
    id: control
    // Trim the vertical chrome so the visible whitespace inside the
    // box is just font leading, not Material's stock padding.
    topPadding: 4
    bottomPadding: 4
    leftPadding: 10
    rightPadding: control.indicator ? control.indicator.width + 4 : 24

    contentItem: Text {
        text: control.displayText
        font: control.font
        color: control.Material.foreground
        verticalAlignment: Text.AlignVCenter
        horizontalAlignment: Text.AlignLeft
        elide: Text.ElideRight
    }

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
