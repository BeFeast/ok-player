import AppKit
import SwiftUI

// The Welcome view the player window shows while nothing is loaded: a first-run card,
// or the Continue watching shelf. It is a SwiftUI island over the canvas, bound to a
// `ContinueWatchingModel`; every colour is a palette role.

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

    static let cardWidth: CGFloat = 176

    var body: some View {
        Group {
            if model.isFirstRun {
                firstRun
                    .padding(20)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                shelf
                    .padding(.horizontal, 24)
                    .padding(.top, 34)
                    .frame(maxWidth: 744, maxHeight: .infinity, alignment: .topLeading)
                    .frame(maxWidth: .infinity)
            }
        }
    }

    private var dropHint: some View {
        Text("or drop a video on this window")
            .font(.callout)
            .foregroundStyle(PaletteRole.textMuted.swiftUI)
    }

    private var firstRun: some View {
        VStack(spacing: 10) {
            if let tile = BrandIcon.welcome {
                Image(nsImage: tile)
                    .resizable()
                    .frame(width: 48, height: 48)
                    .accessibilityHidden(true)
                    .padding(.bottom, 6)
            }
            (Text("OK").fontWeight(.bold) + Text(" Player").fontWeight(.regular))
                .font(.system(size: 30))
                .foregroundStyle(PaletteRole.text.swiftUI)
            Text("Open a file to start playing.")
                .foregroundStyle(PaletteRole.textMuted.swiftUI)
            Button("Open…") { model.openFile() }
                .buttonStyle(AccentButtonStyle())
                .padding(.top, 4)
            dropHint
            if let problem = model.problem {
                Text(problem)
                    .font(.caption)
                    .foregroundStyle(PaletteRole.danger.swiftUI)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 360)
            }
        }
    }

    private var shelf: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Continue watching")
                    .font(.title2.weight(.bold))
                    .foregroundStyle(PaletteRole.text.swiftUI)
                Text("Pick up where you left off — or open something new.")
                    .foregroundStyle(PaletteRole.textMuted.swiftUI)
            }
            // As many cards as the width takes, newest first.
            ViewThatFits(in: .horizontal) {
                cards(3)
                cards(2)
                cards(1)
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
                .frame(maxWidth: .infinity, minHeight: 58)
                .background(PaletteRole.surface.swiftUI, in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(PaletteRole.borderSubtle.swiftUI,
                                                                          style: StrokeStyle(dash: [4, 3])))
                .allowsHitTesting(false)
            }
            if model.privateSession {
                Text("Private session: nothing new is recorded.")
                    .font(.caption)
                    .foregroundStyle(PaletteRole.textMuted.swiftUI)
            }
        }
    }

    private func cards(_ count: Int) -> some View {
        HStack(alignment: .center, spacing: 14) {
            ForEach(model.entries.prefix(count)) { entry in
                ContinueWatchingCard(entry: entry, width: Self.cardWidth) { model.resume(entry) }
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
            .help("History")
        }
    }
}

/// One Continue watching card: a placeholder tile (no posters yet) with the state,
/// the watched bar, the title and when it was last opened. A click resumes it.
struct ContinueWatchingCard: View {
    let entry: HistoryEntry
    let width: CGFloat
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 6) {
                ZStack(alignment: .bottomTrailing) {
                    RoundedRectangle(cornerRadius: 8)
                        .fill(PaletteRole.surfaceRaised.swiftUI)
                        .overlay(RoundedRectangle(cornerRadius: 8).stroke(PaletteRole.borderSubtle.swiftUI))
                        .overlay(Image(systemName: "play.fill")
                            .font(.system(size: 22))
                            .foregroundStyle(PaletteRole.textMuted.swiftUI))
                    Text(historyStateText(entry))
                        .font(.caption.weight(.medium))
                        .foregroundStyle(PaletteRole.text.swiftUI)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Capsule().fill(PaletteRole.surface.swiftUI))
                        .padding(6)
                }
                .frame(width: width, height: width * 9 / 16)
                WatchedBar(fraction: entry.progress)
                Text(entry.title)
                    .font(.body.weight(.medium))
                    .foregroundStyle(PaletteRole.text.swiftUI)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Text(entry.whenLabel)
                    .font(.caption)
                    .foregroundStyle(PaletteRole.textMuted.swiftUI)
                    .lineLimit(1)
            }
            .frame(width: width, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help(entry.source.location)
    }
}
