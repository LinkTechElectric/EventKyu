import EventKyuCore
import Foundation

/// SwiftUI 使用的 cue 行模型，避免 UI 直接依赖 UniFFI Record。
struct CueItem: Identifiable, Hashable {
    let id: String
    let name: String
    let kind: String
    let state: CueState
    let phaseRemainingMs: UInt64?
    let volume: Float
    let opacity: Float
}

struct EventLogItem: Identifiable, Hashable {
    let id = UUID()
    let text: String
}

/// 主线程上的核心桥接层；所有 Rust 调用和错误处理都集中在这里。
@MainActor
final class CueStore: ObservableObject {
    static let shared = CueStore()

    @Published private(set) var cues: [CueItem] = []
    @Published private(set) var eventLog: [EventLogItem] = []
    @Published private(set) var playheadCueID: String?
    @Published private(set) var currentCueName = "待机"
    @Published private(set) var lastError: String?

    let version = eventkyuVersion()
    private let engine = CueEngineHandle()

    private init() {
        refresh()
    }

    func addSample(_ kind: CueKind) {
        perform {
            let name: String
            let source: String
            let duration: UInt64?
            switch kind {
            case .audio:
                name = "示例音频"
                source = "demo://audio"
                duration = nil
            case .video:
                name = "示例视频"
                source = "demo://video"
                duration = 10_000
            case .wait:
                name = "示例等待"
                source = ""
                duration = 3_000
            }
            let id = try engine.addCue(
                kind: kind,
                name: name,
                source: source,
                preWaitMs: 0,
                durationMs: duration,
                postWaitMs: 0,
                autoContinue: AutoContinue.none
            )
            appendLog("已添加 cue #\(id)：\(name)")
        }
    }

    func go() {
        perform {
            let id = try engine.go()
            appendLog("GO → cue #\(id)")
        }
    }

    func start(cueID: String) {
        perform { try engine.start(cueId: cueID, fadeMs: nil) }
    }

    func stop(cueID: String) {
        perform { try engine.stop(cueId: cueID, fadeMs: nil) }
    }

    func pause(cueID: String) {
        perform { try engine.pause(cueId: cueID, fadeMs: nil) }
    }

    func resume(cueID: String) {
        perform { try engine.resume(cueId: cueID, fadeMs: nil) }
    }

    /// 由 50 ms Timer 驱动；一次刷新同时推进时间、读取状态并排空事件。
    func refresh() {
        perform {
            try engine.tickNow()
            let snapshot = try engine.status()
            cues = snapshot.cues.map {
                CueItem(
                    id: $0.cueId,
                    name: $0.name,
                    kind: $0.kind,
                    state: $0.state,
                    phaseRemainingMs: $0.phaseRemainingMs,
                    volume: $0.volume,
                    opacity: $0.opacity
                )
            }
            playheadCueID = snapshot.playheadCueId

            let events = try engine.drainEvents()
            apply(events: events)
            if let running = cues.first(where: { $0.state == .running }) {
                currentCueName = running.name
            } else if !events.isEmpty || currentCueName != "待机" {
                currentCueName = "待机"
            }
        }
    }

    private func apply(events: [EngineEvent]) {
        for event in events {
            if event.kind == .cueStateChanged, event.state == .running,
               let cue = cues.first(where: { $0.id == event.cueId }) {
                currentCueName = cue.name
            }
            eventLog.append(EventLogItem(text: describe(event)))
        }
        if eventLog.count > 200 {
            eventLog.removeFirst(eventLog.count - 200)
        }
    }

    private func perform(_ operation: () throws -> Void) {
        do {
            try operation()
            lastError = nil
        } catch {
            lastError = error.localizedDescription
            appendLog("错误：\(error.localizedDescription)")
        }
    }

    private func appendLog(_ message: String) {
        eventLog.append(EventLogItem(text: message))
    }

    private func describe(_ event: EngineEvent) -> String {
        let id = event.cueId ?? "-"
        let detail: String
        switch event.kind {
        case .cueStateChanged:
            detail = "状态 → \(stateName(event.state))"
        case .cueCompleted:
            detail = "已完成"
        case .fadeUpdated:
            detail = String(
                format: "淡变 volume %.2f / opacity %.2f",
                event.volume ?? 0,
                event.opacity ?? 0
            )
        case .mediaActionRequested:
            detail = "媒体操作 → \(mediaActionName(event.mediaAction))"
        case .error:
            detail = "错误：\(event.message ?? "未知错误")"
        }
        return "[\(event.atMs) ms] cue #\(id) \(detail)"
    }
}

func stateName(_ state: CueState?) -> String {
    switch state {
    case .stopped: return "已停止"
    case .preWait: return "预等待"
    case .running: return "运行中"
    case .postWait: return "后等待"
    case .paused: return "已暂停"
    case .completed: return "已完成"
    case .error: return "错误"
    case nil: return "未知"
    }
}

private func mediaActionName(_ action: MediaAction?) -> String {
    switch action {
    case .play: return "播放"
    case .pause: return "暂停"
    case .resume: return "恢复"
    case .stop: return "停止"
    case nil: return "未知"
    }
}
