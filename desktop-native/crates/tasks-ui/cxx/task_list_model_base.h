// SPDX-License-Identifier: GPL-3.0-or-later
//
// **WIP scaffolding** — committed ahead of the bridge.rs subclassing
// work that will reference it. Until then this header is built and
// MOC'd by `build.rs` but no Rust code instantiates `TaskListModelBase`.
// Safe to revert if the QAbstractListModel promotion is abandoned;
// presence does not change runtime behaviour.
//
// Thin C++ base class that provides `roleNames()` for the Rust-side
// `TaskListViewModel`. cxx-qt 0.7 supports `#[base = X]` on a qobject,
// and we use that to make `TaskListViewModel` subclass this class —
// which in turn subclasses `QAbstractListModel`.
//
// Why this split: cxx-qt-lib doesn't ship a `QHash<int, QByteArray>`
// binding, which is what `QAbstractListModel::roleNames()` must
// return. Rather than add a custom QHash bridge, we keep the role
// map in C++ (read at construct time, stable for the model's lifetime)
// and let the Rust side implement the pure-virtual `rowCount()` and
// `data()` via cxx-qt's `#[cxx_override]` mechanism.

#pragma once

#include <QAbstractListModel>
#include <QByteArray>
#include <QHash>

class TaskListModelBase : public QAbstractListModel {
    Q_OBJECT

public:
    // Role IDs the Rust-side `data(index, role)` dispatches on.
    // Declared as a plain enum (not Q_ENUM) because QML will get the
    // role NAMES from `roleNames()` below; the integers are an
    // implementation detail between this header and bridge.rs.
    enum TaskRole : int {
        IdRole = Qt::UserRole + 1,
        TitleRole,
        NotesRole,
        PriorityRole,
        IndentRole,
        CompletedRole,
        DueLabelRole,
        ParentIdRole,
    };

    explicit TaskListModelBase(QObject* parent = nullptr)
        : QAbstractListModel(parent) {}

    QHash<int, QByteArray> roleNames() const override {
        return {
            { IdRole,        "taskId" },
            { TitleRole,     "title" },
            { NotesRole,     "notes" },
            { PriorityRole,  "priority" },
            { IndentRole,    "indent" },
            { CompletedRole, "completed" },
            { DueLabelRole,  "dueLabel" },
            { ParentIdRole,  "parentId" },
        };
    }
};
