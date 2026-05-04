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
    Material.accent: Material.Blue

    required property QtObject vm
    // Reference to the main ApplicationWindow so the List tab can
    // toggle `appearanceTheme`. The List tab reads / writes it
    // directly; we don't proxy through a property on this window
    // because that would introduce two sources of truth.
    required property var appWindow

    // Called by Main.qml before show() so each tab starts from the
    // bridge's live state rather than whatever stale value the
    // widget held from the prior open.
    function loadFromVm() {
        generalPane.loadFromVm();
        listPane.loadFromVm();
    }

    header: TabBar {
        id: tabs
        // Three tabs:
        //   General      — app-wide preferences (theme, etc.).
        //   List defaults — defaults applied to every list view.
        //                   Per-list overrides will land behind a
        //                   right-click affordance on each sidebar
        //                   entry; not implemented yet.
        //   Accounts     — sync account configuration.
        TabButton { text: qsTr("General") }
        TabButton { text: qsTr("List defaults") }
        TabButton { text: qsTr("Accounts") }
    }

    // Pane wrapper anchors the Material attached context so the
    // tab body has a themed background; without it the StackLayout
    // sits on whatever Qt happens to default to (white on Windows
    // with Mica disabled, black with Mica enabled), which doesn't
    // always match the window-level theme that the panes expect.
    Pane {
        anchors.fill: parent
        Material.accent: Material.Blue
        padding: 16

        StackLayout {
            anchors.fill: parent
            currentIndex: tabs.currentIndex

            GeneralSettingsPane {
                id: generalPane
                vm: settingsWindow.vm
                appWindow: settingsWindow.appWindow
            }

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
