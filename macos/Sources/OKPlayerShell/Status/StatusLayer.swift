import AppKit

/// The status states over the video and the toast, wired to the session. After
/// each drained batch, and after the shell opens a source, it asks the session for
/// the status surface (`query`) and shows it; it toasts the batch's notices and the
/// errors no card shows. Nothing here decides a state: the session classifies the
/// failure and words every line.
final class StatusLayer {
    let overlay = StatusOverlay()
    let toastView = ToastView()
    let toast: Toast
    /// The session's status surface; `nil` without a session.
    var query: (() -> StatusSurface?)?
    /// Retry: open the card's source again.
    var onRetry: ((MediaSource) -> Void)?
    /// Open another: let the user choose a source.
    var onOpenAnother: (() -> Void)?
    /// Copy details: put the card's detail on the pasteboard.
    var copy: (String) -> Void = { text in
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }
    /// Called with every toast line that shows.
    var onToast: ((String) -> Void)?

    private(set) var surface = StatusSurface.clear
    private(set) var shellFailure: String?
    /// Whether the next accepted open clears `shellFailure`.
    private(set) var shellFailureClearsOnOpen = false
    var display: StatusDisplay { overlay.display }

    init(timer: HideTimer = MainQueueHideTimer()) {
        toast = Toast(timer: timer)
        toast.onChange = { [weak self] text in
            self?.toastView.show(text)
            if let text { self?.onToast?(text) }
        }
        overlay.onAction = { [weak self] action, card in self?.perform(action, card) }
    }

    /// Put the overlay right above the video and the toast above everything, near
    /// the top of the window under the title bar.
    func install(in root: NSView, above video: NSView) {
        overlay.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(overlay, positioned: .above, relativeTo: video)
        toastView.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(toastView)
        NSLayoutConstraint.activate([
            overlay.topAnchor.constraint(equalTo: root.topAnchor),
            overlay.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            overlay.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            overlay.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            toastView.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor, constant: 12),
            toastView.centerXAnchor.constraint(equalTo: root.centerXAnchor),
            toastView.leadingAnchor.constraint(greaterThanOrEqualTo: root.leadingAnchor, constant: 16),
        ])
    }

    /// The session reported a batch.
    func sessionChanged(_ batch: EventBatch) {
        refresh()
        for text in StatusRules.toasts(in: batch) { toast.show(text) }
    }

    /// Ask the session for the surface again and show it.
    func refresh() {
        if let surface = query?() { self.surface = surface }
        overlay.show(StatusRules.display(surface, shellFailure: shellFailure))
    }

    /// The shell itself failed. A failure that `clearsOnOpen` (a frame that did not
    /// render, after which later frames may) stays until the next accepted open.
    /// Any other (no renderer, no session, the engine quit) stays for good: nothing
    /// replaces it, since nothing can be retried. Returns whether the card shows
    /// `message`.
    @discardableResult
    func showShellFailure(_ message: String, clearsOnOpen: Bool = false) -> Bool {
        if shellFailure != nil, !shellFailureClearsOnOpen { return false }
        shellFailure = message
        shellFailureClearsOnOpen = clearsOnOpen
        overlay.show(StatusRules.display(surface, shellFailure: message))
        return true
    }

    /// The shell asked the session to open a source and got `result`. A rejection
    /// shows as a toast; any other answer clears a shell failure that the next open
    /// clears, so the session's surface for the new source shows.
    func opened(_ result: DispatchResult) {
        if case .rejected(_, let message) = result {
            toast.show(message)
        } else if shellFailureClearsOnOpen {
            shellFailure = nil
            shellFailureClearsOnOpen = false
        }
        refresh()
    }

    /// Show a short notice. The one way for any part of the shell to toast.
    func show(toast text: String) {
        toast.show(text)
    }

    private func perform(_ action: FailureAction, _ card: FailureCard) {
        switch action {
        case .retry: onRetry?(card.source)
        case .openAnother: onOpenAnother?()
        case .copyDetails:
            copy(card.detail)
            toast.show("Copied details")
        }
    }
}
