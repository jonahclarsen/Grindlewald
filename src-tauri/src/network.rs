use std::{net::Ipv4Addr, time::Duration};

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
pub struct HomeNetworkStatus {
    pub fingerprint: Option<String>,
}

fn wifi_interfaces(output: &str) -> Vec<String> {
    let mut wifi = false;
    let mut interfaces = Vec::new();
    for line in output.lines() {
        if let Some(port) = line.strip_prefix("Hardware Port: ") {
            wifi = matches!(port.trim(), "Wi-Fi" | "AirPort");
        } else if wifi && let Some(device) = line.strip_prefix("Device: ") {
            let device = device.trim();
            if device.strip_prefix("en").is_some_and(|number| {
                !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                interfaces.push(device.to_owned());
            }
        }
    }
    interfaces
}

fn router_address(output: &str, interface: &str) -> Option<Ipv4Addr> {
    let value = |key: &str| {
        output
            .lines()
            .find_map(|line| line.trim().strip_prefix(key))
            .map(str::trim)
    };
    if value("interface:")? != interface {
        return None;
    }
    let address: Ipv4Addr = value("gateway:")?.parse().ok()?;
    (!address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && !address.is_broadcast())
    .then_some(address)
}

fn router_fingerprint(output: &str, router: Ipv4Addr, interface: &str) -> Option<String> {
    for line in output.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() < 6
            || fields[1] != format!("({router})")
            || fields[2] != "at"
            || fields[4] != "on"
            || fields[5] != interface
        {
            continue;
        }
        let parts: Vec<_> = fields[3].split(':').collect();
        if parts.len() != 6
            || parts.iter().any(|part| {
                part.is_empty()
                    || part.len() > 2
                    || !part.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            continue;
        }
        let bytes: Vec<u8> = parts
            .iter()
            .map(|part| u8::from_str_radix(part, 16).unwrap())
            .collect();
        // Only a nonzero unicast Ethernet address can identify the router.
        if bytes.iter().all(|byte| *byte == 0) || bytes[0] & 1 != 0 {
            continue;
        }
        return Some(format!("router-sha256:{:x}", Sha256::digest(&bytes)));
    }
    None
}

pub(crate) fn matches_saved_network(saved: Option<&str>, current: Option<&str>) -> bool {
    match (saved, current) {
        (Some(saved), Some(current)) if saved == current => {
            saved.strip_prefix("router-sha256:").is_some_and(|hash| {
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        }
        _ => false,
    }
}

async fn command(program: &str, args: &[&str]) -> Result<Option<String>, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new(program)
            .args(args)
            .env("LC_ALL", "C")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "Home network lookup timed out. Try again.".to_string())?
    .map_err(|_| "Could not start the home network lookup.".to_string())?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
}

async fn connected_router(interface: &str) -> Result<Option<Ipv4Addr>, String> {
    let link = command("/sbin/ifconfig", &[interface])
        .await?
        .unwrap_or_default();
    if !link.lines().any(|line| line.trim() == "status: active") {
        return Ok(None);
    }
    // Scope the route to physical Wi-Fi so a VPN's default route cannot identify home.
    let route = command(
        "/sbin/route",
        &["-n", "get", "-ifscope", interface, "default"],
    )
    .await?
    .unwrap_or_default();
    Ok(router_address(&route, interface))
}

async fn lookup() -> Result<HomeNetworkStatus, String> {
    let ports = command("/usr/sbin/networksetup", &["-listallhardwareports"])
        .await?
        .ok_or("Could not discover Wi-Fi interfaces.")?;
    for interface in wifi_interfaces(&ports) {
        let Some(router) = connected_router(&interface).await? else {
            continue;
        };
        let address = router.to_string();
        let mut arp = command("/usr/sbin/arp", &["-n", "-i", &interface, &address])
            .await?
            .unwrap_or_default();
        if router_fingerprint(&arp, router, &interface).is_none() {
            // Populate an empty ARP cache with one interface-bound local probe.
            // A router need not answer ICMP: address resolution alone is sufficient.
            let _ = command(
                "/sbin/ping",
                &[
                    "-n", "-c", "1", "-W", "500", "-t", "1", "-b", &interface, &address,
                ],
            )
            .await?;
            arp = command("/usr/sbin/arp", &["-n", "-i", &interface, &address])
                .await?
                .unwrap_or_default();
        }
        let fingerprint = router_fingerprint(&arp, router, &interface);
        // Do not save an identity if the route/link changed during the lookup.
        if fingerprint.is_some() && connected_router(&interface).await? == Some(router) {
            return Ok(HomeNetworkStatus { fingerprint });
        }
    }
    Ok(HomeNetworkStatus { fingerprint: None })
}

#[tauri::command]
pub async fn home_network_status() -> Result<HomeNetworkStatus, String> {
    if !cfg!(target_os = "macos") {
        return Err("Home network detection is only supported on macOS.".into());
    }
    tokio::time::timeout(Duration::from_secs(8), lookup())
        .await
        .map_err(|_| "Home network lookup timed out. Try again.".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_wifi_without_assuming_en0() {
        assert_eq!(
            wifi_interfaces(
                "Hardware Port: Ethernet\nDevice: en0\n\nHardware Port: Wi-Fi\nDevice: en7\nEthernet Address: ignored\n"
            ),
            vec!["en7"]
        );
        assert!(wifi_interfaces("Hardware Port: Wi-Fi\nDevice: en0;invalid\n").is_empty());
    }

    #[test]
    fn rejects_vpn_and_invalid_routes() {
        assert_eq!(
            router_address(" gateway: 192.0.2.1\n interface: en7\n", "en7"),
            Some(Ipv4Addr::new(192, 0, 2, 1))
        );
        assert_eq!(
            router_address(" gateway: 192.0.2.1\n interface: utun0\n", "en7"),
            None
        );
        assert_eq!(
            router_address(" gateway: link#7\n interface: en7\n", "en7"),
            None
        );
    }

    #[test]
    fn normalizes_router_mac_and_requires_the_correct_route_and_interface() {
        let router = Ipv4Addr::new(192, 0, 2, 1);
        let fingerprint = router_fingerprint(
            "? (192.0.2.1) at 2:0:0:ab:cd:ef on en7 ifscope [ethernet]",
            router,
            "en7",
        );
        assert!(fingerprint.is_some());
        assert_eq!(
            fingerprint,
            router_fingerprint(
                "? (192.0.2.1) at 02:00:00:AB:CD:EF on en7 ifscope [ethernet]",
                router,
                "en7"
            )
        );
        for invalid in [
            "(incomplete)",
            "ff:ff:ff:ff:ff:ff",
            "00:00:00:00:00:00",
            "02:00:00:00:00:zz",
        ] {
            assert!(
                router_fingerprint(&format!("? (192.0.2.1) at {invalid} on en7"), router, "en7")
                    .is_none()
            );
        }
        assert!(
            router_fingerprint("? (192.0.2.1) at 02:00:00:ab:cd:ef on en0", router, "en7")
                .is_none()
        );
        assert!(
            router_fingerprint("? (192.0.2.2) at 02:00:00:ab:cd:ef on en7", router, "en7")
                .is_none()
        );
        let other = router_fingerprint("? (192.0.2.1) at 02:00:00:ab:cd:ee on en7", router, "en7");
        assert!(matches_saved_network(
            fingerprint.as_deref(),
            fingerprint.as_deref()
        ));
        assert!(!matches_saved_network(
            fingerprint.as_deref(),
            other.as_deref()
        ));
        assert!(!matches_saved_network(fingerprint.as_deref(), None));
        assert!(!matches_saved_network(None, None));
        assert!(!matches_saved_network(Some("Old SSID"), Some("Old SSID")));
    }

    #[tokio::test]
    #[ignore = "Requires this Mac to be connected to an IPv4 Wi-Fi network"]
    async fn connected_mac_can_identify_router_without_location_services() {
        let status = home_network_status().await.unwrap();
        assert!(status.fingerprint.is_some(), "No router identity available");
        assert!(matches_saved_network(
            status.fingerprint.as_deref(),
            status.fingerprint.as_deref()
        ));
    }
}
