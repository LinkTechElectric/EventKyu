// swift-tools-version: 5.9
import Foundation
import PackageDescription

// 使用 manifest 所在目录推导静态库路径，移动整个仓库后仍可直接构建。
let packageDirectory = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()
    .path
let coreFFIDirectory = "\(packageDirectory)/Sources/EventKyuCoreFFI"

let package = Package(
    name: "EventKyu",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "EventKyu", targets: ["EventKyu"])
    ],
    targets: [
        // UniFFI 生成的 C 头文件与 module map；静态库由 build-core.sh 放入此处。
        .systemLibrary(
            name: "EventKyuCoreFFI",
            path: "Sources/EventKyuCoreFFI"
        ),
        .target(
            name: "EventKyuCore",
            dependencies: ["EventKyuCoreFFI"],
            path: "Sources/EventKyuCore",
            sources: ["Generated/EventKyuCore.swift"]
        ),
        .executableTarget(
            name: "EventKyu",
            dependencies: ["EventKyuCore"],
            path: "Sources/EventKyu",
            linkerSettings: [
                .linkedLibrary("eventkyu_uniffi"),
                // Rust 静态库在本机链接时需要 C++ 运行时与这些系统框架。
                .linkedLibrary("c++"),
                .linkedFramework("Foundation"),
                .linkedFramework("CoreFoundation"),
                .linkedFramework("Security"),
                .unsafeFlags(["-L\(coreFFIDirectory)"])
            ]
        )
    ]
)
