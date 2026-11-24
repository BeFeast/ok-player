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
                HistoryButton("footer-navigation", action: navigate) {
                    Label(returnsToWelcome ? "Continue watching" : "History",
                          systemImage: returnsToWelcome ? "chevron.left" : "clock.arrow.circlepath")
                }
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

/// Exposes a mounted SwiftUI control's action and geometry to the hosted verifier.
struct HistoryControl: Equatable {
    let frame: CGRect
    var activate: (() -> Void)?

    static func == (left: Self, right: Self) -> Bool {
        left.frame == right.frame && (left.activate == nil) == (right.activate == nil)
    }
}

struct HistoryBoundsKey: PreferenceKey {
    static let defaultValue: [String: HistoryControl] = [:]

    static func reduce(value: inout [String: HistoryControl], nextValue: () -> [String: HistoryControl]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}

struct HistoryButton<Label: View>: View {
    let id: String
    let action: () -> Void
    let label: () -> Label

    init(_ id: String, action: @escaping () -> Void, @ViewBuilder label: @escaping () -> Label) {
        self.id = id
        self.action = action
        self.label = label
    }

    var body: some View {
        Button(action: action, label: label)
            .historyBounds(id, action: action)
    }
}

extension View {
    func historyBounds(_ id: String, action: (() -> Void)? = nil) -> some View {
        background {
            GeometryReader { proxy in
                Color.clear.preference(key: HistoryBoundsKey.self,
                                       value: [id: HistoryControl(frame: proxy.frame(in: .named("history-surface")),
                                                                  activate: action)])
            }
            .allowsHitTesting(false)
        }
    }
}
