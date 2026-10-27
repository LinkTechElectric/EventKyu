import AppKit
import Combine

/// 纯 AppKit 舞台视图；后续视频渲染直接挂到此 NSView。
final class StageOutputView: NSView {
    private let titleLabel = NSTextField(labelWithString: "EventKyu Stage Output")
    private let cueLabel = NSTextField(labelWithString: "待机")

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor

        titleLabel.textColor = .white
        titleLabel.font = .systemFont(ofSize: 44, weight: .bold)
        titleLabel.alignment = .center
        cueLabel.textColor = NSColor.white.withAlphaComponent(0.78)
        cueLabel.font = .systemFont(ofSize: 30, weight: .medium)
        cueLabel.alignment = .center

        for label in [titleLabel, cueLabel] {
            label.translatesAutoresizingMaskIntoConstraints = false
            label.lineBreakMode = .byTruncatingTail
            addSubview(label)
        }
        NSLayoutConstraint.activate([
            titleLabel.centerXAnchor.constraint(equalTo: centerXAnchor),
            titleLabel.centerYAnchor.constraint(equalTo: centerYAnchor, constant: -24),
            titleLabel.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 32),
            titleLabel.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -32),
            cueLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 28),
            cueLabel.centerXAnchor.constraint(equalTo: centerXAnchor),
            cueLabel.leadingAnchor.constraint(greaterThanOrEqualTo: leadingAnchor, constant: 32),
            cueLabel.trailingAnchor.constraint(lessThanOrEqualTo: trailingAnchor, constant: -32)
        ])

        // 留白：libmpv OpenGL FBO / Metal 纹理在此 NSView 上合成。
        // 留白：Syphon 输出可在完成合成后共享同一张 Metal 纹理。
    }

    required init?(coder: NSCoder) {
        fatalError("StageOutputView 仅支持代码初始化")
    }

    func update(currentCueName: String) {
        cueLabel.stringValue = currentCueName
    }
}

@MainActor
final class StageOutputWindowController: NSWindowController {
    private let stageView: StageOutputView
    private var cancellable: AnyCancellable?

    init(store: CueStore) {
        let stageView = StageOutputView(frame: NSRect(x: 0, y: 0, width: 1280, height: 720))
        self.stageView = stageView
        let window = NSWindow(
            contentRect: stageView.frame,
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = "EventKyu 舞台输出"
        window.backgroundColor = .black
        window.contentView = stageView
        window.isReleasedWhenClosed = false
        super.init(window: window)
        cancellable = store.$currentCueName.receive(on: DispatchQueue.main).sink { [weak stageView] name in
            stageView?.update(currentCueName: name)
        }
    }

    required init?(coder: NSCoder) {
        fatalError("StageOutputWindowController 仅支持代码初始化")
    }

    func showWindowed() {
        guard let window else { return }
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.level = .normal
        window.setFrame(NSRect(x: 0, y: 0, width: 1280, height: 720), display: true)
        window.center()
        showWindow(nil)
    }

    /// 全屏到调用方指定的屏幕；未指定时优先选择第二块显示器。
    func presentFullScreen(on requestedScreen: NSScreen? = nil) {
        let targetScreen = requestedScreen
            ?? NSScreen.screens.first(where: { $0 != NSScreen.main })
            ?? NSScreen.main
            ?? NSScreen.screens.first
        guard let window, let targetScreen else { return }

        window.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        window.styleMask = [.borderless]
        window.level = .screenSaver
        window.setFrame(targetScreen.frame, display: true)
        window.makeKeyAndOrderFront(nil)
    }
}
