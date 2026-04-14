use std::fs;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::common::applet::finish_code;
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::dhcp::{
    BOOTREPLY, DhcpPacket, MESSAGE_ACK, MESSAGE_DISCOVER, MESSAGE_NAK, MESSAGE_OFFER,
    MESSAGE_RELEASE, MESSAGE_REQUEST, OPTION_DNS, OPTION_LEASE_TIME, OPTION_MESSAGE_TYPE,
    OPTION_REQUESTED_IP, OPTION_ROUTER, OPTION_SERVER_ID, OPTION_SUBNET_MASK, decode_ipv4, decode_u32,
    default_server_identifier, encode_ipv4, encode_ipv4_list, encode_option_value, encode_u32,
    format_mac, option_code, prepare_socket, receive_packet, send_packet, server_port,
};
use crate::common::error::AppletError;

const APPLET: &str = "udhcpd";
const DEFAULT_CONFIG: &str = "/etc/udhcpd.conf";
const DEFAULT_LEASE_SECS: u32 = 600;

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(args)?;
    let config = read_config(&options.config_path)?;
    let _pidfile = options
        .pidfile
        .clone()
        .or_else(|| config.pidfile.clone())
        .map(PidFileGuard::create)
        .transpose()?;
    let socket = bind_socket()?;
    let packet_limit = packet_limit();
    let mut leases = load_leases(config.lease_file.as_deref())?;
    let mut processed = 0_u64;

    loop {
        purge_expired(&mut leases);
        let Some((packet, from)) = receive_packet(&socket).map_err(|err| vec![err])? else {
            continue;
        };
        if packet.op != 1 {
            continue;
        }
        if handle_packet(&socket, &config, &mut leases, &packet, from)? {
            persist_leases(config.lease_file.as_deref(), &leases)?;
        }
        processed = processed.saturating_add(1);
        if packet_limit.is_some_and(|limit| processed >= limit) {
            return Ok(0);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    config_path: PathBuf,
    pidfile: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ServerConfig {
    start: Ipv4Addr,
    end: Ipv4Addr,
    interface: String,
    server_id: Ipv4Addr,
    subnet: Option<Ipv4Addr>,
    routers: Vec<Ipv4Addr>,
    dns: Vec<Ipv4Addr>,
    lease_time: u32,
    max_leases: Option<usize>,
    lease_file: Option<PathBuf>,
    pidfile: Option<PathBuf>,
    siaddr: Option<Ipv4Addr>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Lease {
    ip: Ipv4Addr,
    mac: [u8; 6],
    expires_at: u64,
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut config_path = None;
    let mut pidfile = None;

    while let Some(arg) = cursor.next_arg() {
        match arg {
            ArgToken::Operand(value) => {
                if config_path.is_some() {
                    return Err(vec![AppletError::new(APPLET, "extra operand")]);
                }
                config_path = Some(PathBuf::from(value));
            }
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'f' | 'S' => {}
                        'p' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            pidfile = Some(PathBuf::from(cursor.next_value_or_attached(
                                attached, APPLET, "p",
                            )?));
                            break;
                        }
                        other => return Err(vec![AppletError::invalid_option(APPLET, other)]),
                    }
                    offset += flag.len_utf8();
                }
            }
        }
    }

    Ok(Options {
        config_path: config_path.unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG)),
        pidfile,
    })
}

fn read_config(path: &Path) -> Result<ServerConfig, Vec<AppletError>> {
    let path_text = path.to_string_lossy().into_owned();
    let text = fs::read_to_string(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&path_text), err)])?;
    parse_config(&text)
}

