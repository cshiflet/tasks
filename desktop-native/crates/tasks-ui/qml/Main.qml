// Placeholder QML shell for the native desktop client.
//
// The cxx-qt bridge that drives this file is not wired up in this commit —
// see desktop-native/README.md. When it lands, Main.qml will split into a
// three-pane layout (SidebarPane.qml / TaskListPane.qml / TaskDetailPane.qml)
// mirroring the Android compact-window navigation.
import QtQuick 6.5
import QtQuick.Controls 6.5

ApplicationWindow {
    id: root
    width: 1100
    height: 720
    visible: true
    title: qsTr("Tasks")

    Label {
        anchors.centerIn: parent
        text: qsTr("tasks-ui — Qt bridge pending")
    }
}
