import XCTest
@testable import OKPlayerShell

final class SettingsProtocolTests: XCTestCase {
    static let fixtures: Set<String> = [
        "settings/command-theme.json", "settings/query-settings.json", "settings/query-search.json",
        "settings/reply-settings.json", "settings/reply-search.json",
    ]

    func testSettingsDocumentsRoundTrip() throws {
        let commandPath = "settings/command-theme.json"
        let command = CommandEnvelope(command: .setAppearanceTheme(theme: .light))
        XCTAssertEqual(try GoldenFixtures.decode(CommandEnvelope.self, commandPath), command)
        XCTAssertEqual(try GoldenFixtures.encodedObject(command), try GoldenFixtures.object(commandPath))

        for (file, query) in [("query-settings.json", Query.settings),
                              ("query-search.json", .settingsSearch(text: "history retention"))] {
            let path = "settings/\(file)"
            let envelope = QueryEnvelope(query: query)
            XCTAssertEqual(try GoldenFixtures.decode(QueryEnvelope.self, path), envelope)
            XCTAssertEqual(try GoldenFixtures.encodedObject(envelope), try GoldenFixtures.object(path))
        }

        let settingsPath = "settings/reply-settings.json"
        let settings = try GoldenFixtures.decode(QueryReply.self, settingsPath)
        guard case .settings(let state) = settings.result else { return XCTFail("Missing Settings state") }
        XCTAssertEqual(state.theme, .auto)
        XCTAssertEqual(state.sections.filter(\.available).map(\.id), [.appearance, .updates])
        XCTAssertEqual(state.about.id, .about)
        XCTAssertEqual(try GoldenFixtures.encodedObject(settings), try GoldenFixtures.object(settingsPath))

        let searchPath = "settings/reply-search.json"
        let search = try GoldenFixtures.decode(QueryReply.self, searchPath)
        let integration = SettingsDestination(id: .integration, title: "Integration", available: false)
        XCTAssertEqual(search.result, .settingsSearch(hits: [
            SettingsSearchHit(label: "History retention", destination: integration),
        ]))
        XCTAssertEqual(try GoldenFixtures.encodedObject(search), try GoldenFixtures.object(searchPath))
    }
}
