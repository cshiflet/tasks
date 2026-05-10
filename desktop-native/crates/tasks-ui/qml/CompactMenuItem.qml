// Drop-in MenuItem with reduced vertical padding.
//
// Material's default MenuItem ships with ~14 px top + bottom
// padding, which makes short dropdown menus feel oversized.
// Use this everywhere a MenuItem appears as an explicit child of
// a Menu — the parent Menu's `delegate` property does NOT cover
// declared children, only rows added via the actionsModel /
// addAction APIs, so trimming padding has to happen on the item
// itself.
import QtQuick
import QtQuick.Controls

MenuItem {
    topPadding: 4
    bottomPadding: 4
}
