import AppKit

@MainActor
extension PlayerApp {
    func showFeedback() {
        guard let window, window.attachedSheet == nil else { return }
        let port: UInt16
        let directory: String
        if verification?.scenarioID == "feedback-dialog" {
            guard let test = verification?.httpFixture, test.host == "127.0.0.1",
                  let rawPort = test.port, let loopbackPort = UInt16(exactly: rawPort), loopbackPort > 0,
                  let profile = verification?.profile else { return }
            port = loopbackPort
            directory = profile.path
        } else {
            port = 0
            directory = SessionConfig.forApp().configDir
        }
        let client = FeedbackClient(directory: directory, testPort: port)
        do {
            let panel = FeedbackPanel(app: self, client: client, form: try client.form())
            feedback = panel
            window.beginSheet(panel.window) { [weak self] _ in self?.feedback = nil }
        } catch {
            let alert = NSAlert()
            alert.messageText = "Feedback is unavailable"
            alert.informativeText = error.localizedDescription
            alert.beginSheetModal(for: window)
        }
    }

    func feedbackScreenshot() -> Data? {
        guard let root = window?.contentView, root.bounds.width > 0, root.bounds.height > 0 else { return nil }
        root.layoutSubtreeIfNeeded()
        // Cap pixel area so even a noisy RGBA frame fits the service's 8 MiB budget.
        let scale = min(2, sqrt(1_500_000 / (root.bounds.width * root.bounds.height)))
        var frame: CGImage?
        let status = client?.store.snapshot.status
        if let video, status == .playing || status == .paused {
            let width = Int((video.bounds.width * scale).rounded())
            let height = Int((video.bounds.height * scale).rounded())
            guard width > 0, height > 0,
                  let drawn = video.videoLayer.verificationFrame(width: width, height: height) else { return nil }
            frame = drawn.image
        }
        guard let image = windowComposite(root: root, video: video, frame: frame, scale: scale) else { return nil }
        return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])
    }
}

@MainActor
final class FeedbackPanel: NSObject, NSWindowDelegate {
    let window = NSPanel(contentRect: NSRect(x: 0, y: 0, width: 540, height: 460),
                         styleMask: [.titled], backing: .buffered, defer: false)
    let descriptionView = NSTextView()
    let captureButton = NSButton(title: "Capture Player Window", target: nil, action: nil)
    let sendButton = NSButton(title: "Send", target: nil, action: nil)
    private(set) var screenshot: Data?
    private(set) var issue: FeedbackIssue?

    private let reportKind = NSPopUpButton()
    private let name = NSTextField()
    private let includeDiagnostics = NSButton(checkboxWithTitle: "Include private diagnostics", target: nil, action: nil)
    private let remove = NSButton(title: "Remove Screenshot", target: nil, action: nil)
    private let thumbnail = NSImageView()
    private let status = NSTextField(wrappingLabelWithString: "")
    private let discard = NSButton(title: "Discard Saved Report", target: nil, action: nil)
    private let cancel = NSButton(title: "Close", target: nil, action: nil)
    private var sending = false
    private var token: String?
    private let client: FeedbackClient
    private unowned let app: PlayerApp
    private let metadata: FeedbackReport.Metadata
    private let diagnostics: FeedbackReport.Diagnostics

