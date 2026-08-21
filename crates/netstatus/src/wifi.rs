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
        .and_then(|ifaces| {
            ifaces
                .iter()
                .find_map(|iface| iface.get("spairport_current_network_information"))
        });

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
              }
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
}
