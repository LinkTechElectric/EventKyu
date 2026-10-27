import AppKit
import Combine
import EventKyuCore
import SwiftUI

@main
struct EventKyuApp: App {
    @NSApplicationDelegateAdaptor(EventKyuAppDelegate.self) private var appDelegate
    @StateObject private var store = CueStore.shared

    private let ticker = Timer.publish(every: 0.05, on: .main, in: .common).autoconnect()

    var body: some Scene {
        WindowGroup("EventKyu 控制台") {
            ControlView(store: store)
                .frame(minWidth: 880, minHeight: 620)
                .onReceive(ticker) { _ in store.refresh() }
        }
        .commands {
            CommandGroup(after: .newItem) {
                Button("GO") { store.go() }
                    .keyboardShortcut(.return, modifiers: [])
            }
        }
    }
}

@MainActor
final class EventKyuAppDelegate: NSObject, NSApplicationDelegate {
    private var stageController: StageOutputWindowController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        let controller = StageOutputWindowController(store: .shared)
        stageController = controller
        controller.showWindowed()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}

private struct ControlView: View {
    @ObservedObject var store: CueStore
    @State private var selectedCueID: String?

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            HSplitView {
                cueList
                    .frame(minWidth: 470)
                eventList
                    .frame(minWidth: 300)
            }
            Divider()
            transport
        }
        .background(Color(nsColor: .windowBackgroundColor))
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("EventKyu")
                        .font(.largeTitle.bold())
                    Text("cue-core \(store.version) · UniFFI 已连接")
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button(action: store.go) {
                    Text("GO")
                        .font(.title2.bold())
                        .frame(width: 116, height: 52)
                }
                .buttonStyle(.borderedProminent)
                .tint(.green)
                .controlSize(.large)
                .keyboardShortcut(.return, modifiers: [])
            }

            HStack(spacing: 10) {
                Button("＋ Audio") { store.addSample(.audio) }
                Button("＋ Video") { store.addSample(.video) }
                Button("＋ Wait") { store.addSample(.wait) }
                Spacer()
                Text("播放头：\(store.playheadCueID.map { "#\($0)" } ?? "列表末尾")")
                    .foregroundStyle(.secondary)
            }
        }
        .padding(20)
    }

    private var cueList: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Cue 列表")
                .font(.headline)
                .padding(.horizontal, 16)
                .padding(.top, 14)
            List(store.cues, selection: $selectedCueID) { cue in
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(cue.name)
                            .font(.headline)
                        Text("#\(cue.id) · \(cue.kind)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    if let remaining = cue.phaseRemainingMs {
                        Text("\(remaining) ms")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                    Text(stateName(cue.state))
                        .font(.callout.weight(.medium))
                        .foregroundStyle(color(for: cue.state))
                }
                .padding(.vertical, 5)
                .tag(cue.id)
            }
            .listStyle(.inset)
        }
    }

    private var eventList: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("事件日志")
                .font(.headline)
                .padding(.horizontal, 16)
                .padding(.top, 14)
            List(store.eventLog) { event in
                Text(event.text)
                    .font(.caption.monospaced())
                    .textSelection(.enabled)
                    .padding(.vertical, 2)
            }
            .listStyle(.inset)
        }
    }

    private var transport: some View {
        HStack(spacing: 12) {
            Button("开始") {
                if let selectedCueID { store.start(cueID: selectedCueID) }
            }
            Button("暂停") {
                if let selectedCueID { store.pause(cueID: selectedCueID) }
            }
            Button("恢复") {
                if let selectedCueID { store.resume(cueID: selectedCueID) }
            }
            Button("停止") {
                if let selectedCueID { store.stop(cueID: selectedCueID) }
            }
            .tint(.red)
            Spacer()
            if let error = store.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .lineLimit(1)
                    .help(error)
            } else {
                Text("50 ms tick · 媒体渲染尚未接入")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(14)
    }

    private func color(for state: CueState) -> Color {
        switch state {
        case .running: return .green
        case .paused, .preWait, .postWait: return .orange
        case .completed: return .secondary
        case .error: return .red
        case .stopped: return .primary
        }
    }
}
