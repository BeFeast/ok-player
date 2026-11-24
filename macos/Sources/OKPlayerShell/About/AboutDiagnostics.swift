import AppKit

struct EngineDiagnostics: Codable, Equatable {
    enum RenderAPI: String, Codable {
        // The session JSON decoder converts snake_case keys, leaving enum values alone.
        case openGL = "open_gl"
    }

    var mpvVersion: String?
    var ffmpegVersion: String?
    var renderApi: RenderAPI?
    var hardwareDecode: String?
}

struct AboutDiagnostics {
    struct Row {
        let name: String
        let value: String

        init(_ name: String, _ value: String?) {
            self.name = name
            self.value = value ?? "Unavailable"
        }
    }

    let name: String
    let version: String?
    let app: [Row]
    let engine: [Row]
    let host: [Row]

    static let projectURL = URL(string: "https://github.com/BeFeast/ok-player")!
    static let licenseURL = projectURL.appendingPathComponent("blob/main/LICENSE")

    init(bundle: Bundle = .main, runtime: EngineDiagnostics?) {
        func info(_ key: String) -> String? { bundle.object(forInfoDictionaryKey: key) as? String }
        name = info("CFBundleName") ?? "OK Player"
        version = info("CFBundleShortVersionString")
        app = [
            Row("Version", version),
            Row("Channel", info("OKPReleaseChannel")),
            Row("Build", info("CFBundleVersion")),
            Row("Revision", info("OKPSourceRevision")),
            Row("License", info("OKPLicense")),
        ]
        engine = [
            Row("libmpv", runtime?.mpvVersion),
            Row("FFmpeg", runtime?.ffmpegVersion),
            Row("Render API", runtime?.renderApi == .openGL ? "libmpv render" : nil),
            Row("Graphics", runtime?.renderApi == .openGL ? "OpenGL · CAOpenGLLayer" : nil),
            Row("Hardware decode", runtime.map { $0.hardwareDecode ?? "Inactive" }),
        ]
        let system = ProcessInfo.processInfo.operatingSystemVersion
        host = [
            Row("macOS", "\(system.majorVersion).\(system.minorVersion).\(system.patchVersion)"),
            Row("AppKit", String(NSAppKitVersionNumber)),
            Row("CPU", Self.machineArchitecture()),
            Row("Install", info("OKPPackageKind")),
        ]
    }

    var plainText: String {
        func section(_ title: String, _ rows: [Row]) -> String {
            ([title] + rows.map { "  \($0.name): \($0.value)" }).joined(separator: "\n")
        }
        return [name, section("App", app), section("Engine", engine), section("Host", host)]
            .joined(separator: "\n\n")
    }

    private static func machineArchitecture() -> String? {
        var host = utsname()
        guard uname(&host) == 0 else { return nil }
        return withUnsafeBytes(of: host.machine) { bytes in
            String(decoding: bytes.prefix { $0 != 0 }, as: UTF8.self)
        }
    }
}
