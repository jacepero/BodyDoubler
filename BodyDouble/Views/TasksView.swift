// Task list + open threads (plan §3). Mostly model-maintained, but manual
// controls keep Phase 1 usable before inference exists: add a task, tap to
// make one current, swipe to change status, close threads.

import SwiftUI

struct TasksView: View {
    @EnvironmentObject private var coordinator: SessionCoordinator

    @State private var tasks: [TaskItem] = []
    @State private var threads: [ThreadItem] = []
    @State private var newTaskTitle = ""
    @State private var showingAdd = false

    var body: some View {
        NavigationStack {
            List {
                currentSection
                section("Active", tasks.filter { $0.isCurrent == 0 && $0.taskStatus == .active })
                section("Paused", tasks.filter { $0.taskStatus == .paused })
                threadsSection
                section("Done / Dropped", tasks.filter {
                    $0.taskStatus == .done || $0.taskStatus == .dropped
                })
            }
            .navigationTitle("Tasks")
            .toolbar {
                Button {
                    showingAdd = true
                } label: {
                    Label("Add task", systemImage: "plus")
                }
            }
            .alert("New task", isPresented: $showingAdd) {
                TextField("Title", text: $newTaskTitle)
                Button("Start") {
                    let title = newTaskTitle.trimmingCharacters(in: .whitespacesAndNewlines)
                    newTaskTitle = ""
                    guard !title.isEmpty else { return }
                    try? coordinator.store.setCurrentTask(title: title)
                    reload()
                }
                Button("Cancel", role: .cancel) { newTaskTitle = "" }
            }
            .onAppear(perform: reload)
            .onChange(of: coordinator.turns) { _, _ in reload() }
        }
    }

    // MARK: - Sections

    @ViewBuilder
    private var currentSection: some View {
        if let current = tasks.first(where: { $0.isCurrent == 1 }) {
            Section("Current") {
                TaskRow(task: current, isCurrent: true, onAction: handle)
            }
        }
    }

    @ViewBuilder
    private func section(_ title: String, _ items: [TaskItem]) -> some View {
        if !items.isEmpty {
            Section(title) {
                ForEach(items) { task in
                    TaskRow(task: task, isCurrent: false, onAction: handle)
                }
            }
        }
    }

    @ViewBuilder
    private var threadsSection: some View {
        if !threads.isEmpty {
            Section("Open threads") {
                ForEach(threads) { thread in
                    HStack {
                        Image(systemName: "circle.dotted")
                            .foregroundStyle(.orange)
                        Text(thread.note)
                    }
                    .swipeActions {
                        Button("Close") {
                            if let id = thread.id {
                                try? coordinator.store.closeThread(id: id)
                                reload()
                            }
                        }
                        .tint(.green)
                    }
                }
            }
        }
    }

    // MARK: - Actions

    private enum RowAction { case makeCurrent, setStatus(TaskStatus) }

    private func handle(_ action: RowAction, _ task: TaskItem) {
        guard let id = task.id else { return }
        switch action {
        case .makeCurrent:
            try? coordinator.store.setCurrentTask(title: task.title)
        case .setStatus(let status):
            try? coordinator.store.updateTask(id: id, status: status)
        }
        reload()
    }

    private func reload() {
        tasks = (try? coordinator.store.allTasks()) ?? []
        threads = (try? coordinator.store.openThreads()) ?? []
    }

    // MARK: - Row

    private struct TaskRow: View {
        let task: TaskItem
        let isCurrent: Bool
        let onAction: (RowAction, TaskItem) -> Void

        var body: some View {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(task.title)
                        .fontWeight(isCurrent ? .semibold : .regular)
                    if isCurrent, let since = task.updatedDate {
                        Text("current for \(SessionCoordinator.elapsedString(since: since))")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
                Spacer()
                if isCurrent {
                    Image(systemName: "scope").foregroundStyle(.blue)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture {
                if !isCurrent, task.taskStatus == .active || task.taskStatus == .paused {
                    onAction(.makeCurrent, task)
                }
            }
            .swipeActions(edge: .trailing) {
                if task.taskStatus != .done {
                    Button("Done") { onAction(.setStatus(.done), task) }.tint(.green)
                }
                if task.taskStatus == .active {
                    Button("Pause") { onAction(.setStatus(.paused), task) }.tint(.orange)
                }
                if task.taskStatus == .paused {
                    Button("Resume") { onAction(.setStatus(.active), task) }.tint(.blue)
                }
            }
            .swipeActions(edge: .leading) {
                if task.taskStatus != .dropped && task.taskStatus != .done {
                    Button("Drop") { onAction(.setStatus(.dropped), task) }.tint(.red)
                }
            }
        }
    }
}
