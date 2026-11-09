import Foundation
import UniformTypeIdentifiers

/// `ok-player --check-document-types`, run by the macOS workflow once the built app
/// is registered with Launch Services: every video and audio extension the
/// session's `open_formats` lists (okp-core's `media_formats`) must resolve to a
/// declared type that conforms to `public.movie` or `public.audio`. A system
/// identifier in okp-core's document-type table that macOS does not declare leaves
/// its extension without such a type, because the bundle imports no declaration
/// for it; Linux tests cannot see that.
enum DocumentTypeCheck {
    static let argument = "--check-document-types"

    struct Failure: Equatable, CustomStringConvertible {
        var fileExtension: String
        var umbrella: UTType
        /// The type macOS gave the extension, if any.
        var resolved: String?

        var description: String {
            ".\(fileExtension) has no declared type conforming to \(umbrella.identifier)"
                + (resolved.map { " (it resolves to \($0))" } ?? "")
        }
    }

    /// Finds the declared type for an extension that conforms to the umbrella type.
    typealias Resolve = (_ fileExtension: String, _ umbrella: UTType) -> UTType?

    /// `UTType(filenameExtension:conformingTo:)`, which makes up a dynamic type when
    /// nothing is declared.
    static let system: Resolve = { UTType(filenameExtension: $0, conformingTo: $1) }

    /// Every video extension without a declared movie type and every audio
    /// extension without a declared audio type.
    static func failures(_ formats: OpenFormats, resolve: Resolve = system) -> [Failure] {
        let kinds: [(extensions: [String], umbrella: UTType)] = [(formats.video, .movie), (formats.audio, .audio)]
        return kinds.flatMap { kind in
            kind.extensions.compactMap { ext -> Failure? in
                let type = resolve(ext, kind.umbrella)
                if let type, !type.isDynamic, type.conforms(to: kind.umbrella) { return nil }
                return Failure(fileExtension: ext, umbrella: kind.umbrella, resolved: type?.identifier)
            }
        }
    }

    /// Runs the check when the app was started for it: prints one line per failure
    /// and a summary, and returns the exit status. `nil` when it was not asked for.
    static func runIfRequested(_ arguments: [String]) -> Int32? {
        guard arguments.contains(argument) else { return nil }
        let formats: OpenFormats
        do {
            let client = try SessionClient(config: .forApp())
            defer { client.close() }
            guard case .openFormats(let answer)? = client.query(.openFormats) else {
                fputs("OK Player: the session did not answer open_formats\n", stderr)
                return 1
            }
            formats = answer
        } catch {
            fputs("OK Player: \(error)\n", stderr)
            return 1
        }
        let found = Self.failures(formats)
        for failure in found { print("OKP_DOCUMENT_TYPES failure \(failure)") }
        print("OKP_DOCUMENT_TYPES video=\(formats.video.count) audio=\(formats.audio.count) failures=\(found.count)")
        return found.isEmpty && !formats.video.isEmpty && !formats.audio.isEmpty ? 0 : 1
    }
}
