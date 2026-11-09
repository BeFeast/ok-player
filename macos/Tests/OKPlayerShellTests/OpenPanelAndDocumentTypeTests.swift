import AppKit
import UniformTypeIdentifiers
import XCTest
@testable import OKPlayerShell

private let formats = OpenFormats(video: ["mkv", "mp4", "mpg", "mpeg"], audio: ["mp3", "flac"],
                                  subtitle: ["srt"], playlist: ["m3u", "m3u8"])

/// File > Open and File > Open Folder.
final class OpenPanelConfigurationTests: XCTestCase {
    func testOpenTakesSeveralFilesOfTheSessionsTypesEachOnce() throws {
        let configuration = OpenPanelConfiguration.files(formats)
        XCTAssertTrue(configuration.canChooseFiles)
        XCTAssertFalse(configuration.canChooseDirectories)
        XCTAssertTrue(configuration.allowsMultipleSelection)
        let types = configuration.allowedContentTypes
        for ext in ["mkv", "mp4", "mp3", "flac", "srt", "m3u", "m3u8"] {
            let type = try XCTUnwrap(UTType(filenameExtension: ext))
            XCTAssertTrue(types.contains(type), ".\(ext) is offered")
        }
        XCTAssertEqual(Set(types).count, types.count, "a type is offered once")
        // macOS declares .mpg and .mpeg as one type: it is listed once for both.
        let mpeg = try XCTUnwrap(UTType(filenameExtension: "mpg"))
        XCTAssertEqual(UTType(filenameExtension: "mpeg"), mpeg)
        XCTAssertEqual(types.filter { $0 == mpeg }.count, 1)
    }

    func testWithoutTheSessionsTypesOpenOffersEveryFile() {
        XCTAssertEqual(OpenPanelConfiguration.files(nil).allowedContentTypes, [])
    }

    func testOpenFolderTakesSeveralFoldersAndNoFiles() {
        let configuration = OpenPanelConfiguration.folders
        XCTAssertFalse(configuration.canChooseFiles)
        XCTAssertTrue(configuration.canChooseDirectories)
        XCTAssertTrue(configuration.allowsMultipleSelection)
        XCTAssertEqual(configuration.allowedContentTypes, [])
    }
}

/// `--check-document-types`: every video, audio and playlist extension has a declared
/// type its document type claims, and the Folder document type claims folders.
final class DocumentTypeCheckTests: XCTestCase {
    private typealias Claims = DocumentTypeCheck.Claims
    private typealias Failure = DocumentTypeCheck.Failure
    private let claims = Claims(video: ["public.movie", "com.microsoft.advanced-systems-format"], audio: ["public.audio"],
                                playlist: ["public.m3u-playlist"], folder: ["public.folder"])

    func testTypesMacOSDeclaresPassWithTheSystemsAnswer() {
        let system = OpenFormats(video: ["mp4", "mov", "avi"], audio: ["mp3", "m4a", "wav"], subtitle: [],
                                 playlist: ["m3u"])
        XCTAssertEqual(DocumentTypeCheck.failures(system, claims: claims), [])
    }

    func testATypeIsOfferedByConformanceOrByItsClaimedIdentifier() throws {
        let mpeg4 = try XCTUnwrap(UTType("public.mpeg-4"))
        let mp3 = try XCTUnwrap(UTType("public.mp3"))
        // Not a movie, but the Video document type claims it by name.
        let claimedByName = UTType.plainText
        let lookup: DocumentTypeCheck.Lookup = { ext in
            switch ext {
            case "mp4": return [mpeg4]
            case "txt": return [claimedByName]
            // An audio type where a video type is expected.
            case "mts": return [mp3]
            // Nothing declared: macOS makes up a dynamic type.
            default: return [UTType(filenameExtension: "okp-undeclared-\(ext)")].compactMap { $0 }
            }
        }
        let checked = OpenFormats(video: ["mp4", "txt", "mts", "xyz"], audio: [], subtitle: [], playlist: [])
        var withText = claims
        withText.video.append(claimedByName.identifier)
        let failures = DocumentTypeCheck.failures(checked, claims: withText, lookup: lookup)
        XCTAssertEqual(failures, [
            Failure(item: ".mts", documentType: "Video", declared: ["public.mp3"]),
            Failure(item: ".xyz", documentType: "Video", declared: []),
        ])
        XCTAssertTrue(failures[1].description.hasPrefix(".xyz: no type macOS declares for it is claimed by the Video"))
    }

    func testPlaylistFilesAndFoldersNeedTheirOwnDocumentTypes() {
        let formats = OpenFormats(video: [], audio: [], subtitle: [], playlist: ["m3u"])
        let lookup: DocumentTypeCheck.Lookup = { $0 == "m3u" ? [.m3uPlaylist] : [] }
        XCTAssertEqual(DocumentTypeCheck.failures(formats, claims: claims, lookup: lookup), [])
        // Without the Folder and Playlist document types, Finder offers the app for neither.
        let mediaOnly = Claims(video: claims.video, audio: claims.audio)
        XCTAssertEqual(DocumentTypeCheck.failures(formats, claims: mediaOnly, lookup: lookup), [
            Failure(item: ".m3u", documentType: "Playlist", declared: ["public.m3u-playlist"]),
            Failure(item: "folders", documentType: "Folder", declared: ["public.folder"]),
        ])
    }

    func testWithoutClaimedTypesEveryItemFails() {
        let formats = OpenFormats(video: ["mp4"], audio: ["mp3"], subtitle: [], playlist: ["m3u"])
        XCTAssertEqual(DocumentTypeCheck.failures(formats, claims: Claims()).map(\.item),
                       [".mp4", ".mp3", ".m3u", "folders"])
    }

    func testTheClaimsAreReadFromTheDocumentTypesOfAnInfoPlist() {
        let info: [String: Any] = ["CFBundleDocumentTypes": [
            ["CFBundleTypeName": "Audio", "LSItemContentTypes": ["public.audio", "org.xiph.flac"]],
            ["CFBundleTypeName": "Video", "LSItemContentTypes": ["public.movie", "org.matroska.mkv"]],
            ["CFBundleTypeName": "Folder", "LSItemContentTypes": ["public.folder"]],
            ["CFBundleTypeName": "Playlist", "LSItemContentTypes": ["public.m3u-playlist"]],
        ]]
        XCTAssertEqual(DocumentTypeCheck.claims(info),
                       Claims(video: ["public.movie", "org.matroska.mkv"], audio: ["public.audio", "org.xiph.flac"],
                              playlist: ["public.m3u-playlist"], folder: ["public.folder"]))
        XCTAssertEqual(DocumentTypeCheck.claims([:]), Claims())
    }

    func testTheCheckRunsOnlyWhenAskedFor() {
        XCTAssertNil(DocumentTypeCheck.runIfRequested(["ok-player", "--smoke-test"]))
    }
}
