import AppKit
import COKPCore
import ImageIO
import SwiftUI
import UniformTypeIdentifiers

/// One decoder and cache for both history surfaces. Disk and media access runs
/// serially off the main thread, with stale and private writes gated at commit.
final class HistoryPosters: @unchecked Sendable {
    private let queue = DispatchQueue(label: "uk.oklabs.ok-player.history-posters", qos: .utility)
    private let images = NSCache<NSString, CGImage>()
    private let retryAfter = NSCache<NSString, NSDate>()
    private let lock = NSLock()
    private var isPrivate = false
    private var generation: UInt64 = 0
    private let directory: URL?
    private let canDecode: Bool

    init(directory: URL? = nil, canDecode: Bool = true) {
        self.directory = directory
        self.canDecode = canDecode
        images.totalCostLimit = 24 * 1024 * 1024
        retryAfter.countLimit = 128
    }

    func setPrivate(_ value: Bool) {
        lock.withLock {
            guard value != isPrivate else { return }
            isPrivate = value
            generation &+= 1
        }
    }

    private func fingerprint(_ path: String) -> String? {
        var key = [CChar](repeating: 0, count: 17)
        let length = UInt(key.count)
        guard path.withCString({ okp_history_poster_key($0, &key, length) }) else { return nil }
        return String(cString: key)
    }

    func image(for entry: HistoryEntry, privateSession: Bool) async -> CGImage? {
        guard entry.isAvailable, entry.source.kind == .local, !Task.isCancelled else { return nil }
        let cancelled = PosterCancellation()
        let allowed = lock.withLock { isPrivate || privateSession || !canDecode ? nil : generation }
        return await withTaskCancellationHandler {
            await withCheckedContinuation { reply in
                queue.async {
                    let image = cancelled.isCancelled ? nil : self.resolve(
                        path: entry.source.location, duration: entry.duration, generation: allowed, cancelled: cancelled)
                    reply.resume(returning: image)
                }
            }
        } onCancel: {
            cancelled.cancel()
        }
    }

    private func resolve(path: String, duration: Double, generation: UInt64?,
                         cancelled: PosterCancellation) -> CGImage? {
        guard let key = fingerprint(path) else { return nil }
        let identity = key as NSString
        if let cached = images.object(forKey: identity) { return cached }
        let url = directory?.appendingPathComponent(key).appendingPathExtension("png")
        if let url, let source = CGImageSourceCreateWithURL(url as CFURL, nil),
           let cached = CGImageSourceCreateImageAtIndex(source, 0, nil) {
            images.setObject(cached, forKey: identity, cost: cached.bytesPerRow * cached.height)
            return cached
        }
        let retryDate = retryAfter.object(forKey: identity).map { $0 as Date } ?? .distantPast
        guard let generation, lock.withLock({ !isPrivate && generation == self.generation }),
              !cancelled.isCancelled, retryDate < Date() else {
            return nil
        }
        guard let decoded = decode(path: path, duration: duration) else {
            retryAfter.setObject(Date().addingTimeInterval(15) as NSDate, forKey: identity)
            return nil
        }
        guard fingerprint(path) == key, !cancelled.isCancelled else { return nil }
        let png = url == nil ? nil : pngData(decoded)
        return lock.withLock {
            guard !isPrivate, generation == self.generation, !cancelled.isCancelled else { return nil }
            if let directory, let url, let png {
                try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
                try? png.write(to: url, options: .atomic)
            }
            images.setObject(decoded, forKey: identity, cost: decoded.bytesPerRow * decoded.height)
            return decoded
        }
    }

    private func decode(path: String, duration: Double) -> CGImage? {
        let width = 384
        let height = 216
        var pixels = Data(count: width * height * 4)
        let count = UInt(pixels.count)
        let decoded = path.withCString { path in
            pixels.withUnsafeMutableBytes { buffer in
                okp_history_poster_decode(path, duration, buffer.bindMemory(to: UInt8.self).baseAddress, count)
            }
        }
        guard decoded, let provider = CGDataProvider(data: pixels as CFData),
              let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) else { return nil }
        let format = CGBitmapInfo.byteOrder32Little.union(CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipFirst.rawValue))
        return CGImage(width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
                       bytesPerRow: width * 4, space: colorSpace, bitmapInfo: format, provider: provider,
                       decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }

    private func pngData(_ image: CGImage) -> Data? {
        let data = NSMutableData()
        guard let writer = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil)
        else { return nil }
        CGImageDestinationAddImage(writer, image, nil)
        return CGImageDestinationFinalize(writer) ? data as Data : nil
    }
}

private final class PosterCancellation: @unchecked Sendable {
    private let lock = NSLock()
    private var cancelled = false

    var isCancelled: Bool { lock.withLock { cancelled } }
    func cancel() { lock.withLock { cancelled = true } }
}

struct HistoryPoster: View {
    let entry: HistoryEntry
    let posters: HistoryPosters
    let privateSession: Bool
    let width: CGFloat
    let height: CGFloat
    @State private var picture: CGImage?

    private struct Request: Equatable {
        let id: String
        let available: Bool
        let revision: Int64
        let privateSession: Bool
    }

    var body: some View {
        Color.clear
            .frame(width: width, height: height)
            .overlay {
                if let picture {
                    Image(decorative: picture, scale: 1)
                        .resizable()
                        .scaledToFill()
                        .opacity(entry.state == .finished ? 0.68 : 1)
                        .historyBounds("poster:\(entry.id)")
                } else {
                    LinearGradient(colors: [PaletteRole.surfaceRaised.swiftUI, PaletteRole.borderSubtle.swiftUI],
                                   startPoint: .topLeading, endPoint: .bottomTrailing)
                    Image(systemName: entry.isAvailable ? "play.fill" : "exclamationmark.triangle")
                        .font(.system(size: width >= 100 ? 22 : 12))
                        .foregroundStyle(PaletteRole.textMuted.swiftUI)
                }
            }
            .overlay(alignment: .bottom) {
                if entry.state == .progress { WatchedBar(fraction: entry.progress) }
            }
            .clipShape(RoundedRectangle(cornerRadius: 7))
            .accessibilityHidden(true)
            .task(id: Request(id: entry.id, available: entry.isAvailable, revision: entry.updatedAtUnix,
                              privateSession: privateSession)) {
                if !entry.isAvailable { picture = nil }
                let resolved = await posters.image(for: entry, privateSession: privateSession)
                if !Task.isCancelled { picture = resolved }
            }
    }
}
