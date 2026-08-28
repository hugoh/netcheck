import CoreWLAN
import Foundation

public extension WifiStatus {
    /// Combines the two halves: when the identity half says disconnected,
    /// the radio half is discarded so a down radio can't show a phantom
    /// channel/security.
    init(identity: WifiIdentity, radio: WifiRadio) {
        let radio = identity.connected ? radio : .empty
        connected = identity.connected
        ssid = identity.connected ? identity.ssid : nil
        channel = radio.channel
        signalDbm = radio.signalDbm
        noiseDbm = radio.noiseDbm
        security = radio.security
        phyMode = radio.phyMode
    }
}

public extension WifiRadio {
    static let empty = WifiRadio(
        channel: nil, signalDbm: nil, noiseDbm: nil, security: nil, phyMode: nil
    )
}

public extension Probe {
    /// Channel/signal/noise/security/PHY-mode straight from CoreWLAN — no
    /// shell-out, no Location Services authorization.
    static func wifiRadio() -> WifiRadio {
        guard let interface = CWWiFiClient.shared().interface() else { return .empty }

        let channel = interface.wlanChannel()
        let associated = channel != nil

        return WifiRadio(
            channel: channel.map {
                formatChannel($0.channelNumber, band: $0.channelBand, width: $0.channelWidth)
            },
            signalDbm: associated ? Int(exactly: interface.rssiValue()) : nil,
            noiseDbm: associated ? Int(exactly: interface.noiseMeasurement()) : nil,
            security: associated ? formatSecurity(interface.security()) : nil,
            phyMode: associated ? formatPhyMode(interface.activePHYMode()) : nil
        )
    }

    /// SSID/connected-state via `system_profiler SPAirPortDataType -json` —
    /// the only source reachable without Location Services authorization.
    static func wifiIdentity() async -> WifiIdentity {
        await blocking {
            guard let output = runSystemProfilerAirport() else {
                return WifiIdentity(connected: false, ssid: nil)
            }
            let info = parseAirportJSON(output)
            return WifiIdentity(connected: info.connected, ssid: info.ssid)
        }
    }

    private static func runSystemProfilerAirport() -> Data? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/system_profiler")
        process.arguments = ["SPAirPortDataType", "-json"]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            return nil
        }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        return data
    }
}

struct AirportInfo {
    var connected = false
    var ssid: String?
    var channel: String?
    var signalDbm: Int?
    var noiseDbm: Int?
    var security: String?
    var phyMode: String?
}

/// Parses `system_profiler SPAirPortDataType -json` output — finds the
/// first genuinely-connected Wi-Fi interface and reads its current-network
/// block; malformed or disconnected input yields a disconnected
/// `AirportInfo`.
func parseAirportJSON(_ data: Data) -> AirportInfo {
    guard let root = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
          let entries = root["SPAirPortDataType"] as? [[String: Any]],
          let interfaces = entries.first?["spairport_airport_interfaces"] as? [[String: Any]],
          let current = interfaces.lazy.compactMap(connectedNetworkInformation).first
    else { return AirportInfo() }

    let (signal, noise) = parseSignalNoise(current["spairport_signal_noise"] as? String)

    return AirportInfo(
        connected: true,
        ssid: ssidField(current),
        channel: current["spairport_network_channel"] as? String,
        signalDbm: signal,
        noiseDbm: noise,
        security: current["spairport_security_mode"] as? String,
        phyMode: current["spairport_network_phymode"] as? String
    )
}

/// macOS 26 reports `_name` as the literal `"<redacted>"` (or omits it) for
/// a process lacking Location Services authorization — normalise both to nil.
private func ssidField(_ current: [String: Any]) -> String? {
    guard let name = current["_name"] as? String, name != "<redacted>" else { return nil }
    return name
}

/// A Wi-Fi-family interface can carry a current-network block even when not
/// associated (e.g. `awdl0`). Require either
/// `spairport_status_information == connected` or a non-empty SSID.
private func connectedNetworkInformation(_ interface: [String: Any]) -> [String: Any]? {
    guard let current = interface["spairport_current_network_information"] as? [String: Any]
    else { return nil }

    let statusConnected =
        (interface["spairport_status_information"] as? String) == "spairport_status_connected"
    let hasSSID = !((current["_name"] as? String) ?? "").isEmpty
    return statusConnected || hasSSID ? current : nil
}

private func parseSignalNoise(_ text: String?) -> (Int?, Int?) {
    guard let text, let slash = text.firstIndex(of: "/") else { return (nil, nil) }
    func dbm(_ substring: Substring) -> Int? {
        substring.split(separator: " ").first.flatMap { Int($0) }
    }
    return (dbm(text[..<slash]), dbm(text[text.index(after: slash)...]))
}

func formatChannel(_ number: Int, band: CWChannelBand, width: CWChannelWidth) -> String {
    let bandName = switch band {
    case .band2GHz: "2.4GHz"
    case .band5GHz: "5GHz"
    case .band6GHz: "6GHz"
    default: "unknown band"
    }
    let widthName = switch width {
    case .width20MHz: "20MHz"
    case .width40MHz: "40MHz"
    case .width80MHz: "80MHz"
    case .width160MHz: "160MHz"
    default: "unknown width"
    }
    return "\(number) (\(bandName), \(widthName))"
}

private let securityNames: [CWSecurity: String] = [
    .none: "Open",
    .WEP: "WEP",
    .wpaPersonal: "WPA Personal",
    .wpaPersonalMixed: "WPA Personal Mixed",
    .wpa2Personal: "WPA2 Personal",
    .wpaEnterprise: "WPA Enterprise",
    .wpaEnterpriseMixed: "WPA Enterprise Mixed",
    .wpa2Enterprise: "WPA2 Enterprise",
    .wpa3Personal: "WPA3 Personal",
    .wpa3Enterprise: "WPA3 Enterprise",
    .wpa3Transition: "WPA3 Transition",
]

func formatSecurity(_ security: CWSecurity) -> String {
    securityNames[security] ?? "Unknown"
}

func formatPhyMode(_ phy: CWPHYMode) -> String {
    switch phy {
    case .mode11a: "802.11a"
    case .mode11b: "802.11b"
    case .mode11g: "802.11g"
    case .mode11n: "802.11n"
    case .mode11ac: "802.11ac"
    case .mode11ax: "802.11ax"
    default: "unknown"
    }
}
