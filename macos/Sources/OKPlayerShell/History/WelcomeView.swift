import AppKit
import SwiftUI

// The Welcome view the player window shows while nothing is loaded: a first-run card,
// or the Continue watching shelf. It is a SwiftUI island over the canvas, bound to a
// `ContinueWatchingModel`; every colour is a palette role.

extension PaletteRole {
    /// The role for SwiftUI, following the appearance it is drawn in.
    var swiftUI: Color { Color(nsColor: color) }
}

/// The palette's accent push button, as `AccentButton` draws it in AppKit.
struct AccentButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.system(size: NSFont.systemFontSize))
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
            } else {
                shelf
            }
        }
        .padding(20)
    }

    private var dropHint: some View {
        Text("or drop a video on this window")
            .font(.callout)
            .foregroundStyle(PaletteRole.textMuted.swiftUI)
    }

    private var firstRun: some View {
        VStack(spacing: 10) {
            Text("Welcome to OK Player")
                .font(.title2.weight(.semibold))
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
                    .font(.title2.weight(.semibold))
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
            HStack(spacing: 12) {
                Button("Open…") { model.openFile() }
                    .buttonStyle(AccentButtonStyle())
                dropHint
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
