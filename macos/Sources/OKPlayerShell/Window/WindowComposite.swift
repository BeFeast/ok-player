import AppKit

@MainActor
extension PlayerApp {
    /// Capture only our own rendered content, including the separate OpenGL plane.
    func windowComposite(root: NSView, video: VideoView?, frame: CGImage?, scale: CGFloat) -> CGImage? {
        let width = Int((root.bounds.width * scale).rounded())
        let height = Int((root.bounds.height * scale).rounded())
        guard width > 0, height > 0, let controller = windowBehaviour,
              let space = CGColorSpace(name: CGColorSpace.sRGB),
              let context = CGContext(data: nil, width: width, height: height, bitsPerComponent: 8,
                                      bytesPerRow: width * 4, space: space,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return nil }
        context.scaleBy(x: scale, y: scale)
        if miniPlayer.isActive {
            context.addPath(CGPath(roundedRect: root.bounds, cornerWidth: 14, cornerHeight: 14, transform: nil))
            context.clip()
        }
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
        PlayerCanvas.drawCanvas(in: root.bounds, appearance: root.effectiveAppearance)
        NSGraphicsContext.restoreGraphicsState()
        if let frame, let video, !video.isHiddenOrHasHiddenAncestor, !video.videoLayer.isHidden {
            context.draw(frame, in: video.convert(video.bounds, to: root))
        }
        // The title floats over the content. The side panel and toast sit above the OSC.
        var planes: [NSView] = [osc.scrim, osc.panel, miniPlayer.controls, music.view,
                                statusLayer.overlay, prompt,
                                history.welcomeView, controller.titleChrome.label]
        if !music.isActive { planes.append(upNext.panel) }
        planes += [osc.volumeCapsule, tracks.panel, statusLayer.toastView]
            + controller.trafficLights + [controller.pinButton]
        if let dropTarget = dropTarget { planes.append(dropTarget.highlight) }
        if commandPopover.isShown { planes.append(commandPopover) }
        for view in planes where !view.isHiddenOrHasHiddenAncestor && view.alphaValue > 0 && !view.bounds.isEmpty {
            let bounds = view.bounds
            guard let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(ceil(bounds.width * scale)),
                                               pixelsHigh: Int(ceil(bounds.height * scale)), bitsPerSample: 8,
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


@MainActor
extension VerifyRun {
    func windowComposite(root: NSView, video: VideoView, frame: CGImage?) -> CGImage? {
        app.windowComposite(root: root, video: video, frame: frame, scale: 2)
    }
}
