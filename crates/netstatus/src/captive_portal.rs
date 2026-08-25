use serde::Serialize;
use std::time::Duration;
use ureq::Agent;

const CAPTIVE_PORTAL_URL: &str = "http://captive.apple.com/hotspot-detect.html";
const CAPTIVE_PORTAL_TIMEOUT: Duration = Duration::from_secs(10);
const EXPECTED_BODY: &str = "Success";

/// Whether something between this machine and the internet is
/// intercepting plain HTTP traffic to serve a login page (a captive
/// portal), determined by fetching Apple's own hotspot-detect endpoint —
/// the same check macOS itself uses.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum CaptivePortalStatus {
    Clear,
    Detected,
    Unknown,
}

/// Apple's endpoint returns exactly `Success` (200) when nothing
/// intercepted the request. Anything else — a rewritten body, a
/// redirect, any other status — means a portal (or something else) is
/// altering the response.
fn classify_response(status: u16, body: &str) -> CaptivePortalStatus {
    if status == 200 && body.trim() == EXPECTED_BODY {
        CaptivePortalStatus::Clear
    } else {
        CaptivePortalStatus::Detected
    }
}

/// Fetches Apple's captive-portal-check endpoint over plain HTTP — not
/// HTTPS, since a captive portal intercepts/rewrites unencrypted HTTP and
/// this probe needs that interception to be visible. A transport-level
/// failure (timeout, DNS failure, connection refused) means the check
/// itself didn't complete, not that a portal was found, so it maps to
/// `Unknown` rather than `Detected`.
pub fn check_captive_portal() -> CaptivePortalStatus {
    let config = Agent::config_builder()
        .timeout_global(Some(CAPTIVE_PORTAL_TIMEOUT))
        .build();
    let agent: Agent = config.into();

    let Ok(mut response) = agent.get(CAPTIVE_PORTAL_URL).call() else {
        return CaptivePortalStatus::Unknown;
    };
    let status = response.status().as_u16();
    let Ok(body) = response.body_mut().read_to_string() else {
        return CaptivePortalStatus::Unknown;
    };
    classify_response(status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_success_body_is_clear() {
        assert_eq!(
            classify_response(200, "Success"),
            CaptivePortalStatus::Clear
        );
    }

    #[test]
    fn altered_body_is_detected() {
        assert_eq!(
            classify_response(200, "<HTML><HEAD><TITLE>Success</TITLE></HEAD><BODY>Login required</BODY></HTML>"),
            CaptivePortalStatus::Detected
        );
    }

    #[test]
    fn non_200_status_is_detected() {
        assert_eq!(
            classify_response(302, "Success"),
            CaptivePortalStatus::Detected
        );
    }
}
