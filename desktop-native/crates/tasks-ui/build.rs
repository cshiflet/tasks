use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new()
        .qt_module("Gui")
        .qt_module("Qml")
        .qt_module("Quick")
        .qml_module(QmlModule {
            uri: "com.tasks.desktop",
            rust_files: &["src/bridge.rs"],
            qml_files: &["qml/Main.qml"],
            ..Default::default()
        })
        .build();
}
