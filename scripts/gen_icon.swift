import AppKit

let size: CGFloat = 1024
let img = NSImage(size: NSSize(width: size, height: size))
img.lockFocus()

let rect = NSRect(x: 0, y: 0, width: size, height: size)
let bgPath = NSBezierPath(roundedRect: rect, xRadius: size * 0.22, yRadius: size * 0.22)
let gradient = NSGradient(colors: [
    NSColor(calibratedRed: 0.09, green: 0.63, blue: 0.55, alpha: 1),
    NSColor(calibratedRed: 0.02, green: 0.20, blue: 0.17, alpha: 1),
])
gradient?.draw(in: bgPath, angle: -90)

// Mark: a wifi signal's concentric arcs radiating from the checkmark's
// short tip instead of the usual dot — "network" and "all clear" in one
// shape. Geometry is authored in a 0-1 local space (vertex bottom-center,
// long leg reaching up-right, short leg's tip up-left acting as the signal
// source, arcs continuing outward past that tip so no stroke crosses
// another) then scaled/centered onto the canvas below.
let vertex = CGPoint(x: 0.38, y: 0.36)
let shortLegEnd = CGPoint(x: 0.24, y: 0.50)
let longLegEnd = CGPoint(x: 0.70, y: 0.74)
let arcCenter = shortLegEnd
let arcRadii: [CGFloat] = [0.15, 0.28, 0.41]
let arcStartAngle: CGFloat = 95
let arcEndAngle: CGFloat = 175
let arcLineWidthFraction: CGFloat = 0.075

let bboxMinX: CGFloat = -0.21
let bboxMaxX: CGFloat = 0.70
let bboxMinY: CGFloat = 0.36
let bboxMaxY: CGFloat = 0.95
let bboxCenter = CGPoint(x: (bboxMinX + bboxMaxX) / 2, y: (bboxMinY + bboxMaxY) / 2)
let scale = size * 0.60 / max(bboxMaxX - bboxMinX, bboxMaxY - bboxMinY)
let verticalNudge = -size * 0.01

func pt(_ p: CGPoint) -> NSPoint {
    NSPoint(
        x: size / 2 + (p.x - bboxCenter.x) * scale,
        y: size / 2 + (p.y - bboxCenter.y) * scale + verticalNudge
    )
}

let white = NSColor.white
let markVertex = pt(vertex)
let markArcCenter = pt(arcCenter)

let arcsPath = NSBezierPath()
arcsPath.lineCapStyle = .round
arcsPath.lineWidth = arcLineWidthFraction * scale
for (i, r) in arcRadii.enumerated() {
    let arc = NSBezierPath()
    arc.appendArc(
        withCenter: markArcCenter,
        radius: r * scale,
        startAngle: arcStartAngle,
        endAngle: arcEndAngle,
        clockwise: false
    )
    white.withAlphaComponent(1.0 - CGFloat(i) * 0.3).setStroke()
    arc.lineCapStyle = .round
    arc.lineWidth = arcLineWidthFraction * scale
    arc.stroke()
}

let checkPath = NSBezierPath()
checkPath.move(to: pt(shortLegEnd))
checkPath.line(to: markVertex)
checkPath.line(to: pt(longLegEnd))
checkPath.lineCapStyle = .round
checkPath.lineJoinStyle = .round
checkPath.lineWidth = 0.09 * scale
white.setStroke()
checkPath.stroke()

img.unlockFocus()

guard let tiff = img.tiffRepresentation,
      let rep = NSBitmapImageRep(data: tiff),
      let png = rep.representation(using: .png, properties: [:])
else {
    FileHandle.standardError.write("failed to render icon\n".data(using: .utf8)!)
    exit(1)
}

let outPath = CommandLine.arguments[1]
try! png.write(to: URL(fileURLWithPath: outPath))
