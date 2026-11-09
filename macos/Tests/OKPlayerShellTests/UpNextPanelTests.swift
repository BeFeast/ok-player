import AppKit
import XCTest
@testable import OKPlayerShell

/// The Up Next panel over real AppKit views, in a window that is never shown: what
/// Return, Delete and the row menu ask the session for, where the keys the list
/// does not handle go, and who gets a scroll over the panel.
final class UpNextPanelTests: XCTestCase {
    private var window: NSWindow!
    private var video: UpNextFocusTarget!
    private var panel: UpNextPanel!
    private var dispatched: [Command] = []

    override func setUp() {
        super.setUp()
        _ = NSApplication.shared
        let frame = NSRect(x: 0, y: 0, width: 960, height: 580)
        window = NSWindow(contentRect: frame, styleMask: [.titled], backing: .buffered, defer: true)
        window.isReleasedWhenClosed = false
        let root = NSView(frame: frame)
        window.contentView = root
        video = UpNextFocusTarget(frame: root.bounds)
        root.addSubview(video)
        panel = UpNextPanel()
        panel.frame = NSRect(x: 648, y: 90, width: 300, height: 440)
        root.addSubview(panel)
        panel.focusReturn = video
        dispatched = []
        panel.dispatch = { [unowned self] command in self.dispatched.append(command) }
        panel.show(UpNextList(playlist: upNextPlaylist(["a.mkv", "b.mkv", "c.mkv"], current: 1), mediaTitle: "B"))
        window.layoutIfNeeded()
    }

    override func tearDown() {
        window.close()
        super.tearDown()
    }

    private func press(_ keyCode: UInt16, _ characters: String, onRow row: Int) throws {
        panel.tableView.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        let event = try XCTUnwrap(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                                                   windowNumber: window.windowNumber, context: nil,
                                                   characters: characters, charactersIgnoringModifiers: characters,
                                                   isARepeat: false, keyCode: keyCode))
        panel.tableView.keyDown(with: event)
    }

    func testReturnPlaysTheSelectedRowAndDeleteRemovesIt() throws {
        XCTAssertEqual(panel.tableView.numberOfRows, 3)
        try press(UpNextKey.returnKeyCode, "\r", onRow: 2)
        try press(UpNextKey.deleteKeyCode, "\u{7f}", onRow: 0)
        XCTAssertEqual(dispatched, [.playlist(action: .jump(index: 2)), .playlist(action: .remove(index: 0))])
    }

    func testTheCurrentRowIsNeitherPlayedAgainNorRemoved() throws {
        try press(UpNextKey.returnKeyCode, "\r", onRow: 1)
        try press(UpNextKey.forwardDeleteKeyCode, "\u{F728}", onRow: 1)
        XCTAssertEqual(dispatched, [])
        let items = panel.menuItems(forRow: 1)
        XCTAssertEqual(items.map(\.title), ["Play", "Remove from Up Next"])
        XCTAssertEqual(items.map(\.isEnabled), [false, false])
    }

    func testTheRowMenuPlaysAndRemovesItsRow() {
        let items = panel.menuItems(forRow: 2)
        XCTAssertEqual(items.map(\.isEnabled), [true, true])
        for item in items { _ = NSApp.sendAction(item.action!, to: item.target, from: item) }
        XCTAssertEqual(dispatched, [.playlist(action: .jump(index: 2)), .playlist(action: .remove(index: 2))])
        XCTAssertEqual(panel.menuItems(forRow: 3), [], "no menu below the last row")
    }

    func testKeysTheListDoesNotHandleGoToTheVideo() throws {
        try press(49, " ", onRow: 0)
        try press(3, "f", onRow: 0)
        XCTAssertEqual(video.keys, [49, 3], "Space and F reach the video while the list has focus")
        XCTAssertEqual(dispatched, [])
    }

    func testAScrollOverThePanelScrollsItsListNotTheVolume() {
        let controls = TransportOverlay(hideTimer: ManualHideTimer())
        controls.frame = video.frame
        window.contentView?.addSubview(controls, positioned: .below, relativeTo: panel)
        controls.focusReturn = video
        XCTAssertFalse(controls.takesScroll(at: NSPoint(x: panel.frame.midX, y: panel.frame.midY)))
        XCTAssertTrue(controls.takesScroll(at: NSPoint(x: 200, y: 300)), "over the video it changes the volume")
        panel.isHidden = true
        XCTAssertTrue(controls.takesScroll(at: NSPoint(x: panel.frame.midX, y: panel.frame.midY)),
                      "a hidden panel leaves the scroll to the volume")
        controls.removeFromSuperview()
    }
}
