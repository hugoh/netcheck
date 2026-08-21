use objc2_core_wlan::{CWChannelBand, CWChannelWidth, CWPHYMode, CWSecurity, CWWiFiClient};
use serde::Serialize;
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct WifiStatus {
    pub connected: bool,
    pub ssid: Option<String>,
    pub channel: Option<String>,
    pub signal_dbm: Option<i32>,
    pub noise_dbm: Option<i32>,
    pub security: Option<String>,
    pub phy_mode: Option<String>,
}

/// SSID/connected-state, the slow half of Wi-Fi status — only obtainable via
/// `system_profiler` (a ~1s shell-out), since reading it through CoreWLAN
/// needs Location Services authorization a bare CLI/TUI binary can't get.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct WifiIdentity {
    pub connected: bool,
    pub ssid: Option<String>,
}

/// Channel/signal/noise/security/PHY-mode, the fast half of Wi-Fi status —
/// read straight from CoreWLAN (`CWWiFiClient`/`CWInterface`), no shell-out.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct WifiRadio {
    pub channel: Option<String>,
    pub signal_dbm: Option<i32>,
    pub noise_dbm: Option<i32>,
    pub security: Option<String>,
    pub phy_mode: Option<String>,
}

pub(crate) fn parse_wifi_json(json: &str) -> WifiStatus {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return WifiStatus::default();
    };

    let current = root
        .get("SPAirPortDataType")
        .and_then(Value::as_array)
        .and_then(|entries| entries.first())
        .and_then(|entry| entry.get("spairport_airport_interfaces"))
        .and_then(Value::as_array)
        .and_then(|ifaces| ifaces.iter().find_map(connected_network_information));

    let Some(current) = current else {
        return WifiStatus::default();
    };

    let str_field = |key: &str| current.get(key).and_then(Value::as_str).map(str::to_string);

    let (signal_dbm, noise_dbm) = str_field("spairport_signal_noise")
        .as_deref()
        .and_then(parse_signal_noise)
        .unzip();

    WifiStatus {
        connected: true,
        ssid: ssid_field(current),
        channel: str_field("spairport_network_channel"),
        signal_dbm,
        noise_dbm,
        security: str_field("spairport_security_mode"),
        phy_mode: str_field("spairport_network_phymode"),
    }
}

/// As of macOS 26, `system_profiler`'s `_name` field for the connected
/// network is the literal string `"<redacted>"` rather than the real SSID,
/// or simply absent — both mean "unavailable", for a process lacking
/// Location Services authorization. Normalized to `None` so callers see an
/// honest "unknown" rather than a placeholder that looks like real data.
fn ssid_field(current: &Value) -> Option<String> {
    let name = current.get("_name").and_then(Value::as_str)?;
    (name != "<redacted>").then(|| name.to_string())
}

/// Returns this interface's `spairport_current_network_information` block
/// only if the interface is actually connected to a network.
///
/// A Wi-Fi-family interface can carry a
/// `spairport_current_network_information` key even when it isn't really
/// associated with anything — e.g. `awdl0` (Apple Wireless Direct Link)
/// reports `{"spairport_network_type": "spairport_network_type_station"}`
/// with no SSID whenever the radio is up, regardless of the real Wi-Fi
/// interface's state. The interface-level `spairport_status_information`
/// key (a sibling of `spairport_current_network_information`, not nested
/// inside it) is `"spairport_status_connected"` when genuinely connected;
/// fall back to requiring a non-empty `_name` (SSID) if that key is absent.
fn connected_network_information(iface: &Value) -> Option<&Value> {
    let current = iface.get("spairport_current_network_information")?;

    let status_connected = iface
        .get("spairport_status_information")
        .and_then(Value::as_str)
        == Some("spairport_status_connected");
    let has_ssid = current
        .get("_name")
        .and_then(Value::as_str)
        .is_some_and(|name| !name.is_empty());

    (status_connected || has_ssid).then_some(current)
}

fn parse_signal_noise(text: &str) -> Option<(i32, i32)> {
    let (signal, noise) = text.split_once('/')?;
    let parse_dbm = |s: &str| s.split_whitespace().next()?.parse::<i32>().ok();
    Some((parse_dbm(signal)?, parse_dbm(noise)?))
}

fn format_channel(number: isize, band: CWChannelBand, width: CWChannelWidth) -> String {
    let band = match band {
        CWChannelBand::Band2GHz => "2.4GHz",
        CWChannelBand::Band5GHz => "5GHz",
        CWChannelBand::Band6GHz => "6GHz",
        _ => "unknown band",
    };
    let width = match width {
        CWChannelWidth::Width20MHz => "20MHz",
        CWChannelWidth::Width40MHz => "40MHz",
        CWChannelWidth::Width80MHz => "80MHz",
        CWChannelWidth::Width160MHz => "160MHz",
        _ => "unknown width",
    };
    format!("{number} ({band}, {width})")
}

