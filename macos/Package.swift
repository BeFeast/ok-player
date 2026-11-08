// swift-tools-version: 5.9
// The macOS shell as a Swift package: the AppKit shell library, its XCTest target and
// the thin `ok-player` executable the app bundle runs.
//
// Build it through scripts/build-macos-preview.sh, not on its own. Before `swift
// build` the script writes the two generated inputs this package expects and links
// against the okp-ffi dylib it just built:
//   * Sources/COKPCore/okp_core.h, the C header cbindgen generates for okp-ffi;
//   * Sources/OKPlayerShell/Generated/Palette.swift, from brand/palette.json.
// Both are ignored by git. The script then runs `swift test` and assembles the .app.
import PackageDescription

let package = Package(
    name: "OKPlayer",
    // NSView.displayLink and @Observable, which the shell is moving to, need macOS 14.
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "ok-player", targets: ["OKPlayer"]),
    ],
    targets: [
        // The okp-ffi C ABI. The module map links libokp_ffi; the build script passes
        // the directory that holds it.
        .systemLibrary(name: "COKPCore", path: "Sources/COKPCore"),
        .target(
            name: "OKPlayerShell",
            dependencies: ["COKPCore"],
            path: "Sources/OKPlayerShell",
            linkerSettings: [.linkedFramework("AppKit"), .linkedFramework("OpenGL")]
        ),
        .executableTarget(
            name: "OKPlayer",
            dependencies: ["OKPlayerShell"],
            path: "Sources/OKPlayer"
        ),
        .testTarget(
            name: "OKPlayerShellTests",
            dependencies: ["OKPlayerShell"],
            path: "Tests/OKPlayerShellTests"
        ),
    ]
)
