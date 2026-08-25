import Foundation
import Testing
@testable import NetCheckApp

struct FooterFormattingTests {
    @Test func showsSecondsStartingAtThreeSeconds() {
        let updated = Date(timeIntervalSince1970: 0)
        let now = updated.addingTimeInterval(3)
        #expect(updatedAgoText(now: now, updated: updated) == "Updated 3 seconds ago")
    }

    @Test func showsPluralSecondsUnderOneMinute() {
        let updated = Date(timeIntervalSince1970: 0)
        let now = updated.addingTimeInterval(45)
        #expect(updatedAgoText(now: now, updated: updated) == "Updated 45 seconds ago")
    }

    @Test func hidesSecondsAtSixtySeconds() {
        let updated = Date(timeIntervalSince1970: 0)
        let now = updated.addingTimeInterval(60)
        #expect(updatedAgoText(now: now, updated: updated) == "Updated 1 minute ago")
    }

    @Test func hidesSecondsAboveOneMinute() {
        let updated = Date(timeIntervalSince1970: 0)
        let now = updated.addingTimeInterval(125)
        #expect(updatedAgoText(now: now, updated: updated) == "Updated 2 minutes ago")
    }

    @Test func showsJustNowAtZeroSeconds() {
        let updated = Date(timeIntervalSince1970: 0)
        #expect(updatedAgoText(now: updated, updated: updated) == "Updated just now")
    }

    @Test func showsJustNowUnderThreeSeconds() {
        let updated = Date(timeIntervalSince1970: 0)
        let now = updated.addingTimeInterval(2)
        #expect(updatedAgoText(now: now, updated: updated) == "Updated just now")
    }

    @Test func hidesMinutesAtFiveHoursThreeMinutes() {
        let updated = Date(timeIntervalSince1970: 0)
        let now = updated.addingTimeInterval(5 * 3600 + 3 * 60)
        #expect(updatedAgoText(now: now, updated: updated) == "Updated 5 hours ago")
    }
}
