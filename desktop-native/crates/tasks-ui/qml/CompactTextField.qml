// Drop-in TextField replacement with reduced vertical padding +
// theme-aware background.
//
// Qt 6.6's Material style picks `Filled` as the default container
// for TextField. The filled background is a tinted rectangle whose
// colour is computed at construction time, and on Windows we've
// seen it lock to the Light-theme grey even when the rest of the
// window resolves to Dark — so every form field flashes a bright
// slab against the dark chrome. The `Material.containerStyle`
// directive that would side-step it cleanly only landed in Qt 6.5+,
// so we replace `background` outright with a transparent rectangle
// + a Material-foreground border. Renders correctly in both themes
// on every Qt 6 release we target.
//
// Padding override matches the rest of the desktop client's denser
// chrome; the search toolbar uses the taller default deliberately.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material

TextField {
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
