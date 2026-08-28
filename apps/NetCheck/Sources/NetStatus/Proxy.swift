import Foundation
import SystemConfiguration

public extension ProxyConfig {
    /// No proxy of any kind — the fallback when the dynamic store can't be
    /// read.
    static let disabled = ProxyConfig(
        http: .init(enabled: false, host: nil, port: nil),
        https: .init(enabled: false, host: nil, port: nil),
        socks: .init(enabled: false, host: nil, port: nil),
        pacUrl: nil,
        exceptions: []
    )
}

extension Probe {
    /// The system proxy configuration from `SCDynamicStoreCopyProxies` —
    /// the same data `scutil --proxy` prints. Scoped per-interface
    /// overrides don't appear in this top-level dictionary at all.
    public static func proxyConfig() -> ProxyConfig {
        guard let raw = SCDynamicStoreCopyProxies(nil) as? [String: Any] else { return .disabled }
        return proxyConfig(from: raw)
    }

    static func proxyConfig(from dict: [String: Any]) -> ProxyConfig {
        func flag(_ key: String) -> Bool {
            (dict[key] as? NSNumber)?.boolValue ?? (dict[key] as? Bool) ?? false
        }
        func port(_ key: String) -> UInt16? {
            (dict[key] as? NSNumber).flatMap { UInt16(exactly: $0.intValue) }
        }
        func endpoint(_ enable: String, _ host: String, _ portKey: String) -> ProxyEndpoint {
            ProxyEndpoint(enabled: flag(enable), host: dict[host] as? String, port: port(portKey))
        }

        return ProxyConfig(
            http: endpoint("HTTPEnable", "HTTPProxy", "HTTPPort"),
            https: endpoint("HTTPSEnable", "HTTPSProxy", "HTTPSPort"),
            socks: endpoint("SOCKSEnable", "SOCKSProxy", "SOCKSPort"),
            pacUrl: flag("ProxyAutoConfigEnable") ? dict["ProxyAutoConfigURLString"] as? String : nil,
            exceptions: (dict["ExceptionsList"] as? [String]) ?? []
        )
    }
}