fn format_security(security: CWSecurity) -> &'static str {
    match security {
        CWSecurity::None => "Open",
        CWSecurity::WEP => "WEP",
        CWSecurity::WPAPersonal => "WPA Personal",
        CWSecurity::WPAPersonalMixed => "WPA Personal Mixed",
        CWSecurity::WPA2Personal => "WPA2 Personal",
        CWSecurity::WPAEnterprise => "WPA Enterprise",
        CWSecurity::WPAEnterpriseMixed => "WPA Enterprise Mixed",
        CWSecurity::WPA2Enterprise => "WPA2 Enterprise",
        CWSecurity::WPA3Personal => "WPA3 Personal",
        CWSecurity::WPA3Enterprise => "WPA3 Enterprise",
        CWSecurity::WPA3Transition => "WPA3 Transition",
        _ => "Unknown",
    }
}

fn format_phy_mode(phy: CWPHYMode) -> &'static str {
    match phy {
        CWPHYMode::Mode11a => "802.11a",
        CWPHYMode::Mode11b => "802.11b",
        CWPHYMode::Mode11g => "802.11g",
        CWPHYMode::Mode11n => "802.11n",
        CWPHYMode::Mode11ac => "802.11ac",
        CWPHYMode::Mode11ax => "802.11ax",
        _ => "unknown",
    }
}

/// Reads channel/signal/noise/security/PHY-mode straight from CoreWLAN
/// (`CWWiFiClient`/`CWInterface`) — no shell-out, no Location Services
/// authorization needed for these particular properties (unlike SSID).
/// Fast: no subprocess, returns in well under a millisecond.
pub fn wifi_radio() -> WifiRadio {
    let Some(iface) = (unsafe { CWWiFiClient::sharedWiFiClient().interface() }) else {
        return WifiRadio::default();
    };

    let channel = unsafe { iface.wlanChannel() }.map(|c| {
        let number = unsafe { c.channelNumber() };
        let band = unsafe { c.channelBand() };
        let width = unsafe { c.channelWidth() };
        format_channel(number, band, width)
    });

    let signal_dbm = i32::try_from(unsafe { iface.rssiValue() }).ok();
    let noise_dbm = i32::try_from(unsafe { iface.noiseMeasurement() }).ok();
    let security = Some(format_security(unsafe { iface.security() }).to_string());
    let phy_mode = Some(format_phy_mode(unsafe { iface.activePHYMode() }).to_string());

    WifiRadio {
        channel,
        signal_dbm,
        noise_dbm,
        security,
        phy_mode,
    }
}

/// Reads SSID/connected-state via `system_profiler SPAirPortDataType
/// -json` — the only source a bare binary can reach without Location
/// Services authorization. Slow: a subprocess call, commonly ~1s.
pub fn wifi_identity() -> WifiIdentity {
    let output = Command::new("system_profiler")
        .args(["SPAirPortDataType", "-json"])
        .output()
        .expect("system_profiler should be runnable on macOS");
    let status = parse_wifi_json(&String::from_utf8_lossy(&output.stdout));
    WifiIdentity {
        connected: status.connected,
        ssid: status.ssid,
    }
}

