import AppKit
import SwiftUI

struct HistoryView: View {
    let model: HistoryPageModel
    let posters: HistoryPosters
    var onBounds: ([String: CGRect]) -> Void = { _ in }

    var body: some View {
        VStack(spacing: 0) {
            VStack(spacing: 0) {
                header
                if model.privateSession {
                    banner("Private session: new opens aren’t recorded. Your existing history is still here.",
                           symbol: "eye.slash", role: .info)
                }
                if let readFailure = model.readFailure {
                    banner(readFailure, symbol: "exclamationmark.triangle", role: .warning)
                }
                if let notice = model.notice {
                    banner(notice, symbol: "exclamationmark.circle", role: .danger)
                }
                Rectangle().fill(PaletteRole.borderSubtle.swiftUI).frame(height: 1)
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
            .frame(maxWidth: 740)
            .padding(.horizontal, 24)
            .padding(.top, 22)
            HistoryFooter(privateSession: model.privateSession, returnsToWelcome: true,
                          navigate: model.onBack, settings: model.onShowSettings)
        }
        .frame(minWidth: 0, maxWidth: .infinity, minHeight: 0, maxHeight: .infinity)
        .tint(PaletteRole.accent.swiftUI)
        .coordinateSpace(name: "history-surface")
        .onPreferenceChange(HistoryBoundsKey.self, perform: onBounds)
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 9) {
            HStack(spacing: 11) {
                Button(action: model.onBack) {
                    Image(systemName: "chevron.left")
                        .frame(width: 32, height: 42)
                        .background(PaletteRole.surfaceRaised.swiftUI, in: RoundedRectangle(cornerRadius: 8))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Back to Continue watching")
                .help("Back to Continue watching")
                .historyBounds("back")
                Text("History")
                    .font(.system(size: 30, weight: .semibold))
                Spacer(minLength: 4)
                Toggle("Private session", isOn: Binding(get: { model.privateSession },
                                                        set: { model.setPrivate($0) }))
                    .toggleStyle(.switch)
                    .controlSize(.small)
                    .help("While on, new activity is not recorded")
                    .fixedSize()
            }
            .foregroundStyle(PaletteRole.text.swiftUI)
            if !model.summary.isEmpty {
                Text(model.summary)
                    .font(.callout)
                    .foregroundStyle(PaletteRole.textMuted.swiftUI)
            }
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(PaletteRole.textMuted.swiftUI)
                    .accessibilityHidden(true)
                TextField("Search history", text: Binding(get: { model.search }, set: { model.setSearch($0) }))
                    .textFieldStyle(.plain)
                    .foregroundStyle(PaletteRole.text.swiftUI)
                    .historyBounds("search")
                if !model.search.isEmpty {
                    Button { model.setSearch("") } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(PaletteRole.textMuted.swiftUI)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Clear search")
                    .help("Clear search")
                    .historyBounds("clear")
                }
            }
            .padding(8)
            .background(PaletteRole.surface.swiftUI, in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(PaletteRole.border.swiftUI))
        }
        .padding(.bottom, 12)
    }

    private func banner(_ text: String, symbol: String, role: PaletteRole) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: symbol)
                .foregroundStyle(role.swiftUI)
            Text(text)
                .font(.callout)
                .foregroundStyle(PaletteRole.text.swiftUI)
                .fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 8).fill(PaletteRole.surfaceRaised.swiftUI))
        .padding(.bottom, 10)
    }

    @ViewBuilder private var content: some View {
        switch model.content {
        case .loading:
            ProgressView("Loading history…")
        case .failed(let message):
            HistoryStateCard(symbol: "exclamationmark.triangle", title: "Couldn’t read your history just now",
                             message: message, retry: { model.retry() })
        case .empty:
            HistoryStateCard(symbol: "clock.arrow.circlepath", title: "Nothing here yet",
                             message: "Files you open show up in History.")
        case .noMatch(let search):
            HistoryStateCard(symbol: "magnifyingglass", title: "No matches",
                             message: "Nothing in your history matches “\(search)”.")
        case .sections(let sections):
            rows(sections).id(model.search)
        }
    }

    private func rows(_ sections: [HistorySection]) -> some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                ForEach(sections, id: \.bucket) { section in
                    Text(section.header)
                        .font(.caption.weight(.semibold))
                        .tracking(1)
                        .foregroundStyle(PaletteRole.textMuted.swiftUI)
                        .padding(.horizontal, 12)
                        .padding(.top, 18)
                        .padding(.bottom, 6)
                        .accessibilityAddTraits(.isHeader)
                    ForEach(section.entries) { entry in
                        HistoryRow(entry: entry, model: model, posters: posters)
                            .historyBounds(entry.id)
                    }
                }
                Group {
                    if model.hasMore {
                        Button("Show more", action: model.loadMore)
                            .historyBounds("show-more")
                    } else if !model.endCap.isEmpty {
                        Text(model.endCap)
                            .font(.caption)
                            .foregroundStyle(PaletteRole.textMuted.swiftUI)
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
                .overlay(alignment: .top) {
                    Rectangle().fill(PaletteRole.borderSubtle.swiftUI).frame(height: 1)
                }
                .padding(.top, 18)
            }
        }
    }
}

