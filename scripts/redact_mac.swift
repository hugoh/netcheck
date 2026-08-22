import AppKit
import CoreImage
import Vision

// Blurs any text region in a screenshot that looks like it contains an
// EUI-64-derived link-local IPv6 address (e.g. fe80::94db:fff:fe9d:6294) —
// the "ff:fe" (or "fff:fe") marker in the interface identifier embeds the
// underlying interface's real MAC address, which link-local/utun addresses
// otherwise don't leak.

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
	FileHandle.standardError.write(Data("usage: redact_mac.swift <path> [outputPath]\n".utf8))
	exit(1)
}
let inputPath = arguments[1]
let outputPath = arguments.count >= 3 ? arguments[2] : arguments[1]

guard let nsImage = NSImage(contentsOfFile: inputPath),
	let cgImage = nsImage.cgImage(forProposedRect: nil, context: nil, hints: nil)
else {
	FileHandle.standardError.write(Data("error: could not load image at \(inputPath)\n".utf8))
	exit(1)
}

let width = CGFloat(cgImage.width)
let height = CGFloat(cgImage.height)

let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false

let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
try handler.perform([request])

let markerPattern = try! NSRegularExpression(pattern: "ff:?fe", options: .caseInsensitive)

var boxes: [CGRect] = []
for observation in request.results ?? [] {
	guard let candidate = observation.topCandidates(1).first else { continue }
	let text = candidate.string
	let range = NSRange(text.startIndex..., in: text)
	guard markerPattern.firstMatch(in: text, options: [], range: range) != nil else { continue }

	// boundingBox is normalized with origin at bottom-left, matching CIImage's
	// coordinate space, so no y-flip is needed before using it directly.
	let bb = observation.boundingBox
	boxes.append(CGRect(x: bb.origin.x * width, y: bb.origin.y * height, width: bb.width * width, height: bb.height * height))
	print("redacting: \(text)")
}

guard !boxes.isEmpty else {
	print("no matches found in \(inputPath), leaving unchanged")
	if outputPath != inputPath {
		try FileManager.default.copyItem(atPath: inputPath, toPath: outputPath)
	}
	exit(0)
}

let sourceImage = CIImage(cgImage: cgImage)
var composited = sourceImage

for box in boxes {
	let padded = box.insetBy(dx: -6, dy: -6).intersection(sourceImage.extent)
	guard let blurFilter = CIFilter(name: "CIGaussianBlur") else { continue }
	blurFilter.setValue(sourceImage.clampedToExtent(), forKey: kCIInputImageKey)
	blurFilter.setValue(20.0, forKey: kCIInputRadiusKey)
	guard let blurred = blurFilter.outputImage?.cropped(to: padded) else { continue }
	composited = blurred.composited(over: composited)
}

let context = CIContext()
guard let outputCGImage = context.createCGImage(composited, from: sourceImage.extent) else {
	FileHandle.standardError.write(Data("error: could not render redacted image\n".utf8))
	exit(1)
}

let rep = NSBitmapImageRep(cgImage: outputCGImage)
guard let pngData = rep.representation(using: .png, properties: [:]) else {
	FileHandle.standardError.write(Data("error: could not encode PNG\n".utf8))
	exit(1)
}

try pngData.write(to: URL(fileURLWithPath: outputPath))
print("wrote \(outputPath)")
