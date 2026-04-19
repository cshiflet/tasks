use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    // The custom `TaskListModelBase` header subclasses QAbstractListModel
    // with the role-name hash baked in. cxx-qt's `#[base = ...]`
    // mechanism uses this type so the Rust side only has to provide
    // `rowCount()` + `data()` + `load_tasks()` and get the rest from the
    // Qt framework.
    CxxQtBuilder::new()
        .qt_module("Gui")
        .qt_module("Qml")
        .qt_module("Quick")
        .qobject_header("cxx/task_list_model_base.h")
        .cc_builder(|cc| {
            cc.include("cxx");
        })
        .qml_module(QmlModule {
            uri: "com.tasks.desktop",
            rust_files: &["src/bridge.rs"],
            qml_files: &[
                "qml/Main.qml",
                "qml/SidebarPane.qml",
                "qml/TaskListPane.qml",
                "qml/TaskDetailPane.qml",
                "qml/PriorityDot.qml",
            ],
            ..Default::default()
        })
        .build();
}
