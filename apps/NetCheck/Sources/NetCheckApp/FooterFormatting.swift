import Foundation

/// Elapsed-time label for the footer's "Updated ... ago" text. Seconds are
/// only shown under a minute; at and beyond 60 seconds it rounds to minutes,
/// since second-level precision stops being useful past that point.
func updatedAgoText(now: Date, updated: Date) -> String {
    let seconds = Int(now.timeIntervalSince(updated).rounded())
    if seconds < 3 {
        return "Updated just now"
    }
    if seconds < 60 {
        return "Updated \(seconds) second\(seconds == 1 ? "" : "s") ago"
    }
    if seconds < 3600 {
        let minutes = seconds / 60
        return "Updated \(minutes) minute\(minutes == 1 ? "" : "s") ago"
    }
    let hours = seconds / 3600
    return "Updated \(hours) hour\(hours == 1 ? "" : "s") ago"
}
