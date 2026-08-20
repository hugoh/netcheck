import AppKit

let size: CGFloat = 1024
let img = NSImage(size: NSSize(width: size, height: size))
img.lockFocus()

let rect = NSRect(x: 0, y: 0, width: size, height: size)
let bgPath = NSBezierPath(roundedRect: rect, xRadius: size * 0.22, yRadius: size * 0.22)
let gradient = NSGradient(colors: [
    NSColor(calibratedRed: 0.15, green: 0.55, blue: 0.90, alpha: 1),
    NSColor(calibratedRed: 0.04, green: 0.18, blue: 0.42, alpha: 1),
])
gradient?.draw(in: bgPath, angle: -90)

if let symbol = NSImage(systemSymbolName: "wifi", accessibilityDescription: nil) {
    let config = NSImage.SymbolConfiguration(pointSize: size * 0.5, weight: .bold)
        .applying(NSImage.SymbolConfiguration(paletteColors: [NSColor.white]))
    let configured = symbol.withSymbolConfiguration(config) ?? symbol
    let symSize = configured.size
    let symRect = NSRect(
        x: (size - symSize.width) / 2,
        y: (size - symSize.height) / 2 - size * 0.03,
        width: symSize.width,
        height: symSize.height
    )
    configured.draw(in: symRect, from: .zero, operation: .sourceOver, fraction: 1.0)
}

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