fn parse_config(text: &str) -> Result<ServerConfig, Vec<AppletError>> {
    let mut start = None;
    let mut end = None;
    let mut interface = String::from("eth0");
    let mut server_id = None;
    let mut subnet = None;
    let mut routers = Vec::new();
    let mut dns = Vec::new();
    let mut lease_time = DEFAULT_LEASE_SECS;
    let mut max_leases = None;
    let mut lease_file = None;
    let mut pidfile = None;
    let mut siaddr = None;

    for (line_number, raw_line) in text.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err(vec![AppletError::new(
                APPLET,
                format!("invalid config line {}: '{line}'", line_number + 1),
            )]);
        }

        match parts[0] {
            "start" => start = Some(parse_ipv4(parts[1], line_number + 1)?),
            "end" => end = Some(parse_ipv4(parts[1], line_number + 1)?),
            "interface" => interface = parts[1].to_string(),
            "server" => server_id = Some(parse_ipv4(parts[1], line_number + 1)?),
            "max_leases" => {
                max_leases = Some(parts[1].parse::<usize>().map_err(|_| {
                    vec![AppletError::new(
                        APPLET,
                        format!("invalid config line {}: '{line}'", line_number + 1),
                    )]
                })?);
            }
            "lease_file" => lease_file = Some(PathBuf::from(parts[1])),
            "pidfile" => pidfile = Some(PathBuf::from(parts[1])),
            "siaddr" => siaddr = Some(parse_ipv4(parts[1], line_number + 1)?),
            "option" | "opt" => {
                let Some(code) = option_code(parts[1]) else {
                    return Err(vec![AppletError::new(
                        APPLET,
                        format!("unsupported DHCP option '{}'", parts[1]),
                    )]);
                };
                let value = encode_option_value(code, &parts[2..]).map_err(|err| vec![err])?;
                match code {
                    OPTION_SUBNET_MASK => subnet = decode_ipv4(&value),
                    OPTION_ROUTER => routers = crate::common::dhcp::decode_ipv4_list(&value).unwrap_or_default(),
                    OPTION_DNS => dns = crate::common::dhcp::decode_ipv4_list(&value).unwrap_or_default(),
                    OPTION_LEASE_TIME => lease_time = decode_u32(&value).unwrap_or(DEFAULT_LEASE_SECS),
                    OPTION_SERVER_ID => server_id = decode_ipv4(&value),
                    _ => {}
                }
            }
            // BusyBox configs commonly carry these; we accept and ignore them for compatibility.
            "remaining" | "auto_time" | "decline_time" | "conflict_time" | "offer_time"
            | "min_lease" | "notify_file" | "sname" | "boot_file" => {}
            _ => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unsupported config directive '{}'", parts[0]),
                )]);
            }
        }
    }

    let start = start.ok_or_else(|| vec![AppletError::new(APPLET, "missing 'start' in config")])?;
    let end = end.ok_or_else(|| vec![AppletError::new(APPLET, "missing 'end' in config")])?;
    if ipv4_to_u32(start) > ipv4_to_u32(end) {
        return Err(vec![AppletError::new(APPLET, "lease pool start exceeds end")]);
    }
    let server_id = server_id.unwrap_or(default_server_identifier().map_err(|err| vec![err])?);
    Ok(ServerConfig {
        start,
        end,
        interface,
        server_id,
        subnet,
        routers,
        dns,
        lease_time,
        max_leases,
        lease_file,
        pidfile,
        siaddr,
    })
}

fn bind_socket() -> Result<UdpSocket, Vec<AppletError>> {
    let socket = UdpSocket::bind(("0.0.0.0", server_port()))
        .map_err(|err| vec![AppletError::from_io(APPLET, "binding", None, err)])?;
    prepare_socket(&socket).map_err(|err| vec![AppletError::from_io(APPLET, "configuring", None, err)])?;
    Ok(socket)
}

