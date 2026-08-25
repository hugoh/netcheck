import Foundation
import Testing
@testable import NetCheckApp

/// One entry of testdata/age-format-cases.json, shared with the Rust TUI's
/// format_age tests so both UIs agree on the now/seconds/minutes/hours
/// thresholds.
private struct AgeFormatCase: Decodable {
    let seconds: TimeInterval
    let magnitude: Int
    let unit: String

    var expectedText: String {
        switch unit {
        case "now": return "Updated just now"
        case "s": return "Updated \(magnitude) second\(magnitude == 1 ? "" : "s") ago"
        case "m": return "Updated \(magnitude) minute\(magnitude == 1 ? "" : "s") ago"
        case "h": return "Updated \(magnitude) hour\(magnitude == 1 ? "" : "s") ago"
        default: fatalError("unknown unit \(unit)")
        }
    }
}

private func loadAgeFormatCases() -> [AgeFormatCase] {
    let fixtureURL = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent() // FooterFormattingTests.swift
        .deletingLastPathComponent() // NetCheckAppTests
        .deletingLastPathComponent() // Tests
        .deletingLastPathComponent() // NetCheck
        .deletingLastPathComponent() // apps
        .appendingPathComponent("testdata/age-format-cases.json")
    let data = try! Data(contentsOf: fixtureURL)
    return try! JSONDecoder().decode([AgeFormatCase].self, from: data)
}

struct FooterFormattingTests {
    @Test func matchesSharedFixture() {
        let updated = Date(timeIntervalSince1970: 0)
        for testCase in loadAgeFormatCases() {
            let now = updated.addingTimeInterval(testCase.seconds)
            #expect(
                updatedAgoText(now: now, updated: updated) == testCase.expectedText,
                "seconds=\(testCase.seconds)"
            )
        }
    }
}
