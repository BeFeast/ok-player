import AppKit

final class PlayerUpdates: NSObject {
    enum Channel: String {
        case beta, stable
        var title: String { rawValue.capitalized }
    }

    struct Offer: Decodable, Equatable {
        let build: String
        let version: String
        let sha256: String
        let key: String
    }

    struct Fixture {
        let port: String
        let key: String
    }

    let build: String
    let version: String
    private let source: String
    private let bundle: URL
    private let helper: URL
    var fixture: Fixture?
    var changed: (() -> Void)?
    var available: ((Offer) -> Void)?
    var willRelaunch: (() -> Void)?
    private var timer: Timer?
    private var automatic = false
    private var lastAutomatic: Date?
    private var operation: Process?
    private(set) var lastCheck: Date?
    private(set) var offer: Offer?
    private(set) var problem: String?
    private(set) var checking = false
    private(set) var installing = false
    private(set) var skipped = false
    private(set) var channel: Channel

    init(bundle: Bundle = .main, defaults: UserDefaults = .standard) {
        let info = bundle.infoDictionary ?? [:]
        self.bundle = bundle.bundleURL
        helper = bundle.bundleURL.appendingPathComponent("Contents/MacOS/okp-updater")
        version = info["CFBundleShortVersionString"] as? String ?? "Unavailable"
        build = info["CFBundleVersion"] as? String ?? "Unavailable"
        source = info["OKPSourceSHA"] as? String ?? ""
        channel = defaults.string(forKey: "signedUpdateChannel").flatMap(Channel.init(rawValue:)) ?? .beta
        super.init()
    }

    var canCheck: Bool { FileManager.default.isExecutableFile(atPath: helper.path) }
    var canInstall: Bool {
        fixture != nil || bundle.resolvingSymlinksInPath().path ==
            FileManager.default.homeDirectoryForCurrentUser.resolvingSymlinksInPath()
                .appendingPathComponent("Applications/OK Player.app").path
    }

    func start() {
        guard canCheck else { return }
        automatic = true
        automaticallyCheck()
        schedule()
    }

    private func schedule() {
        timer = Timer.scheduledTimer(withTimeInterval: Double.random(in: 28 * 60...30 * 60), repeats: false) {
            [weak self] _ in
            self?.automaticallyCheck()
            self?.schedule()
        }
    }

    func automaticallyCheck() {
        guard automatic, !checking, !installing,
              lastAutomatic.map({ Date().timeIntervalSince($0) >= 5 * 60 }) ?? true else { return }
        lastAutomatic = Date()
        check(automatic: true)
    }

    func choose(_ selected: Channel) {
        guard selected != channel, !checking, !installing else { return }
        channel = selected
        if fixture == nil { UserDefaults.standard.set(channel.rawValue, forKey: "signedUpdateChannel") }
        lastCheck = nil
        offer = nil
        skipped = false
        check()
    }

    func skip() {
        guard let offer, !checking, !installing else { return }
        if fixture == nil { UserDefaults.standard.set(offer.build, forKey: skipKey) }
        skipped = true
        changed?()
    }

    private var skipKey: String { "signedUpdateSkipped.\(channel.rawValue)" }

    func check(automatic: Bool = false) {
        guard canCheck, !checking, !installing else { return }
        checking = true
        problem = nil
        changed?()
        run("check") { [weak self] result in
            guard let self else { return }
            self.checking = false
            self.lastCheck = Date()
            do {
                let previous = self.offer
                let bytes = try result.get()
                self.offer = try JSONDecoder().decode(Offer?.self, from: bytes)
                self.skipped = self.offer.map { $0.build == UserDefaults.standard.string(forKey: self.skipKey) } ?? false
                if automatic, !self.skipped, let offer = self.offer, offer != previous {
                    self.available?(offer)
                }
            } catch { self.problem = error.localizedDescription }
            self.changed?()
        }
    }

    func install() {
        guard let offer, canInstall, !checking, !installing else { return }
        installing = true
        problem = nil
        changed?()
        run("install", extra: [offer.sha256, String(ProcessInfo.processInfo.processIdentifier)]) { [weak self] result in
            guard let self else { return }
            switch result {
            case .success:
                self.willRelaunch?()
                NSApp.terminate(nil)
            case .failure(let error):
                self.installing = false
                self.problem = error.localizedDescription
                self.changed?()
            }
        }
    }

    private func run(_ action: String, extra: [String] = [],
                     finish: @escaping (Result<Data, Error>) -> Void) {
        let process = Process()
        let output = Pipe()
        let input = Pipe()
        process.executableURL = helper
        var args = [action, channel.rawValue, build, source, bundle.path]
        if let fixture {
            args[0] = "fixture-" + action
            args += [fixture.port, fixture.key]
        }
        process.arguments = args + extra
        process.standardOutput = output
        process.standardError = output
        process.standardInput = input
        do {
            try process.run()
            operation = process
        } catch { finish(.failure(error)); return }
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let result = Result<Data, Error> {
                let reader = output.fileHandleForReading
                if action == "install" {
                    var line = Data()
                    while let byte = try reader.read(upToCount: 1), !byte.isEmpty {
                        line.append(byte)
                        if byte == Data([10]) { break }
                    }
                    if line == Data("ready\n".utf8) {
                        try input.fileHandleForWriting.write(contentsOf: Data("relaunch\n".utf8))
                        try input.fileHandleForWriting.close()
                        return line
                    }
                    line.append(try reader.readToEnd() ?? Data())
                    throw UpdateError(line)
                }
                let bytes = try reader.readToEnd() ?? Data()
                process.waitUntilExit()
                guard process.terminationStatus == 0 else { throw UpdateError(bytes) }
                return bytes
            }
            DispatchQueue.main.async {
                self?.operation = nil
                finish(result)
            }
        }
    }

    private struct UpdateError: LocalizedError {
        let errorDescription: String?
        init(_ bytes: Data) {
            let message = String(decoding: bytes, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            errorDescription = message.isEmpty ? "The updater stopped unexpectedly." : message
        }
    }

    static var restartedFixture: URL? {
        let environment = ProcessInfo.processInfo.environment
        guard environment["OKP_UPDATE_RESTART_RECEIPT"] != nil,
              let profile = environment["OKP_VERIFY_UPDATE_RESTART_PROFILE"] else { return nil }
        let temporary = URL(fileURLWithPath: NSTemporaryDirectory()).resolvingSymlinksInPath().path
        guard Bundle.main.bundleURL.resolvingSymlinksInPath().path.hasPrefix(temporary + "/") else { return nil }
        return URL(fileURLWithPath: profile, isDirectory: true)
    }

    static func confirmStartup() {
        let environment = ProcessInfo.processInfo.environment
        guard let path = environment["OKP_UPDATE_RESTART_RECEIPT"],
              let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String,
              let source = Bundle.main.object(forInfoDictionaryKey: "OKPSourceSHA") as? String else { return }
        try? Data("\(build)\n\(source)\n".utf8).write(
            to: URL(fileURLWithPath: path), options: [.withoutOverwriting]
        )
        if let fixture = restartedFixture {
            let result: [String: Any] = ["pid": getpid(), "build": build, "source_sha": source]
            if let bytes = try? JSONSerialization.data(withJSONObject: result) {
                try? bytes.write(to: fixture.appendingPathComponent("restarted.json"), options: .atomic)
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { NSApp.terminate(nil) }
        }
    }
}
