// Top-level Settings window with a TabBar across List + Accounts.
//
// Uses ApplicationWindow (a real top-level window) rather than
// QtQuick.Controls' Dialog so the user gets native title-bar move,
// window-manager close button, and resize handles for free. The
// previous "Preferences…" dialog moved into the `List` tab here;
// the new `Accounts` tab surfaces the session-local sync account
// configuration backed by the bridge's accounts list.
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ApplicationWindow {
    id: settingsWindow
    // Size picked to fit both panes without scrolling on a typical
    // desktop; the user can resize, since this is a real window.
    width: 620
    height: 520
    minimumWidth: 420
    minimumHeight: 360
    title: qsTr("Settings")
    // Hide on close rather than destroy so re-open preserves the
    // selected tab + any in-flight Accounts-pane form state.
    // Callers reopen by setting `visible = true`.
    visible: false
    flags: Qt.Dialog | Qt.WindowTitleHint | Qt.WindowSystemMenuHint
           | Qt.WindowCloseButtonHint | Qt.WindowMinMaxButtonsHint
    // Mirror the main window's theme so Settings doesn't snap back
    // to light on a dark desktop.
    Material.theme: Material.System
    Material.accent: Material.Blue

    required property QtObject vm

    // Called by Main.qml before show() so the List tab's current
    // controls reflect the live preferences.
    function loadFromVm() {
        listPane.loadFromVm();
    }

    header: TabBar {
        id: tabs
        // "View" reads more clearly than "List" — the prefs in this
        // tab control how the task list is sorted and which rows
        // appear, which is a view concern. The pane file kept the
        // ListSettingsPane name to avoid churn in the bridge.
        TabButton { text: qsTr("View") }
        TabButton { text: qsTr("Accounts") }
    }

    // Pane wrapper anchors the Material attached context so the
    // tab body has a themed background; without it the StackLayout
    // sits on whatever Qt happens to default to (white on Windows
    // with Mica disabled, black with Mica enabled), which doesn't
    // always match the window-level theme that the panes expect.
    Pane {
        anchors.fill: parent
        Material.theme: Material.System
        Material.accent: Material.Blue
        padding: 16

        StackLayout {
            anchors.fill: parent
            currentIndex: tabs.currentIndex

            ListSettingsPane {
                id: listPane
                vm: settingsWindow.vm
            }

            AccountsPane {
                id: accountsPane
                vm: settingsWindow.vm
            }
        }
    }

    // Drop the heavy footer ToolBar previously holding only a Close
    // button — the window has a native close X already.
}
