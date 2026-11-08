import AppKit
import XCTest
@testable import OKPlayerShell

/// The shell's chrome colours come from the generated palette table and follow the
/// appearance they are drawn in.
final class PaletteTests: XCTestCase {
    /// The 0xRRGGBB value `color` resolves to when drawn in the named appearance.
    private func resolved(_ color: NSColor, in name: NSAppearance.Name) throws -> UInt32 {
        let appearance = try XCTUnwrap(NSAppearance(named: name))
        var converted: NSColor?
        appearance.performAsCurrentDrawingAppearance { converted = color.usingColorSpace(.sRGB) }
        let srgb = try XCTUnwrap(converted, "\(color) has no sRGB form")
        let channel = { (value: CGFloat) in UInt32((value * 255).rounded()) }
        return channel(srgb.redComponent) << 16 | channel(srgb.greenComponent) << 8 | channel(srgb.blueComponent)
    }

    func testEveryRoleResolvesToItsLightValueInAquaAndItsDarkValueInDarkAqua() throws {
        // Without a role whose two values differ, the dark half would prove nothing.
        XCTAssertTrue(PaletteRole.allCases.contains { $0.srgb.light != $0.srgb.dark })
        for role in PaletteRole.allCases {
            XCTAssertEqual(try resolved(role.color, in: .aqua), role.srgb.light, "\(role) in light")
            XCTAssertEqual(try resolved(role.color, in: .darkAqua), role.srgb.dark, "\(role) in dark")
        }
    }

    func testSRGBValueSplitsIntoRedGreenAndBlueChannels() throws {
        let color = try XCTUnwrap(NSColor.fromSRGB(0x15A89D).usingColorSpace(.sRGB))
        XCTAssertEqual(color.redComponent * 255, 0x15, accuracy: 0.001)
        XCTAssertEqual(color.greenComponent * 255, 0xA8, accuracy: 0.001)
        XCTAssertEqual(color.blueComponent * 255, 0x9D, accuracy: 0.001)
        XCTAssertEqual(color.alphaComponent, 1)
    }
}
