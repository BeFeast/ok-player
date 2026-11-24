import AppKit

@MainActor
extension VerifyRun {
    /// Reproduce the live plane order when the hosted WindowServer cannot supply a 2× capture.
    func windowComposite(root: NSView, video: VideoView, frame: CGImage?) -> CGImage? {
        let width = Int((root.bounds.width * 2).rounded())
        let height = Int((root.bounds.height * 2).rounded())
        guard width > 0, height > 0, let controller = app.windowBehaviour,
              let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                                      bytesPerRow: width * 4, space: space,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        context.scaleBy(x: 2, y: 2)
        if app.miniPlayer.isActive {
            context.addPath(CGPath(roundedRect: root.bounds, cornerWidth: 14, cornerHeight: 14, transform: nil))
            context.clip()
        }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
        PlayerCanvas.drawCanvas(in: root.bounds, appearance: root.effectiveAppearance)
        NSGraphicsContext.restoreGraphicsState()
        if let frame, !video.isHiddenOrHasHiddenAncestor, !video.videoLayer.isHidden {
            context.draw(frame, in: video.convert(video.bounds, to: root))
        }
        // The title floats over the content. The side panel and toast sit above the OSC.
        var planes: [NSView] = [app.osc.scrim, app.osc.panel, app.miniPlayer.controls,
                                app.statusLayer.overlay, app.prompt,
                                app.history.welcomeView, controller.titleChrome.label, app.upNext.panel,
                                app.osc.volumeCapsule, app.tracks.panel, app.statusLayer.toastView] + controller.trafficLights + [controller.pinButton]
        if let dropTarget = app.dropTarget { planes.append(dropTarget.highlight) }
        if app.commandPopover.isShown { planes.append(app.commandPopover) }
        for view in planes where !view.isHiddenOrHasHiddenAncestor && view.alphaValue > 0 && !view.bounds.isEmpty {
            let bounds = view.bounds
            guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(ceil(bounds.width * 2)),
                                               pixelsHigh: Int(ceil(bounds.height * 2)), bitsPerSample: 8,
                                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else { return nil }
            bitmap.size = bounds.size
            view.effectiveAppearance.performAsCurrentDrawingAppearance { view.cacheDisplay(in: bounds, to: bitmap) }
            guard let image = bitmap.cgImage else { return nil }
            context.saveGState()
            context.setAlpha(view.alphaValue)
            context.draw(image, in: view.convert(bounds, to: root))
            context.restoreGState()
        }
        return context.makeImage()
    }
}
