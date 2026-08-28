import Foundation
@testable import NetCheckApp
import Testing

/// One entry of testdata/age-format-cases.json — the now/seconds/minutes/
/// hours threshold cases for `updatedAgoText`.
private struct AgeFormatCase: Decodable {
    let seconds: TimeInterval
    let magnitude: Int
    let unit: String

    var expectedText: String {
        switch unit {
        case "now": "Updated just now"
        case "s": "Updated \(magnitude) second\(magnitude == 1 ? "" : "s") ago"
        case "m": "Updated \(magnitude) minute\(magnitude == 1 ? "" : "s") ago"
        case "h": "Updated \(magnitude) hour\(magnitude == 1 ? "" : "s") ago"
        default: fatalError("unknown unit \(unit)")
        }
    }
}

private enum FixtureError: Error { case notFound(String) }

/// Walks up from this source file until it finds `relative`, rather than
/// assuming a fixed directory depth — survives the source tree being moved
/// or the test layout changing.
private func repoFixture(_ relative: String) throws -> URL {
    var dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
    while dir.path != "/" {
        let candidate = dir.appendingPathComponent(relative)
        if FileManager.default.fileExists(atPath: candidate.path) {
            return candidate
        }
        dir.deleteLastPathComponent()
    }
    throw FixtureError.notFound(relative)
}

private func loadAgeFormatCases() throws -> [AgeFormatCase] {
    let data = try Data(contentsOf: repoFixture("testdata/age-format-cases.json"))
    return try JSONDecoder().decode([AgeFormatCase].self, from: data)
}

struct FooterFormattingTests {
    @Test func matchesSharedFixture() throws {
        let updated = Date(timeIntervalSince1970: 0)
        for testCase in try loadAgeFormatCases() {
            let now = updated.addingTimeInterval(testCase.seconds)
            #expect(
                updatedAgoText(now: now, updated: updated) == testCase.expectedText,
                "seconds=\(testCase.seconds)"
            )
        }
    }
}
