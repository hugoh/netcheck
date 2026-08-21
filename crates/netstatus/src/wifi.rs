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
        ssid: str_field("_name"),
        channel: str_field("spairport_network_channel"),
        signal_dbm,
        noise_dbm,
        security: str_field("spairport_security_mode"),
        phy_mode: str_field("spairport_network_phymode"),
    }
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

/// Runs `system_profiler SPAirPortDataType -json` and parses the current
/// Wi-Fi network's diagnostics, if any.
pub fn wifi_status() -> WifiStatus {
    let output = Command::new("system_profiler")
        .args(["SPAirPortDataType", "-json"])
        .output()
        .expect("system_profiler should be runnable on macOS");
    parse_wifi_json(&String::from_utf8_lossy(&output.stdout))
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
}
