import SwiftUI

struct HistoryFooter: View {
    let privateSession: Bool
    let returnsToWelcome: Bool
    let navigate: () -> Void
    let settings: () -> Void

    var body: some View {
        ZStack {
            Text(privateSession ? "Private session" : "Recording history")
                .font(.caption)
                .foregroundStyle((privateSession ? PaletteRole.textMuted : PaletteRole.link).swiftUI)
            HStack {
                Button(action: navigate) {
                    Label(returnsToWelcome ? "Continue watching" : "History",
                          systemImage: returnsToWelcome ? "chevron.left" : "clock.arrow.circlepath")
                }
                .historyBounds("footer-navigation")
                Spacer()
                Button(action: settings) {
                    Image(systemName: "gearshape")
                        .frame(width: 24, height: 24)
                }
                .accessibilityLabel("Settings")
                .help("Settings")
            }
            .buttonStyle(.plain)
            .foregroundStyle(PaletteRole.textMuted.swiftUI)
            .padding(.horizontal, 24)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 42)
        .overlay(alignment: .top) {
            Rectangle().fill(PaletteRole.borderSubtle.swiftUI).frame(height: 1)
        }
        .historyBounds("footer")
    }
}

// AppKit's hosted verifier uses the laid-out bounds to send ordinary mouse
// events to live SwiftUI controls and to compare geometry between checkpoints.
struct HistoryBoundsKey: PreferenceKey {
    static let defaultValue: [String: CGRect] = [:]

    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}

extension View {
    func historyBounds(_ id: String) -> some View {
        background {
            GeometryReader { proxy in
                Color.clear.preference(key: HistoryBoundsKey.self,
                                       value: [id: proxy.frame(in: .named("history-surface"))])
            }
            .allowsHitTesting(false)
        }
    }
}
