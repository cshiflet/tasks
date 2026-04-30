// Small priority indicator next to task titles.
// Encodes `Task.importance` (0 HIGH .. 3 NONE) via BOTH colour
// AND shape so colour-blind users can distinguish High from
// Medium reliably (H-8). Mappings:
//   HIGH (0)   — solid red filled circle
//   MEDIUM (1) — solid orange filled square (rotated 45°, diamond)
//   LOW (2)    — hollow blue ring
//   NONE (3)   — small grey dash (de-emphasized)
import QtQuick

Item {
    id: root
    property int priority: 3

    implicitWidth: 10
    implicitHeight: 10

    // High — solid red filled circle (most prominent).
    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: "#d32f2f"
        visible: root.priority === 0
    }

    // Medium — solid orange filled diamond (rotated square).
    Rectangle {
        anchors.centerIn: parent
        width: parent.width * 0.85
        height: parent.height * 0.85
        color: "#f57c00"
        rotation: 45
        visible: root.priority === 1
    }

    // Low — hollow blue ring.
    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: "transparent"
        border.color: "#1976d2"
        border.width: 2
        visible: root.priority === 2
    }

    // None — short horizontal grey dash, de-emphasized.
    Rectangle {
        anchors.centerIn: parent
        width: parent.width * 0.65
        height: 2
        color: "#9e9e9e"
        visible: root.priority !== 0 && root.priority !== 1 && root.priority !== 2
    }

    // Accessibility: screen readers announce the priority level.
    Accessible.role: Accessible.Indicator
    Accessible.name: switch (root.priority) {
        case 0: return qsTr("High priority");
        case 1: return qsTr("Medium priority");
        case 2: return qsTr("Low priority");
        default: return qsTr("No priority");
    }
}
