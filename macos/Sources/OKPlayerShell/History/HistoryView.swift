import AppKit
import SwiftUI

// The History window's content: search, the private-session switch and banner, and
// the session's day groups with the row actions, or the loading, error, empty and
// no-match states. A SwiftUI island bound to a `HistoryPageModel`.

struct HistoryView: View {
    let model: HistoryPageModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
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
            Divider()
            content
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(minWidth: 460, idealWidth: 640, maxWidth: .infinity, minHeight: 360, idealHeight: 560, maxHeight: .infinity)
        .background(PaletteRole.canvas.swiftUI)
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("History")
                        .font(.title2.weight(.semibold))
                        .foregroundStyle(PaletteRole.text.swiftUI)
                    if !model.summary.isEmpty {
                        Text(model.summary)
                            .font(.callout)
                            .foregroundStyle(PaletteRole.textMuted.swiftUI)
                    }
                }
                Spacer(minLength: 8)
                Toggle("Private session", isOn: Binding(get: { model.privateSession },
                                                        set: { model.setPrivate($0) }))
                    .toggleStyle(.switch)
                    .help("While on, nothing new is recorded and nothing remembered is applied")
            }
            TextField("Search history", text: Binding(get: { model.search }, set: { model.setSearch($0) }))
                .textFieldStyle(.roundedBorder)
        }
        .padding(16)
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
        .padding(.horizontal, 16)
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
            list(sections)
        }
    }

    private func list(_ sections: [HistorySection]) -> some View {
        List {
            ForEach(sections, id: \.bucket) { section in
                Section(section.header) {
                    ForEach(section.entries) { entry in
                        HistoryRow(entry: entry, model: model)
                    }
                }
            }
            if !model.endCap.isEmpty {
                Text(model.endCap)
                    .font(.caption)
                    .foregroundStyle(PaletteRole.textMuted.swiftUI)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 6)
            }
        }
        .listStyle(.inset)
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

/// One remembered entry. A click resumes it; the menu and the context menu hold
/// Resume, Play from Start, Remove from History and Reveal in Finder.
struct HistoryRow: View {
    let entry: HistoryEntry
    let model: HistoryPageModel

    var body: some View {
        HStack(spacing: 10) {
            Button { model.open(entry, .resume) } label: {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(entry.title)
                            .foregroundStyle(PaletteRole.text.swiftUI)
                            .lineLimit(1)
                            .truncationMode(.tail)
                        Text(entry.location)
                            .font(.caption)
                            .foregroundStyle(PaletteRole.textMuted.swiftUI)
                            .lineLimit(1)
                            .truncationMode(.middle)
                        if entry.state == .progress {
                            WatchedBar(fraction: entry.progress)
                                .frame(maxWidth: 180)
                        }
                    }
                    Spacer(minLength: 8)
                    VStack(alignment: .trailing, spacing: 3) {
                        Text(entry.whenLabel)
                            .font(.caption)
                            .foregroundStyle(PaletteRole.textMuted.swiftUI)
                        Text(historyStateText(entry))
                            .font(.caption.weight(.medium))
                            .foregroundStyle(entry.state == .finished ? PaletteRole.success.swiftUI
                                                                       : PaletteRole.text.swiftUI)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(entry.source.location)
            Menu {
                actions
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.button)
            .buttonStyle(.borderless)
            .menuIndicator(.hidden)
            .fixedSize()
            .help("History actions")
        }
        .padding(.vertical, 4)
        .contextMenu { actions }
    }

    @ViewBuilder private var actions: some View {
        Button("Resume") { model.open(entry, .resume) }
        Button("Play from Start") { model.open(entry, .fromStart) }
        Divider()
        Button("Remove from History") { model.remove(entry) }
        Button("Reveal in Finder") { model.reveal(entry) }
            .disabled(!model.canReveal(entry))
    }
}
