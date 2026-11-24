import AppKit
import SwiftUI

// The populated Welcome view is bound to the session's Continue watching shelf.

/// The palette's accent push button, as `AccentButton` draws it in AppKit.
struct AccentButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: NSFont.systemFontSize, weight: .semibold))
            .foregroundStyle(PaletteRole.onAccent.swiftUI)
            .padding(.horizontal, 16)
            .frame(height: 28)
            .background(RoundedRectangle(cornerRadius: 6).fill(PaletteRole.accent.swiftUI))
            .overlay(RoundedRectangle(cornerRadius: 6).fill(Color.black.opacity(configuration.isPressed ? 0.18 : 0)))
            .contentShape(Rectangle())
    }
}

/// A thin watched-fraction bar.
struct WatchedBar: View {
    let fraction: Double

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(PaletteRole.borderSubtle.swiftUI)
                Capsule().fill(PaletteRole.accent.swiftUI)
                    .frame(width: proxy.size.width * CGFloat(min(max(fraction, 0), 1)))
            }
        }
        .frame(height: 3)
    }
}

/// The history state as one short label: "✓ Finished", "30m left", "2m in · 4%".
func historyStateText(_ entry: HistoryEntry) -> String {
    entry.state == .finished ? "✓ Finished" : entry.stateLabel
}

struct WelcomeView: View {
    let model: ContinueWatchingModel
    let posters: HistoryPosters
    var onBounds: ([String: CGRect]) -> Void = { _ in }

    static let cardWidth: CGFloat = 194

    var body: some View {
        ScrollView {
            shelf
                .frame(maxWidth: 680, alignment: .leading)
                .padding(.horizontal, 24)
                .padding(.top, 27)
                .padding(.bottom, 12)
                .frame(maxWidth: .infinity)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            HistoryFooter(privateSession: model.privateSession, returnsToWelcome: false,
                          navigate: model.showHistory, settings: model.showSettings)
        }
        .coordinateSpace(name: "history-surface")
        .onPreferenceChange(HistoryBoundsKey.self, perform: onBounds)
    }

    private var shelf: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Continue watching")
                    .font(.system(size: 30, weight: .semibold))
                    .foregroundStyle(PaletteRole.text.swiftUI)
                Text("Pick up where you left off — or open something new.")
                    .foregroundStyle(PaletteRole.textMuted.swiftUI)
            }
            // As many cards as the width takes, newest first.
            ViewThatFits(in: .horizontal) {
                cards(3, width: Self.cardWidth)
                cards(2, width: Self.cardWidth)
                cards(1, width: Self.cardWidth)
                cards(1, width: 134)
            }
            HStack(spacing: 14) {
                Button { model.openFile() } label: {
                    Label("Open File…", systemImage: "folder")
                }
                .buttonStyle(AccentButtonStyle())
                VStack(spacing: 6) {
                    Image(systemName: "tray.and.arrow.down")
                        .font(.system(size: 18))
                        .accessibilityHidden(true)
                    Text("Drop a video or folder here")
                        .font(.callout)
                }
                .foregroundStyle(PaletteRole.textMuted.swiftUI)
                .frame(maxWidth: .infinity, minHeight: 72)
                .background((model.dragActive ? PaletteRole.selected : PaletteRole.surface).swiftUI.opacity(0.55),
                            in: RoundedRectangle(cornerRadius: 10))
                .overlay {
                    RoundedRectangle(cornerRadius: 10)
                        .strokeBorder((model.dragActive ? PaletteRole.link : PaletteRole.border).swiftUI,
                                      style: StrokeStyle(lineWidth: model.dragActive ? 2 : 1,
                                                         dash: model.dragActive ? [] : [3, 2]))
                }
                .allowsHitTesting(false)
            }
        }
    }

    private func cards(_ count: Int, width: CGFloat) -> some View {
        HStack(alignment: .center, spacing: 14) {
            ForEach(model.entries.prefix(count)) { entry in
                ContinueWatchingCard(entry: entry, posters: posters, privateSession: model.privateSession,
                                     width: width) { model.resume(entry) }
                    .historyBounds(entry.id)
            }
            Button { model.showHistory() } label: {
                Image(systemName: "chevron.right")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(PaletteRole.text.swiftUI)
                    .frame(width: 36, height: 36)
                    .background(Circle().fill(PaletteRole.surfaceRaised.swiftUI))
                    .overlay(Circle().stroke(PaletteRole.borderSubtle.swiftUI))
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Open History")
            .help("History")
            .historyBounds("shelf-arrow")
        }
    }
}

/// One Continue watching card. A click resumes it through the shared session.
struct ContinueWatchingCard: View {
    let entry: HistoryEntry
    let posters: HistoryPosters
    let privateSession: Bool
    let width: CGFloat
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 4) {
                HistoryPoster(entry: entry, posters: posters, privateSession: privateSession,
                              width: width, height: width * 9 / 16)
                    .overlay(alignment: .bottomTrailing) {
                        Text(historyStateText(entry))
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(.black.opacity(0.68), in: RoundedRectangle(cornerRadius: 4))
                            .padding(6)
                    }
                Text(entry.title)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(PaletteRole.text.swiftUI)
                    .lineLimit(1)
                Text(entry.location)
                    .font(.caption)
                    .foregroundStyle(PaletteRole.textMuted.swiftUI)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .frame(width: width, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(entry.source.location)
    }
}