/// A centred icon, title and message, with Retry for the error state.
struct HistoryStateCard: View {
    let symbol: String
    let title: String
    let message: String
    var retry: (() -> Void)?

    var body: some View {
        VStack(spacing: 10) {
            Image(systemName: symbol)
                .font(.system(size: 22))
                .foregroundStyle(PaletteRole.textMuted.swiftUI)
                .frame(width: 54, height: 54)
                .background(Circle().fill(PaletteRole.surfaceRaised.swiftUI))
                .accessibilityHidden(true)
            Text(title)
                .font(.headline)
                .foregroundStyle(PaletteRole.text.swiftUI)
            Text(message)
                .font(.callout)
                .foregroundStyle(PaletteRole.textMuted.swiftUI)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 340)
            if let retry {
                Button("Retry", action: retry)
                    .padding(.top, 4)
            }
        }
        .padding(24)
    }
}

struct HistoryRow: View {
    let entry: HistoryEntry
    let model: HistoryPageModel
    let posters: HistoryPosters

    var body: some View {
        HStack(spacing: 8) {
            Button { model.open(entry, .resume) } label: {
                HStack(spacing: 12) {
                    HistoryPoster(entry: entry, posters: posters, privateSession: model.privateSession,
                                  width: 64, height: 36)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(entry.title)
                            .font(.callout)
                            .foregroundStyle(PaletteRole.text.swiftUI)
                            .lineLimit(1)
                        Text(entry.location)
                            .font(.caption)
                            .foregroundStyle(PaletteRole.textMuted.swiftUI)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    Spacer(minLength: 4)
                    VStack(alignment: .trailing, spacing: 3) {
                        Text(entry.whenLabel)
                            .font(.caption)
                            .foregroundStyle(PaletteRole.textMuted.swiftUI)
                        Text(entry.isAvailable ? historyStateText(entry) : "Missing")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle((entry.state == .progress && entry.isAvailable
                                               ? PaletteRole.link : PaletteRole.textMuted).swiftUI)
                    }
                    .lineLimit(1)
                    .fixedSize(horizontal: true, vertical: false)
                }
                .contentShape(Rectangle())
            }
            .disabled(!entry.isAvailable)
            .buttonStyle(.plain)
            .help(entry.source.location)
            Menu {
                actions
            } label: {
                Image(systemName: "ellipsis")
                    .rotationEffect(.degrees(90))
                    .foregroundStyle(PaletteRole.text.swiftUI)
            }
            .menuStyle(.button)
            .buttonStyle(.borderless)
            .menuIndicator(.hidden)
            .fixedSize()
            .accessibilityLabel("Actions for \(entry.title)")
            .help("History actions")
            .historyBounds("actions:\(entry.id)")
        }
        .padding(.horizontal, 12)
        .frame(height: 64)
        .contextMenu { actions }
    }

    @ViewBuilder private var actions: some View {
        Button("Resume") { model.open(entry, .resume) }
            .disabled(!entry.isAvailable)
        Button("Play from Start") { model.open(entry, .fromStart) }
            .disabled(!entry.isAvailable)
        Divider()
        Button("Remove from History") { model.remove(entry) }
        Button("Reveal in Finder") { model.reveal(entry) }
            .disabled(!model.canReveal(entry))
    }
}
