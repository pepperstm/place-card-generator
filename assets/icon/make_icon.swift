// Draws the Place Cards app icon with CoreGraphics.
// Usage: swift make_icon.swift <pixels>=<out.png> [<pixels>=<out.png> ...]
// Each size is drawn directly as vectors (not downscaled), with fewer details at small sizes.
import AppKit
import CoreGraphics

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(red: CGFloat((hex >> 16) & 255) / 255, green: CGFloat((hex >> 8) & 255) / 255,
            blue: CGFloat(hex & 255) / 255, alpha: a)
}

func render(pixels: Int) -> Data {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    let ctx = NSGraphicsContext(bitmapImageRep: rep)!
    NSGraphicsContext.current = ctx
    let cg = ctx.cgContext
    cg.scaleBy(x: CGFloat(pixels) / 1024, y: CGFloat(pixels) / 1024)
    cg.setAllowsAntialiasing(true)
    cg.interpolationQuality = .high
    let space = CGColorSpaceCreateDeviceRGB()
    let small = pixels <= 64
    let tiny = pixels <= 32

    // --- macOS-style rounded square (824 px body inside the 1024 canvas)
    let body = CGRect(x: 100, y: 100, width: 824, height: 824)
    let squircle = NSBezierPath(roundedRect: body, xRadius: 186, yRadius: 186).cgPath
    cg.saveGState()
    cg.setShadow(offset: CGSize(width: 0, height: -14), blur: 30, color: rgb(0x000000, 0.40))
    cg.addPath(squircle); cg.setFillColor(rgb(0x123D30)); cg.fillPath()
    cg.restoreGState()

    cg.saveGState()
    cg.addPath(squircle); cg.clip()
    let bg = CGGradient(colorsSpace: space, colors: [rgb(0x34755D), rgb(0x0E3528)] as CFArray, locations: [0, 1])!
    cg.drawLinearGradient(bg, start: CGPoint(x: 300, y: 924), end: CGPoint(x: 700, y: 100), options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
    let glow = CGGradient(colorsSpace: space, colors: [rgb(0xFFFFFF, 0.16), rgb(0xFFFFFF, 0)] as CFArray, locations: [0, 1])!
    cg.drawRadialGradient(glow, startCenter: CGPoint(x: 360, y: 880), startRadius: 0,
                          endCenter: CGPoint(x: 360, y: 880), endRadius: 520, options: [])
    cg.restoreGState()
    // thin inner rim highlight
    cg.addPath(squircle); cg.setStrokeColor(rgb(0xFFFFFF, 0.14)); cg.setLineWidth(3); cg.strokePath()

    // centre the card group in the body and make it a little larger
    cg.saveGState()
    cg.translateBy(x: 0, y: 96)
    cg.translateBy(x: 515, y: 420); cg.scaleBy(x: 1.1, y: 1.1); cg.translateBy(x: -515, y: -420)
    // --- tent place card (fold along the top), seen at a slight 3/4 angle from the right
    let BL = CGPoint(x: 205, y: 262), BR = CGPoint(x: 738, y: 232)
    let TR = CGPoint(x: 704, y: 574), TL = CGPoint(x: 258, y: 566)
    func P(_ u: CGFloat, _ v: CGFloat) -> CGPoint {
        let x = (1 - u) * (1 - v) * BL.x + u * (1 - v) * BR.x + u * v * TR.x + (1 - u) * v * TL.x
        let y = (1 - u) * (1 - v) * BL.y + u * (1 - v) * BR.y + u * v * TR.y + (1 - u) * v * TL.y
        return CGPoint(x: x, y: y)
    }
    func poly(_ pts: [CGPoint]) -> CGPath {
        let p = CGMutablePath(); p.move(to: pts[0]); pts.dropFirst().forEach { p.addLine(to: $0) }; p.closeSubpath(); return p
    }
    func fillPoly(_ pts: [CGPoint], _ top: UInt32, _ bottom: UInt32, shadow: CGFloat = 0, round: CGFloat = 18) {
        let path = poly(pts)
        let box = path.boundingBox
        cg.saveGState()
        if shadow > 0 { cg.setShadow(offset: CGSize(width: 0, height: -shadow), blur: shadow * 1.8, color: rgb(0x000000, 0.45)) }
        cg.setLineJoin(.round); cg.setLineWidth(round)
        cg.addPath(path); cg.setFillColor(rgb(bottom)); cg.setStrokeColor(rgb(bottom)); cg.drawPath(using: .fillStroke)
        cg.restoreGState()
        cg.saveGState()
        cg.setLineJoin(.round); cg.setLineWidth(round)
        cg.addPath(path); cg.replacePathWithStrokedPath(); cg.addPath(path); cg.clip()
        let g = CGGradient(colorsSpace: space, colors: [rgb(top), rgb(bottom)] as CFArray, locations: [0, 1])!
        cg.drawLinearGradient(g, start: CGPoint(x: box.midX, y: box.maxY), end: CGPoint(x: box.midX, y: box.minY),
                              options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
        cg.restoreGState()
    }
    // inside of the far panel, seen through the open right end (shaded)
    fillPoly([BR, TR, CGPoint(x: 790, y: 596), CGPoint(x: 826, y: 262)], 0xD9CFB5, 0xB9AE92, shadow: 12)
    // top ridge strip where the card folds
    fillPoly([TL, TR, CGPoint(x: 790, y: 596), CGPoint(x: 330, y: 604)], 0xF3EDDD, 0xE3DAC2)
    // front face
    fillPoly([BL, BR, TR, TL], 0xFFFFFF, 0xEFE9DA, shadow: 22)
    // fold highlight along the ridge
    cg.setStrokeColor(rgb(0xFFFFFF, 0.95)); cg.setLineWidth(small ? 6 : 5); cg.setLineCap(.round)
    cg.move(to: TL); cg.addLine(to: TR); cg.strokePath()

    // --- name lines on the face (fewer at small sizes)
    let ink = rgb(0x1B3A2E)
    func bar(_ u0: CGFloat, _ u1: CGFloat, _ v0: CGFloat, _ v1: CGFloat) {
        cg.addPath(poly([P(u0, v0), P(u1, v0), P(u1, v1), P(u0, v1)]))
        cg.setFillColor(ink); cg.fillPath()
    }
    if tiny {
        bar(0.14, 0.80, 0.50, 0.74)
    } else if small {
        bar(0.12, 0.82, 0.56, 0.78)
        bar(0.12, 0.62, 0.30, 0.42)
    } else {
        bar(0.12, 0.80, 0.58, 0.76)
        bar(0.12, 0.60, 0.36, 0.46)
        bar(0.12, 0.46, 0.20, 0.28)
    }
    // --- the small allergy dot in the lower-right corner of the face
    if pixels > 16 {
        let c = P(0.88, small ? 0.22 : 0.17)
        let r: CGFloat = small ? 34 : 27
        cg.setFillColor(rgb(0x000000)); cg.fillEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: 2 * r, height: 2 * r))
    }
    cg.restoreGState()
    NSGraphicsContext.current = nil
    return rep.representation(using: .png, properties: [:])!
}

for arg in CommandLine.arguments.dropFirst() {
    let parts = arg.split(separator: "=", maxSplits: 1).map(String.init)
    guard parts.count == 2, let px = Int(parts[0]) else { fatalError("bad argument \(arg)") }
    try! render(pixels: px).write(to: URL(fileURLWithPath: parts[1]))
}
