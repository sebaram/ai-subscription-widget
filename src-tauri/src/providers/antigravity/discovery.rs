pub struct LspInfo {
    pub pid: u32,
    pub csrf_token: Option<String>,
    pub port_hint: Option<u16>,
}

fn flag_value(cmdline: &str, flags: &[&str]) -> Option<String> {
    for f in flags {
        for prefix in [format!("{f}="), format!("{f} ")] {
            if let Some(idx) = cmdline.find(&prefix) {
                let rest = &cmdline[idx + prefix.len()..];
                let end = rest.find(" --").unwrap_or(rest.len());
                let val = rest[..end].trim();
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

pub fn parse_cmdline(cmdline: &str) -> (Option<String>, Option<u16>) {
    let csrf = flag_value(cmdline, &["--csrf_token", "--extension_server_csrf_token"]);
    let mut port = flag_value(cmdline, &["--extension_server_port"])
        .and_then(|v| v.trim().parse::<u16>().ok());
    if port.is_none() {
        port = flag_value(cmdline, &["--https_server_port"])
            .and_then(|v| v.trim().parse::<u16>().ok())
            .filter(|p| *p != 0); // 0 = random port, not discoverable from argv
    }
    (csrf, port)
}

pub fn find_lsp() -> Option<LspInfo> {
    use sysinfo::System;
    let sys = System::new_all().ok()?;
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy().to_lowercase();
        if name.contains("language_server") || name == "agy.exe" || name.starts_with("agy.") {
            let cmdline = p.cmd().iter().map(|c| c.to_string_lossy()).collect::<Vec<_>>().join(" ");
            let (csrf_token, port_hint) = parse_cmdline(&cmdline);
            return Some(LspInfo { pid: pid.as_u32(), csrf_token, port_hint });
        }
    }
    None
}

/// Ports to probe, best-first: PID-owned listening loopback sockets, then argv hint, then argv hint +0..19, then known fallbacks.
pub fn probe_order(info: &LspInfo) -> Vec<u16> {
    let mut ports: Vec<u16> = Vec::new();
    if let Some(listen) = listening_ports(info.pid) {
        ports.extend(listen);
    }
    if let Some(hint) = info.port_hint {
        ports.push(hint);
        ports.extend((0..20u16).map(|i| hint + i));
    }
    ports.extend([53835, 53836, 53837, 53838, 53845, 53849]);
    let mut unique = Vec::new();
    for p in ports {
        if !unique.contains(&p) {
            unique.push(p);
        }
    }
    unique
}

fn listening_ports(pid: u32) -> Option<Vec<u16>> {
    use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
    let sockets = get_sockets_info(AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6, ProtocolFlags::TCP).ok()?;
    let mut out = Vec::new();
    for s in sockets {
        let ProtocolSocketInfo::Tcp(tcp) = s.protocol_socket_info else { continue };
        if s.associated_pids.contains(&pid) && tcp.state == TcpState::Listen && tcp.local_addr.is_loopback() {
            if !out.contains(&tcp.local_port) {
                out.push(tcp.local_port);
            }
        }
    }
    Some(out).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmdline_parses_csrf_and_port() {
        let (csrf, port) = parse_cmdline("agy --csrf_token=tok123 --https_server_port 0 --app_data_dir antigravity");
        assert_eq!(csrf.as_deref(), Some("tok123"));
        assert_eq!(port, None, "https_server_port 0 means random, not discoverable from argv");
        let (csrf2, port2) = parse_cmdline("language_server_windows_x86_64 --extension_server_csrf_token=abc --extension_server_port 53849");
        assert_eq!(csrf2.as_deref(), Some("abc"));
        assert_eq!(port2, Some(53849));
    }

    #[test]
    fn cmdline_empty_is_none() {
        let (csrf, port) = parse_cmdline("language_server.exe");
        assert_eq!(csrf, None);
        assert_eq!(port, None);
    }

    #[test]
    fn probe_order_dedupes() {
        let info = LspInfo { pid: 1, csrf_token: None, port_hint: Some(53835) };
        let ports = probe_order(&info);
        let len = ports.len();
        assert_eq!(ports.iter().collect::<std::collections::HashSet<_>>().len(), len, "no duplicate ports");
        assert_eq!(ports[0], 53835);
    }
}
