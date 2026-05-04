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
    // Outer padding is slightly more generous than the compact
    // text fields so the closed box visually pairs with the
    // adjacent Label rows in Settings instead of looking shorter.
    topPadding: 6
    bottomPadding: 6
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

    // Drop-down items — each row gets a touch more vertical
    // padding so options aren't crammed against each other when
    // the popup opens. Material's default ItemDelegate height is
    // tied to the same 48 px touch-target floor we override
    // elsewhere; an explicit topPadding/bottomPadding here lifts
    // it without touching the entire app's ItemDelegate style.
    delegate: ItemDelegate {
        width: control.width
        topPadding: 8
        bottomPadding: 8
        leftPadding: 10
        rightPadding: 10
        text: control.textRole
            ? (Array.isArray(control.model)
                ? modelData[control.textRole]
                : model[control.textRole])
            : modelData
        font: control.font
        highlighted: control.highlightedIndex === index
        hoverEnabled: control.hoverEnabled
    }
}
