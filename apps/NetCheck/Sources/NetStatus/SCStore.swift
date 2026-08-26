import Foundation
import SystemConfiguration

/// Thin typed reads over the System Configuration dynamic store, replacing
/// the old helper layer. Values bridge straight from Core
/// Foundation: nested dictionaries arrive as `[String: Any]`, arrays as
/// `[String]` / `[[String: Any]]`.
enum SCStore {
    static func make(_ name: String) -> SCDynamicStore? {
        SCDynamicStoreCreate(nil, name as CFString, nil, nil)
    }

    static func dictionary(_ store: SCDynamicStore, _ key: String) -> [String: Any]? {
        SCDynamicStoreCopyValue(store, key as CFString) as? [String: Any]
    }

    static func keys(_ store: SCDynamicStore, matching pattern: String) -> [String] {
        (SCDynamicStoreCopyKeyList(store, pattern as CFString) as? [String]) ?? []
    }

    static func string(_ dict: [String: Any], _ key: String) -> String? {
        dict[key] as? String
    }

    static func stringArray(_ dict: [String: Any], _ key: String) -> [String] {
        (dict[key] as? [String]) ?? []
    }

    static func dictArray(_ dict: [String: Any], _ key: String) -> [[String: Any]] {
        (dict[key] as? [[String: Any]]) ?? []
    }

    /// Whether the SC reachability check for `host` reports `.reachable`.
    static func isReachable(_ host: String) -> Bool {
        guard let reachability = SCNetworkReachabilityCreateWithName(nil, host) else { return false }
        var flags = SCNetworkReachabilityFlags()
        guard SCNetworkReachabilityGetFlags(reachability, &flags) else { return false }
        return flags.contains(.reachable)
    }
}