fn handle_packet(
    socket: &UdpSocket,
    config: &ServerConfig,
    leases: &mut Vec<Lease>,
    packet: &DhcpPacket,
    from: SocketAddr,
) -> Result<bool, Vec<AppletError>> {
    match packet.message_type() {
        Some(MESSAGE_DISCOVER) => {
            if let Some(ip) = select_offer_ip(config, leases, packet.mac(), requested_ip(packet))? {
                let reply = make_reply(config, packet, MESSAGE_OFFER, ip);
                send_packet(socket, &reply, reply_target(from))
                    .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
            }
            Ok(false)
        }
        Some(MESSAGE_REQUEST) => {
            if let Some(server_id) = packet.option(OPTION_SERVER_ID).and_then(decode_ipv4)
                && server_id != config.server_id
            {
                return Ok(false);
            }
            let Some(ip) = requested_ip(packet).or_else(|| nonzero_ip(packet.ciaddr)) else {
                let reply = make_nak(config, packet);
                send_packet(socket, &reply, reply_target(from))
                    .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
                return Ok(false);
            };
            if !ip_in_range(config, ip) || !ip_available(leases, packet.mac(), ip) {
                let reply = make_nak(config, packet);
                send_packet(socket, &reply, reply_target(from))
                    .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
                return Ok(false);
            }
            assign_lease(config, leases, packet.mac(), ip);
            let reply = make_reply(config, packet, MESSAGE_ACK, ip);
            send_packet(socket, &reply, reply_target(from))
                .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
            Ok(true)
        }
        Some(MESSAGE_RELEASE) => {
            let ip = nonzero_ip(packet.ciaddr).or_else(|| requested_ip(packet));
            release_lease(leases, packet.mac(), ip);
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn requested_ip(packet: &DhcpPacket) -> Option<Ipv4Addr> {
    packet.option(OPTION_REQUESTED_IP).and_then(decode_ipv4)
}

fn reply_target(from: SocketAddr) -> SocketAddr {
    SocketAddr::new(from.ip(), from.port())
}

fn make_reply(config: &ServerConfig, request: &DhcpPacket, message_type: u8, yiaddr: Ipv4Addr) -> DhcpPacket {
    let mut reply = DhcpPacket::new(BOOTREPLY, request.xid, request.mac());
    reply.flags = request.flags;
    reply.yiaddr = yiaddr;
    reply.siaddr = config.siaddr.unwrap_or(config.server_id);
    reply.set_option(OPTION_MESSAGE_TYPE, vec![message_type]);
    reply.set_option(OPTION_SERVER_ID, encode_ipv4(config.server_id));
    reply.set_option(OPTION_LEASE_TIME, encode_u32(config.lease_time));
    if let Some(subnet) = config.subnet {
        reply.set_option(OPTION_SUBNET_MASK, encode_ipv4(subnet));
    }
    if !config.routers.is_empty() {
        reply.set_option(OPTION_ROUTER, encode_ipv4_list(&config.routers));
    }
    if !config.dns.is_empty() {
        reply.set_option(OPTION_DNS, encode_ipv4_list(&config.dns));
    }
    reply
}

fn make_nak(config: &ServerConfig, request: &DhcpPacket) -> DhcpPacket {
    let mut reply = DhcpPacket::new(BOOTREPLY, request.xid, request.mac());
    reply.flags = request.flags;
    reply.set_option(OPTION_MESSAGE_TYPE, vec![MESSAGE_NAK]);
    reply.set_option(OPTION_SERVER_ID, encode_ipv4(config.server_id));
    reply
}

fn select_offer_ip(
    config: &ServerConfig,
    leases: &[Lease],
    mac: [u8; 6],
    requested: Option<Ipv4Addr>,
) -> Result<Option<Ipv4Addr>, Vec<AppletError>> {
    if let Some(lease) = leases.iter().find(|lease| lease.mac == mac && !lease_expired(lease)) {
        return Ok(Some(lease.ip));
    }
    if let Some(requested) = requested
        && ip_in_range(config, requested)
        && ip_available(leases, mac, requested)
    {
        return Ok(Some(requested));
    }
    find_first_available(config, leases, mac)
}

fn find_first_available(
    config: &ServerConfig,
    leases: &[Lease],
    mac: [u8; 6],
) -> Result<Option<Ipv4Addr>, Vec<AppletError>> {
    if config
        .max_leases
        .is_some_and(|limit| active_lease_count(leases) >= limit && !leases.iter().any(|lease| lease.mac == mac))
    {
        return Ok(None);
    }

    let mut current = ipv4_to_u32(config.start);
    let end = ipv4_to_u32(config.end);
    while current <= end {
        let ip = u32_to_ipv4(current);
        if ip_available(leases, mac, ip) {
            return Ok(Some(ip));
        }
        current = current.saturating_add(1);
    }
    Ok(None)
}

fn assign_lease(config: &ServerConfig, leases: &mut Vec<Lease>, mac: [u8; 6], ip: Ipv4Addr) {
    let expires_at = unix_now().saturating_add(u64::from(config.lease_time));
    leases.retain(|lease| !(lease.mac == mac || lease.ip == ip));
    leases.push(Lease {
        ip,
        mac,
        expires_at,
    });
    leases.sort_by_key(|lease| ipv4_to_u32(lease.ip));
}

fn release_lease(leases: &mut Vec<Lease>, mac: [u8; 6], ip: Option<Ipv4Addr>) {
    leases.retain(|lease| {
        if lease.mac != mac {
            return true;
        }
        ip.is_none_or(|release_ip| lease.ip != release_ip)
    });
}

fn ip_available(leases: &[Lease], mac: [u8; 6], ip: Ipv4Addr) -> bool {
    leases
        .iter()
        .filter(|lease| !lease_expired(lease))
        .all(|lease| lease.ip != ip || lease.mac == mac)
}

fn active_lease_count(leases: &[Lease]) -> usize {
    leases.iter().filter(|lease| !lease_expired(lease)).count()
}

fn ip_in_range(config: &ServerConfig, ip: Ipv4Addr) -> bool {
    let value = ipv4_to_u32(ip);
    value >= ipv4_to_u32(config.start) && value <= ipv4_to_u32(config.end)
}

fn parse_ipv4(value: &str, line_number: usize) -> Result<Ipv4Addr, Vec<AppletError>> {
    value.parse::<Ipv4Addr>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid IPv4 address on config line {line_number}: '{value}'"),
        )]
    })
}

