import Foundation

/// Well-known probe targets.
public enum Defaults {
    /// Cloudflare, Google, Quad9, OpenDNS anycast resolvers.
    public static let pingTargets = [
        "1.0.0.1", "1.1.1.1", "8.8.4.4", "8.8.8.8", "9.9.9.9",
        "208.67.220.220", "208.67.222.222",
    ]

    /// Cloudflare, Google, Quad9 IPv6 anycast.
    public static let pingTargetsV6 = [
        "2001:4860:4860::8844", "2001:4860:4860::8888",
        "2606:4700:4700::1001", "2606:4700:4700::1111", "2620:fe::fe",
    ]

    public static let resolutionTargets = [
        "amazon.com", "apple.com", "cloudflare.com", "github.com",
        "google.com", "microsoft.com", "wikipedia.org",
    ]
}
