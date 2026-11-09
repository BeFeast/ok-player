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

/// `--check-document-types`: every video and audio extension resolves to a declared
/// type of its kind.
final class DocumentTypeCheckTests: XCTestCase {
    func testTypesMacOSDeclaresPassWithTheSystemsAnswer() {
        let system = OpenFormats(video: ["mp4", "mov", "avi"], audio: ["mp3", "m4a", "wav"], subtitle: [], playlist: [])
        XCTAssertEqual(DocumentTypeCheck.failures(system), [])
    }

    func testAnUndeclaredOrWronglyKindedExtensionFails() throws {
        let movie = try XCTUnwrap(UTType("public.mpeg-4"))
        let mp3 = try XCTUnwrap(UTType("public.mp3"))
        let resolve: DocumentTypeCheck.Resolve = { ext, umbrella in
            switch ext {
            case "mp4": return movie
            // An audio type where a video type is expected.
            case "mts": return mp3
            // Nothing declared: macOS makes up a dynamic type.
            default: return UTType(filenameExtension: "okp-undeclared-\(ext)", conformingTo: umbrella)
            }
        }
        let checked = OpenFormats(video: ["mp4", "mts", "xyz"], audio: [], subtitle: [], playlist: [])
        let failures = DocumentTypeCheck.failures(checked, resolve: resolve)
        XCTAssertEqual(failures.map(\.fileExtension), ["mts", "xyz"])
        XCTAssertEqual(failures.first?.resolved, "public.mp3")
        XCTAssertEqual(failures.first?.umbrella, UTType.movie)
        XCTAssertTrue(failures[1].description.contains(".xyz has no declared type conforming to public.movie"))
    }

    func testTheCheckRunsOnlyWhenAskedFor() {
        XCTAssertNil(DocumentTypeCheck.runIfRequested(["ok-player", "--smoke-test"]))
    }
}
