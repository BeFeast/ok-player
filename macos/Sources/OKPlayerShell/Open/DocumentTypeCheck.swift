import Foundation
import UniformTypeIdentifiers

/// `ok-player --check-document-types`, run by the macOS workflow once the built app
/// is registered with Launch Services: every video and audio extension the
/// session's `open_formats` lists (okp-core's `media_formats`) must have a type
/// macOS declares that the bundle's Video or Audio document type claims, by its
/// identifier or by conformance (to `public.movie`, `public.audio` or another
/// claimed type). That is what makes Finder offer the app for the file. A system
/// identifier in okp-core's document-type table that macOS does not declare leaves
/// its extension without such a type, because the bundle imports no declaration for
/// it; Linux tests cannot see that.
///
/// Conformance to `public.movie` alone is not the rule: macOS declares some
/// containers that hold audio or video (`.asf`) as audiovisual content, which is
/// not a movie, and the Video document type claims those by identifier.
enum DocumentTypeCheck {
    static let argument = "--check-document-types"

    struct Failure: Equatable, CustomStringConvertible {
        var fileExtension: String
        /// `Video` or `Audio`.
        var documentType: String
        /// The identifiers of the types macOS declares for the extension.
        var declared: [String]

        var description: String {
            ".\(fileExtension): no type macOS declares for it is claimed by the \(documentType) document type"
                + (declared.isEmpty ? " (it declares none)" : " (it declares \(declared.joined(separator: ", ")))")
        }
    }

    /// The types macOS knows for a file extension, declared or dynamic.
    typealias Lookup = (_ fileExtension: String) -> [UTType]

    static let system: Lookup = { UTType.types(tag: $0, tagClass: .filenameExtension, conformingTo: nil) }

    /// The content types the Video and Audio document types of an `Info.plist`
    /// claim (`LSItemContentTypes`).
    static func claims(_ info: [String: Any]) -> (video: [String], audio: [String]) {
        let types = info["CFBundleDocumentTypes"] as? [[String: Any]] ?? []
        func claimed(_ name: String) -> [String] {
            types.first(where: { $0["CFBundleTypeName"] as? String == name })?["LSItemContentTypes"] as? [String] ?? []
        }
        return (claimed("Video"), claimed("Audio"))
    }

    /// Every video extension without a declared type the Video document type claims,
    /// and every audio extension without one the Audio document type claims.
    static func failures(_ formats: OpenFormats, video: [String], audio: [String],
                         lookup: Lookup = system) -> [Failure] {
        let kinds = [(name: "Video", extensions: formats.video, claimed: video),
                     (name: "Audio", extensions: formats.audio, claimed: audio)]
        return kinds.flatMap { kind in
            let claimedTypes = kind.claimed.compactMap { UTType($0) }
            return kind.extensions.compactMap { ext -> Failure? in
                let declared = lookup(ext).filter { !$0.isDynamic }
                let offered = declared.contains { type in
                    kind.claimed.contains(type.identifier) || claimedTypes.contains { type.conforms(to: $0) }
                }
                return offered ? nil : Failure(fileExtension: ext, documentType: kind.name,
                                               declared: declared.map(\.identifier))
            }
        }
    }

    /// Runs the check when the app was started for it: prints one line per failure,
    /// what each claimed type is to macOS when something failed, and a summary, and
    /// returns the exit status. `nil` when it was not asked for.
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
        let claimed = claims(Bundle.main.infoDictionary ?? [:])
        let found = Self.failures(formats, video: claimed.video, audio: claimed.audio)
        for failure in found { print("OKP_DOCUMENT_TYPES failure \(failure)") }
        // Extensions offered only through a claimed identifier, not as a movie or audio.
        for (extensions, umbrella) in [(formats.video, UTType.movie), (formats.audio, UTType.audio)] {
            for ext in extensions {
                let declared = system(ext).filter { !$0.isDynamic }
                guard !declared.isEmpty, !declared.contains(where: { $0.conforms(to: umbrella) }) else { continue }
                let described = declared.map { type -> String in
                    let supertypes = type.supertypes.map { $0.identifier }.sorted().joined(separator: ",")
                    return "\(type.identifier)<\(supertypes)>"
                }
                print("OKP_DOCUMENT_TYPES not_\(umbrella.identifier) .\(ext) \(described.joined(separator: " "))")
            }
        }
        if !found.isEmpty {
            for identifier in claimed.video + claimed.audio {
                let type = UTType(identifier)
                let extensions = (type?.tags[.filenameExtension] ?? []).joined(separator: ",")
                let supertypes = (type?.supertypes.map { $0.identifier } ?? []).sorted().joined(separator: ",")
                let declared = type?.isDeclared ?? false
                print("OKP_DOCUMENT_TYPES claimed \(identifier) declared=\(declared) extensions=\(extensions) "
                      + "supertypes=\(supertypes)")
            }
        }
        print("OKP_DOCUMENT_TYPES video=\(formats.video.count) audio=\(formats.audio.count) "
              + "claimed_video=\(claimed.video.count) claimed_audio=\(claimed.audio.count) failures=\(found.count)")
        let complete = !formats.video.isEmpty && !formats.audio.isEmpty && !claimed.video.isEmpty && !claimed.audio.isEmpty
        return found.isEmpty && complete ? 0 : 1
    }
}
