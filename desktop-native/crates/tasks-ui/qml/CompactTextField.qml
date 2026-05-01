// Drop-in TextField replacement with reduced vertical padding.
//
// Qt 6's Material TextField uses ~14 px top + bottom padding by
// default, which makes every form field feel tall against
// surrounding chrome. The desktop client wants a denser look in
// every form except the toolbar's Search field (where the
// taller default actually reads as deliberate). Use this
// component everywhere else; it inherits TextField verbatim,
// just with tighter padding.
import QtQuick
import QtQuick.Controls

TextField {
    topPadding: 6
    bottomPadding: 6
}
