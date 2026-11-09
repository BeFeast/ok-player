import Foundation
import UniformTypeIdentifiers

/// `ok-player --check-document-types`, run by the macOS workflow once the built app
/// is registered with Launch Services. Every video, audio and playlist extension the
/// session's `open_formats` lists (okp-core's `media_formats` and the playlist's M3U
/// types) must have a type macOS declares that the bundle's Video, Audio or Playlist
/// document type claims, by its identifier or by conformance (to `public.movie`,
/// `public.audio` or another claimed type), and the Folder document type must claim
/// folders. That is what makes Finder offer the app for the file or folder, in Open
/// With and on the Dock icon. A system identifier in okp-core's document-type table
/// that macOS does not declare leaves its extension without such a type, because the
/// bundle imports no declaration for it; Linux tests cannot see that.
///
/// Conformance to `public.movie` alone is not the rule: macOS declares some
/// containers that hold audio or video (`.asf`) as audiovisual content, which is
/// not a movie, and the Video document type claims those by identifier.
enum DocumentTypeCheck {
    static let argument = "--check-document-types"

    struct Failure: Equatable, CustomStringConvertible {
        /// `.mkv`, or `folders`.
        var item: String
        /// `Video`, `Audio`, `Playlist` or `Folder`.
        var documentType: String
        /// The identifiers of the types macOS declares for the item.
        var declared: [String]

        var description: String {
            "\(item): no type macOS declares for it is claimed by the \(documentType) document type"
                + (declared.isEmpty ? " (it declares none)" : " (it declares \(declared.joined(separator: ", ")))")
        }
    }

    /// The content types the bundle's document types claim (`LSItemContentTypes`),
    /// by `CFBundleTypeName`.
    struct Claims: Equatable {
        var video: [String] = []
        var audio: [String] = []
        var playlist: [String] = []
        var folder: [String] = []
    }

    /// The types macOS knows for a file extension, declared or dynamic.
    typealias Lookup = (_ fileExtension: String) -> [UTType]

    static let system: Lookup = { UTType.types(tag: $0, tagClass: .filenameExtension, conformingTo: nil) }

    /// The claims of the Video, Audio, Playlist and Folder document types of an
    /// `Info.plist`.
    static func claims(_ info: [String: Any]) -> Claims {
        let types = info["CFBundleDocumentTypes"] as? [[String: Any]] ?? []
        func claimed(_ name: String) -> [String] {
            types.first(where: { $0["CFBundleTypeName"] as? String == name })?["LSItemContentTypes"] as? [String] ?? []
        }
        return Claims(video: claimed("Video"), audio: claimed("Audio"), playlist: claimed("Playlist"),
                      folder: claimed("Folder"))
    }

    /// Whether one of `declared` is claimed, by identifier or by conformance.
    private static func offered(_ declared: [UTType], claimed: [String]) -> Bool {
        let claimedTypes = claimed.compactMap { UTType($0) }
        return declared.contains { type in
            claimed.contains(type.identifier) || claimedTypes.contains { type.conforms(to: $0) }
        }
    }

    /// Every video, audio and playlist extension without a declared type its document
    /// type claims, and folders when the Folder document type does not claim them.
    static func failures(_ formats: OpenFormats, claims: Claims, lookup: Lookup = system) -> [Failure] {
        let kinds = [(name: "Video", extensions: formats.video, claimed: claims.video),
                     (name: "Audio", extensions: formats.audio, claimed: claims.audio),
                     (name: "Playlist", extensions: formats.playlist, claimed: claims.playlist)]
        var found = kinds.flatMap { kind in
            kind.extensions.compactMap { ext -> Failure? in
                let declared = lookup(ext).filter { !$0.isDynamic }
                return offered(declared, claimed: kind.claimed)
                    ? nil : Failure(item: ".\(ext)", documentType: kind.name, declared: declared.map(\.identifier))
            }
        }
        if !offered([.folder], claimed: claims.folder) {
            found.append(Failure(item: "folders", documentType: "Folder", declared: [UTType.folder.identifier]))
        }
        return found
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
        let found = Self.failures(formats, claims: claimed)
        for failure in found { print("OKP_DOCUMENT_TYPES failure \(failure)") }
        // Extensions offered only through a claimed identifier, not as a movie or audio.
        for (extensions, umbrella) in [(formats.video, UTType.movie), (formats.audio, UTType.audio)] {
            for ext in extensions {
                let declared = system(ext).filter { !$0.isDynamic }
                guard !declared.isEmpty, !declared.contains(where: { $0.conforms(to: umbrella) }) else { continue }
                print("OKP_DOCUMENT_TYPES not_\(umbrella.identifier) .\(ext) \(describe(declared))")
            }
        }
        // What each playlist extension is to macOS, for the record.
        for ext in formats.playlist {
            print("OKP_DOCUMENT_TYPES playlist .\(ext) \(describe(system(ext).filter { !$0.isDynamic }))")
        }
        if !found.isEmpty {
            for identifier in claimed.video + claimed.audio + claimed.playlist + claimed.folder {
                let type = UTType(identifier)
                let extensions = (type?.tags[.filenameExtension] ?? []).joined(separator: ",")
                let supertypes = (type?.supertypes.map { $0.identifier } ?? []).sorted().joined(separator: ",")
                let declared = type?.isDeclared ?? false
                print("OKP_DOCUMENT_TYPES claimed \(identifier) declared=\(declared) extensions=\(extensions) "
                      + "supertypes=\(supertypes)")
            }
        }
        print("OKP_DOCUMENT_TYPES video=\(formats.video.count) audio=\(formats.audio.count) "
              + "playlist=\(formats.playlist.count) claimed_video=\(claimed.video.count) "
              + "claimed_audio=\(claimed.audio.count) claimed_playlist=\(claimed.playlist.count) "
              + "claimed_folder=\(claimed.folder.count) failures=\(found.count)")
        let complete = !formats.video.isEmpty && !formats.audio.isEmpty && !formats.playlist.isEmpty
            && !claimed.video.isEmpty && !claimed.audio.isEmpty && !claimed.playlist.isEmpty && !claimed.folder.isEmpty
        return found.isEmpty && complete ? 0 : 1
    }

    /// `identifier<supertype,...>` for each type.
    private static func describe(_ types: [UTType]) -> String {
        types.map { type in
            "\(type.identifier)<\(type.supertypes.map { $0.identifier }.sorted().joined(separator: ","))>"
        }.joined(separator: " ")
    }
}
