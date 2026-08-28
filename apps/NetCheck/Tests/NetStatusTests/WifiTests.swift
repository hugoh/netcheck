import CoreWLAN
import Foundation
@testable import NetStatus
import Testing

private func parse(_ json: String) -> AirportInfo {
    parseAirportJSON(Data(json.utf8))
}

struct WifiJSONParsingTests {
    static let connected = """
    {"SPAirPortDataType":[{"spairport_airport_interfaces":[{
      "_name":"en0",
      "spairport_current_network_information":{
        "_name":"MyWiFi",
        "spairport_network_channel":"36 (5GHz, 80MHz)",
        "spairport_network_phymode":"802.11ax",
        "spairport_security_mode":"spairport_security_mode_wpa2_personal",
        "spairport_signal_noise":"-52 dBm / -90 dBm"},
      "spairport_status_information":"spairport_status_connected"}]}]}
    """

    @Test func parsesConnectedNetwork() {
        let info = parse(Self.connected)
        #expect(info.connected)
        #expect(info.ssid == "MyWiFi")
        #expect(info.channel == "36 (5GHz, 80MHz)")
        #expect(info.phyMode == "802.11ax")
        #expect(info.security == "spairport_security_mode_wpa2_personal")
        #expect(info.signalDbm == -52)
        #expect(info.noiseDbm == -90)
    }

    @Test func disconnectedReturnsDefault() {
        let info = parse(
            #"{"SPAirPortDataType":[{"spairport_airport_interfaces":[{"_name":"en0"}]}]}"#
        )
        #expect(!info.connected)
        #expect(info.ssid == nil)
    }

    @Test func malformedJSONReturnsDefault() {
        #expect(!parse("not json").connected)
    }

    @Test func redactedSSIDNormalizedToNil() {
        let info = parse(Self.connected.replacingOccurrences(of: "MyWiFi", with: "<redacted>"))
        #expect(info.connected)
        #expect(info.ssid == nil)
        #expect(info.channel == "36 (5GHz, 80MHz)")
    }

    // Inline JSON fixtures read better unwrapped.
    // swiftlint:disable line_length
    @Test func skipsFalsePositiveInterfaceAndFindsRealConnection() {
        let json = """
        {"SPAirPortDataType":[{"spairport_airport_interfaces":[
          {"_name":"awdl0","spairport_current_network_information":{"spairport_network_type":"spairport_network_type_station"}},
          {"_name":"en0","spairport_current_network_information":{"_name":"MyWiFi"},"spairport_status_information":"spairport_status_connected"}]}]}
        """
        let info = parse(json)
        #expect(info.connected)
        #expect(info.ssid == "MyWiFi")
    }

    @Test func onlyFalsePositiveInterfaceReturnsDefault() {
        let json = """
        {"SPAirPortDataType":[{"spairport_airport_interfaces":[
          {"_name":"awdl0","spairport_current_network_information":{"spairport_network_type":"spairport_network_type_station"}}]}]}
        """
        #expect(!parse(json).connected)
    }

    // swiftlint:enable line_length
}

struct WifiFormattingTests {
    @Test func formatsChannel() {
        #expect(formatChannel(40, band: .band5GHz, width: .width160MHz) == "40 (5GHz, 160MHz)")
        #expect(formatChannel(6, band: .band2GHz, width: .width20MHz) == "6 (2.4GHz, 20MHz)")
    }

    @Test func formatsSecurity() {
        #expect(formatSecurity(.wpa2Personal) == "WPA2 Personal")
        #expect(formatSecurity(.wpa3Transition) == "WPA3 Transition")
        #expect(formatSecurity(.none) == "Open")
    }

    @Test func formatsPhyMode() {
        #expect(formatPhyMode(.mode11ax) == "802.11ax")
        #expect(formatPhyMode(.mode11ac) == "802.11ac")
    }
}

struct WifiStatusCombineTests {
    @Test func disconnectedIdentityDiscardsRadio() {
        let status = WifiStatus(
            identity: WifiIdentity(connected: false, ssid: nil),
            radio: WifiRadio(
                channel: "36 (5GHz, 80MHz)", signalDbm: -52, noiseDbm: -90,
                security: "Open", phyMode: "802.11ax"
            )
        )
        #expect(!status.connected)
        #expect(status.channel == nil)
        #expect(status.security == nil)
    }
}