fn packet_limit() -> Option<u64> {
    std::env::var("SEED_UDHCPD_MAX_PACKETS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
}

fn load_leases(path: Option<&Path>) -> Result<Vec<Lease>, Vec<AppletError>> {
    let Some(path) = path else {
        return Ok(Vec::new());
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let mut leases = Vec::new();
    for line in text.lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 {
            continue;
        }
        let Ok(ip) = parts[0].parse::<Ipv4Addr>() else {
            continue;
        };
        let Ok(mac) = crate::common::dhcp::parse_mac(parts[1]) else {
            continue;
        };
        let Ok(expires_at) = parts[2].parse::<u64>() else {
            continue;
        };
        leases.push(Lease {
            ip,
            mac,
            expires_at,
        });
    }
    purge_expired(&mut leases);
    Ok(leases)
}

fn persist_leases(path: Option<&Path>, leases: &[Lease]) -> Result<(), Vec<AppletError>> {
    let Some(path) = path else {
        return Ok(());
    };
    let mut lines = String::new();
    for lease in leases.iter().filter(|lease| !lease_expired(lease)) {
        lines.push_str(&format!(
            "{} {} {}\n",
            lease.ip,
            format_mac(lease.mac),
            lease.expires_at
        ));
    }
    fs::write(path, lines).map_err(|err| {
        vec![AppletError::from_io(
            APPLET,
            "writing",
            Some(&path.to_string_lossy()),
            err,
        )]
    })
}

fn purge_expired(leases: &mut Vec<Lease>) {
    leases.retain(|lease| !lease_expired(lease));
}

fn lease_expired(lease: &Lease) -> bool {
    lease.expires_at <= unix_now()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn ipv4_to_u32(ip: Ipv4Addr) -> u32 {
    u32::from_be_bytes(ip.octets())
}

fn u32_to_ipv4(value: u32) -> Ipv4Addr {
    Ipv4Addr::from(value.to_be_bytes())
}

fn nonzero_ip(ip: Ipv4Addr) -> Option<Ipv4Addr> {
    (ip != Ipv4Addr::UNSPECIFIED).then_some(ip)
}

struct PidFileGuard {
    path: PathBuf,
}

impl PidFileGuard {
    fn create(path: PathBuf) -> Result<Self, Vec<AppletError>> {
        fs::write(&path, format!("{}\n", std::process::id())).map_err(|err| {
            vec![AppletError::from_io(
                APPLET,
                "writing",
                Some(&path.to_string_lossy()),
                err,
            )]
        })?;
        Ok(Self { path })
    }
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Lease, ServerConfig, assign_lease, find_first_available, parse_args, parse_config,
        release_lease, select_offer_ip,
    };
    use std::net::Ipv4Addr;
    use std::path::PathBuf;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn sample_config() -> ServerConfig {
        ServerConfig {
            start: Ipv4Addr::new(10, 0, 0, 10),
            end: Ipv4Addr::new(10, 0, 0, 12),
            interface: String::from("eth0"),
            server_id: Ipv4Addr::new(10, 0, 0, 1),
            subnet: Some(Ipv4Addr::new(255, 255, 255, 0)),
            routers: vec![Ipv4Addr::new(10, 0, 0, 1)],
            dns: vec![Ipv4Addr::new(1, 1, 1, 1)],
            lease_time: 600,
            max_leases: None,
            lease_file: None,
            pidfile: None,
            siaddr: None,
        }
    }

    #[test]
    fn parses_flags_and_config_operand() {
        let options = parse_args(&args(&["-f", "-p", "/tmp/pid", "/tmp/udhcpd.conf"])).unwrap();
        assert_eq!(options.config_path, PathBuf::from("/tmp/udhcpd.conf"));
        assert_eq!(options.pidfile, Some(PathBuf::from("/tmp/pid")));
    }

    #[test]
    fn parses_config_file_subset() {
        let config = parse_config(
            "start 10.0.0.10\nend 10.0.0.20\ninterface eth0\nserver 10.0.0.1\noption subnet 255.255.255.0\noption router 10.0.0.1\noption dns 1.1.1.1 8.8.8.8\noption lease 900\nlease_file /tmp/leases\n",
        )
        .unwrap();
        assert_eq!(config.start, Ipv4Addr::new(10, 0, 0, 10));
        assert_eq!(config.end, Ipv4Addr::new(10, 0, 0, 20));
        assert_eq!(config.server_id, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(config.lease_time, 900);
        assert_eq!(config.dns.len(), 2);
    }

    #[test]
    fn offer_prefers_requested_available_address() {
        let config = sample_config();
        let offered = select_offer_ip(
            &config,
            &[],
            [0, 1, 2, 3, 4, 5],
            Some(Ipv4Addr::new(10, 0, 0, 12)),
        )
        .unwrap();
        assert_eq!(offered, Some(Ipv4Addr::new(10, 0, 0, 12)));
    }

    #[test]
    fn offer_reuses_existing_lease_for_same_client() {
        let config = sample_config();
        let leases = vec![Lease {
            ip: Ipv4Addr::new(10, 0, 0, 11),
            mac: [0, 1, 2, 3, 4, 5],
            expires_at: u64::MAX,
        }];
        let offered = select_offer_ip(&config, &leases, [0, 1, 2, 3, 4, 5], None).unwrap();
        assert_eq!(offered, Some(Ipv4Addr::new(10, 0, 0, 11)));
    }

    #[test]
    fn assigns_and_releases_leases() {
        let config = sample_config();
        let mut leases = Vec::new();
        assign_lease(
            &config,
            &mut leases,
            [0, 1, 2, 3, 4, 5],
            Ipv4Addr::new(10, 0, 0, 10),
        );
        assert_eq!(find_first_available(&config, &leases, [6, 7, 8, 9, 10, 11]).unwrap(), Some(Ipv4Addr::new(10, 0, 0, 11)));
        release_lease(
            &mut leases,
            [0, 1, 2, 3, 4, 5],
            Some(Ipv4Addr::new(10, 0, 0, 10)),
        );
        assert!(leases.is_empty());
    }
}