/// The combined snapshot `netcheck wifi`/`netcheck status` report — SSID
/// and connected-state from `system_profiler`, everything else natively
/// from CoreWLAN. For streaming (`collect_streaming`), `wifi_identity` and
/// `wifi_radio` run as independent probes instead, so the fast CoreWLAN
/// fields don't wait on the slow `system_profiler` call.
pub fn wifi_status() -> WifiStatus {
    let identity = wifi_identity();
    if !identity.connected {
        return WifiStatus::default();
    }

    let radio = wifi_radio();
    WifiStatus {
        connected: identity.connected,
        ssid: identity.ssid,
        channel: radio.channel,
        signal_dbm: radio.signal_dbm,
        noise_dbm: radio.noise_dbm,
        security: radio.security,
        phy_mode: radio.phy_mode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONNECTED_JSON: &str = r#"{
      "SPAirPortDataType" : [
        {
          "spairport_airport_interfaces" : [
            {
              "_name" : "en0",
              "spairport_current_network_information" : {
                "_name" : "MyWiFi",
                "spairport_network_channel" : "36 (5GHz, 80MHz)",
                "spairport_network_phymode" : "802.11ax",
                "spairport_security_mode" : "spairport_security_mode_wpa2_personal",
                "spairport_signal_noise" : "-52 dBm / -90 dBm"
              },
              "spairport_status_information" : "spairport_status_connected"
            }
          ]
        }
      ]
    }"#;

    const DISCONNECTED_JSON: &str = r#"{
      "SPAirPortDataType" : [
        {
          "spairport_airport_interfaces" : [
            { "_name" : "en0" }
          ]
        }
      ]
    }"#;

    /// Models macOS 26's Location-Services-gated system_profiler output:
    /// connected (status + real channel/signal/etc. still present), but
    /// `_name` is the literal string "<redacted>" instead of the real SSID.
    const REDACTED_SSID_JSON: &str = r#"{
      "SPAirPortDataType" : [
        {
          "spairport_airport_interfaces" : [
            {
              "_name" : "en0",
              "spairport_current_network_information" : {
                "_name" : "<redacted>",
                "spairport_network_channel" : "36 (5GHz, 80MHz)",
                "spairport_network_phymode" : "802.11ax",
                "spairport_security_mode" : "spairport_security_mode_wpa2_personal",
                "spairport_signal_noise" : "-52 dBm / -90 dBm"
              },
              "spairport_status_information" : "spairport_status_connected"
            }
          ]
        }
      ]
    }"#;

    /// Models real macOS output: `awdl0` carries a
    /// `spairport_current_network_information` block with no SSID and no
    /// `spairport_status_information`, while `en0` is genuinely connected.
    const MULTI_INTERFACE_JSON: &str = r#"{
      "SPAirPortDataType" : [
        {
          "spairport_airport_interfaces" : [
            {
              "_name" : "awdl0",
              "spairport_current_network_information" : {
                "spairport_network_type" : "spairport_network_type_station"
              }
            },
            {
              "_name" : "en0",
              "spairport_current_network_information" : {
                "_name" : "MyWiFi",
                "spairport_network_channel" : "36 (5GHz, 80MHz)",
                "spairport_network_phymode" : "802.11ax",
                "spairport_security_mode" : "spairport_security_mode_wpa2_personal",
                "spairport_signal_noise" : "-52 dBm / -90 dBm"
              },
              "spairport_status_information" : "spairport_status_connected"
            }
          ]
        }
      ]
    }"#;

    /// Models the radio-off case where ONLY the false-positive `awdl0`
    /// interface reports a `spairport_current_network_information` block.
    const ONLY_FALSE_POSITIVE_JSON: &str = r#"{
      "SPAirPortDataType" : [
        {
          "spairport_airport_interfaces" : [
            {
              "_name" : "awdl0",
              "spairport_current_network_information" : {
                "spairport_network_type" : "spairport_network_type_station"
              }
            }
          ]
        }
      ]
    }"#;

    #[test]
    fn parses_connected_network() {
        let status = parse_wifi_json(CONNECTED_JSON);
        assert!(status.connected);
        assert_eq!(status.ssid, Some("MyWiFi".to_string()));
        assert_eq!(status.channel, Some("36 (5GHz, 80MHz)".to_string()));
        assert_eq!(status.phy_mode, Some("802.11ax".to_string()));
        assert_eq!(
            status.security,
            Some("spairport_security_mode_wpa2_personal".to_string())
        );
        assert_eq!(status.signal_dbm, Some(-52));
        assert_eq!(status.noise_dbm, Some(-90));
    }

    #[test]
    fn disconnected_returns_default() {
        let status = parse_wifi_json(DISCONNECTED_JSON);
        assert_eq!(status, WifiStatus::default());
    }

    #[test]
    fn redacted_ssid_normalized_to_none() {
        let status = parse_wifi_json(REDACTED_SSID_JSON);
        assert!(status.connected);
        assert_eq!(status.ssid, None);
        // The rest of the connection info is real and unaffected.
        assert_eq!(status.channel, Some("36 (5GHz, 80MHz)".to_string()));
        assert_eq!(status.signal_dbm, Some(-52));
    }

    #[test]
    fn malformed_json_returns_default() {
        let status = parse_wifi_json("not json");
        assert_eq!(status, WifiStatus::default());
    }

    #[test]
    fn skips_false_positive_interface_and_finds_real_connection() {
        let status = parse_wifi_json(MULTI_INTERFACE_JSON);
        assert!(status.connected);
        assert_eq!(status.ssid, Some("MyWiFi".to_string()));
    }

    #[test]
    fn only_false_positive_interface_returns_default() {
        let status = parse_wifi_json(ONLY_FALSE_POSITIVE_JSON);
        assert_eq!(status, WifiStatus::default());
    }

    #[test]
    fn formats_channel() {
        assert_eq!(
            format_channel(40, CWChannelBand::Band5GHz, CWChannelWidth::Width160MHz),
            "40 (5GHz, 160MHz)"
        );
        assert_eq!(
            format_channel(6, CWChannelBand::Band2GHz, CWChannelWidth::Width20MHz),
            "6 (2.4GHz, 20MHz)"
        );
    }

    #[test]
    fn formats_security() {
        assert_eq!(format_security(CWSecurity::WPA2Personal), "WPA2 Personal");
        assert_eq!(
            format_security(CWSecurity::WPA3Transition),
            "WPA3 Transition"
        );
        assert_eq!(format_security(CWSecurity::None), "Open");
    }

    #[test]
    fn formats_phy_mode() {
        assert_eq!(format_phy_mode(CWPHYMode::Mode11ax), "802.11ax");
        assert_eq!(format_phy_mode(CWPHYMode::Mode11ac), "802.11ac");
    }
}
