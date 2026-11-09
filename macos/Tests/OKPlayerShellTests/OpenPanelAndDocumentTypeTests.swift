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

/// `--check-document-types`: every video and audio extension has a declared type the
/// Video or Audio document type claims.
final class DocumentTypeCheckTests: XCTestCase {
    private let video = ["public.movie", "com.microsoft.advanced-systems-format"]
    private let audio = ["public.audio"]

    func testTypesMacOSDeclaresPassWithTheSystemsAnswer() {
        let system = OpenFormats(video: ["mp4", "mov", "avi"], audio: ["mp3", "m4a", "wav"], subtitle: [], playlist: [])
        XCTAssertEqual(DocumentTypeCheck.failures(system, video: video, audio: audio), [])
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
        let failures = DocumentTypeCheck.failures(checked, video: video + [claimedByName.identifier], audio: audio,
                                                  lookup: lookup)
        XCTAssertEqual(failures, [
            DocumentTypeCheck.Failure(fileExtension: "mts", documentType: "Video", declared: ["public.mp3"]),
            DocumentTypeCheck.Failure(fileExtension: "xyz", documentType: "Video", declared: []),
        ])
        XCTAssertTrue(failures[1].description.hasPrefix(".xyz: no type macOS declares for it is claimed by the Video"))
    }

    func testWithoutClaimedTypesEveryExtensionFails() {
        let formats = OpenFormats(video: ["mp4"], audio: ["mp3"], subtitle: [], playlist: [])
        XCTAssertEqual(DocumentTypeCheck.failures(formats, video: [], audio: []).map(\.fileExtension), ["mp4", "mp3"])
    }

    func testTheClaimsAreReadFromTheDocumentTypesOfAnInfoPlist() {
        let info: [String: Any] = ["CFBundleDocumentTypes": [
            ["CFBundleTypeName": "Audio", "LSItemContentTypes": ["public.audio", "org.xiph.flac"]],
            ["CFBundleTypeName": "Video", "LSItemContentTypes": ["public.movie", "org.matroska.mkv"]],
        ]]
        let claims = DocumentTypeCheck.claims(info)
        XCTAssertEqual(claims.video, ["public.movie", "org.matroska.mkv"])
        XCTAssertEqual(claims.audio, ["public.audio", "org.xiph.flac"])
        XCTAssertTrue(DocumentTypeCheck.claims([:]).video.isEmpty)
    }

    func testTheCheckRunsOnlyWhenAskedFor() {
        XCTAssertNil(DocumentTypeCheck.runIfRequested(["ok-player", "--smoke-test"]))
    }
}