    init(app: PlayerApp, client: FeedbackClient, form: FeedbackForm) {
        self.app = app
        self.client = client
        token = form.draft?.token
        let bundle = Bundle.main
        #if arch(arm64)
        let architecture = "aarch64"
        #else
        let architecture = "x86_64"
        #endif
        let revision = bundle.object(forInfoDictionaryKey: "OKPSourceRevision") as? String ?? ""
        metadata = form.draft?.report.meta ?? .init(
            version: bundle.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown",
            build: bundle.object(forInfoDictionaryKey: "CFBundleVersion") as? String ?? "unknown",
            source: bundle.object(forInfoDictionaryKey: "OKPSourceSHA") as? String ?? "unknown",
            os: "macOS", osVersion: ProcessInfo.processInfo.operatingSystemVersionString,
            arch: architecture, channel: revision.contains("-beta") ? "beta" : "preview")
        let playback = app.client?.store.snapshot.status ?? .idle
        let size = app.window.contentView?.bounds.size ?? .zero
        diagnostics = form.draft?.report.diagnostics ?? .init(
            playback: playback == .opening ? "loading" : playback.rawValue,
            fullscreen: app.window.styleMask.contains(.fullScreen),
            width: Int(size.width), height: Int(size.height))
        super.init()

        window.title = "Send Feedback"
        window.appearance = app.window.effectiveAppearance
        window.delegate = self
        let root = NSView()
        window.contentView = root

        let scroll = NSScrollView()
        scroll.borderType = .bezelBorder
        scroll.hasVerticalScroller = true
        scroll.documentView = descriptionView
        descriptionView.isRichText = false
        descriptionView.setAccessibilityLabel("Description")
        descriptionView.font = .systemFont(ofSize: NSFont.systemFontSize)
        descriptionView.isVerticallyResizable = true
        descriptionView.autoresizingMask = [.width]
        descriptionView.frame.size = NSSize(width: 490, height: 112)
        descriptionView.textContainer?.widthTracksTextView = true
        window.initialFirstResponder = descriptionView

        reportKind.addItems(withTitles: ["Bug report", "Feature request"])
        reportKind.setAccessibilityLabel("Feedback type")
        name.placeholderString = "Name or GitHub (optional)"
        name.setAccessibilityLabel("Name or GitHub (optional)")
        includeDiagnostics.state = form.draft.map { $0.report.diagnostics == nil ? .off : .on } ?? .on
        descriptionView.string = form.draft?.report.text ?? ""
        name.stringValue = form.draft?.report.reporterName ?? form.reporterName
        reportKind.selectItem(at: form.draft?.report.kind == .feature ? 1 : 0)

        thumbnail.imageScaling = .scaleProportionallyDown
        thumbnail.setAccessibilityLabel("Attached player screenshot")
        thumbnail.widthAnchor.constraint(equalToConstant: 112).isActive = true
        thumbnail.heightAnchor.constraint(equalToConstant: 64).isActive = true
        setScreenshot(form.draft?.report.screenshot)
        let attachments = horizontal([captureButton, thumbnail, remove])

        let privacy = NSTextField(wrappingLabelWithString: form.privacy)
        privacy.textColor = .secondaryLabelColor
        status.stringValue = form.draft?.message ?? ""
        status.textColor = .secondaryLabelColor
        discard.isHidden = token == nil
        sendButton.title = token == nil ? "Send" : "Retry"
        cancel.keyEquivalent = "\u{1b}"
        sendButton.bezelStyle = .rounded
        sendButton.keyEquivalent = "\r"
        let buttons = horizontal([discard, cancel, sendButton])
        let body = NSStackView(views: [
            NSTextField(labelWithString: "Description"), scroll, reportKind, name, includeDiagnostics,
            attachments, privacy, status, buttons,
        ])
        body.orientation = .vertical
        body.alignment = .leading
        body.spacing = 10
        body.translatesAutoresizingMaskIntoConstraints = false
        root.addSubview(body)
        NSLayoutConstraint.activate([
            body.topAnchor.constraint(equalTo: root.topAnchor, constant: 18),
            body.leadingAnchor.constraint(equalTo: root.leadingAnchor, constant: 20),
            body.trailingAnchor.constraint(equalTo: root.trailingAnchor, constant: -20),
            body.bottomAnchor.constraint(lessThanOrEqualTo: root.bottomAnchor, constant: -16),
            scroll.widthAnchor.constraint(equalTo: body.widthAnchor),
            scroll.heightAnchor.constraint(equalToConstant: 114),
            name.widthAnchor.constraint(equalTo: body.widthAnchor),
            privacy.widthAnchor.constraint(equalTo: body.widthAnchor),
            status.widthAnchor.constraint(equalTo: body.widthAnchor),
        ])
        for (button, action) in [
            (captureButton, #selector(capture)), (remove, #selector(removeScreenshot)),
            (cancel, #selector(close)), (discard, #selector(discardDraft)), (sendButton, #selector(send)),
        ] {
            button.target = self
            button.action = action
        }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool { !sending }

    private func horizontal(_ views: [NSView]) -> NSStackView {
        let stack = NSStackView(views: views)
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 8
        return stack
    }

    private func setScreenshot(_ png: Data?) {
        screenshot = png
        thumbnail.image = png.flatMap { NSImage(data: $0) }
        thumbnail.isHidden = png == nil
        remove.isHidden = png == nil
    }

    @objc private func capture() {
        guard let png = app.feedbackScreenshot() else {
            status.stringValue = "The player window could not be captured."
            return
        }
        setScreenshot(png)
        status.stringValue = "Player window screenshot attached."
    }

    @objc private func removeScreenshot() { setScreenshot(nil) }

    @objc private func close() {
        guard !sending else { return }
        app.window.endSheet(window)
    }

    @objc private func discardDraft() {
        guard let token else { return }
        do {
            try client.discard(token)
            close()
        } catch {
            status.stringValue = error.localizedDescription
        }
    }

    @objc private func send() {
        guard !sending else { return }
        if let issue {
            NSWorkspace.shared.open(issue.issueUrl)
            return
        }
        let report = FeedbackReport(text: descriptionView.string,
                                    kind: reportKind.indexOfSelectedItem == 1 ? .feature : .bug,
                                    reporterName: name.stringValue, meta: metadata,
                                    diagnostics: includeDiagnostics.state == .on ? diagnostics : nil,
                                    screenshot: screenshot)
        let client = self.client
        let previous = token
        sending = true
        setControls(enabled: false)
        status.stringValue = "Sending…"
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let result = Result { try client.submit(report, token: previous) }
            DispatchQueue.main.async {
                self?.finished(result)
            }
        }
    }

    private func finished(_ result: Result<FeedbackIssue, Error>) {
        sending = false
        cancel.isEnabled = true
        sendButton.isEnabled = true
        switch result {
        case .success(let issue):
            self.issue = issue
            status.stringValue = "Feedback sent. Issue #\(issue.issueNumber)."
            discard.isHidden = true
            sendButton.title = "Open Issue"
            sendButton.keyEquivalent = ""
        case .failure(let error):
            if let saved = (error as? FeedbackClient.Failure)?.retryToken {
                token = saved
            }
            setControls(enabled: true)
            discard.isHidden = token == nil
            sendButton.title = token == nil ? "Send" : "Retry"
            status.stringValue = error.localizedDescription
        }
    }

    private func setControls(enabled: Bool) {
        descriptionView.isEditable = enabled
        [reportKind, name, includeDiagnostics, captureButton, remove, discard, cancel, sendButton]
            .forEach { $0.isEnabled = enabled }
    }
}
