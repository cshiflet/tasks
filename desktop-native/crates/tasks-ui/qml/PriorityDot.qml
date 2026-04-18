// Small colored dot that encodes `Task.importance` (0 HIGH .. 3 NONE).
// Used by both the list row and the detail header; colors mirror the
// Android priority palette (red / orange / blue / grey).
import QtQuick

Rectangle {
    property int priority: 3

    implicitWidth: 10
    implicitHeight: 10
    radius: width / 2

    color: switch (priority) {
        case 0: return "#d32f2f" // HIGH
        case 1: return "#f57c00" // MEDIUM
        case 2: return "#1976d2" // LOW
        default: return "#9e9e9e" // NONE
    }
}
