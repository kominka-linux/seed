use std::env;
use std::fs;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use crate::common::applet::finish;
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;
use crate::common::net::{AddressFamily, list_routes, prefix_to_netmask};

const APPLET: &str = "netstat";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Options {
    all: bool,
    listening: bool,
    numeric: bool,
    tcp: bool,
    udp: bool,
    route: bool,
    wide: bool,
    family: Option<AddressFamily>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Protocol {
    Tcp,
    Tcp6,
    Udp,
    Udp6,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SocketEntry {
    protocol: Protocol,
    recv_q: u64,
    send_q: u64,
    local_addr: String,
    local_port: u16,
    remote_addr: String,
    remote_port: u16,
    state: Option<&'static str>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let mut options = parse_args(args)?;
    if !options.route && !options.tcp && !options.udp {
        options.tcp = true;
        options.udp = true;
    }

    if options.route {
        print_routes(&options)?;
    } else {
        print_sockets(&options)?;
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut cursor = ArgCursor::new(args);

    while let Some(token) = cursor.next_arg() {
        match token {
            ArgToken::ShortFlags(flags) => {
                for flag in flags.chars() {
                    match flag {
                        'a' => options.all = true,
                        'l' => options.listening = true,
                        'n' => options.numeric = true,
                        't' => options.tcp = true,
                        'u' => options.udp = true,
                        'r' => options.route = true,
                        'W' => options.wide = true,
                        '4' => options.family = Some(AddressFamily::Inet4),
                        '6' => options.family = Some(AddressFamily::Inet6),
                        _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                    }
                }
            }
            ArgToken::Operand(_) => return Err(vec![AppletError::new(APPLET, "extra operand")]),
        }
    }

    Ok(options)
}

fn print_sockets(options: &Options) -> Result<(), Vec<AppletError>> {
    let mut entries = Vec::new();
    if options.tcp {
        entries.extend(read_socket_table(Protocol::Tcp)?);
        entries.extend(read_socket_table(Protocol::Tcp6)?);
    }
    if options.udp {
        entries.extend(read_socket_table(Protocol::Udp)?);
        entries.extend(read_socket_table(Protocol::Udp6)?);
    }

    entries.retain(|entry| include_socket(options, entry));
    entries.sort_by(|left, right| {
        protocol_name(left.protocol)
            .cmp(protocol_name(right.protocol))
            .then(left.local_addr.cmp(&right.local_addr))
            .then(left.local_port.cmp(&right.local_port))
    });

    println!("Active Internet connections");
    println!(
        "{:<5} {:>6} {:>6} {:<23} {:<23} State",
        "Proto", "Recv-Q", "Send-Q", "Local Address", "Foreign Address"
    );
    for entry in entries {
        println!(
            "{:<5} {:>6} {:>6} {:<23} {:<23} {}",
            protocol_name(entry.protocol),
            entry.recv_q,
            entry.send_q,
            format_endpoint(&entry.local_addr, entry.local_port),
            format_endpoint(&entry.remote_addr, entry.remote_port),
            entry.state.unwrap_or("")
        );
    }
    Ok(())
}

fn print_routes(options: &Options) -> Result<(), Vec<AppletError>> {
    let mut routes = list_routes().map_err(io_error("listing routes", None))?;
    routes.sort_by(|left, right| left.dev.cmp(&right.dev).then(left.destination.cmp(&right.destination)));
    let show_v4 = options.family != Some(AddressFamily::Inet6);
    let show_v6 = options.family != Some(AddressFamily::Inet4);
    if show_v4 {
        let routes_v4 = routes
            .iter()
            .filter(|route| route.family == AddressFamily::Inet4)
            .collect::<Vec<_>>();
        if !routes_v4.is_empty() || options.family == Some(AddressFamily::Inet4) {
            println!("Kernel IP routing table");
            println!(
                "{:<18} {:<18} {:<18} {:<5} Iface",
                "Destination", "Gateway", "Genmask", "Flags"
            );
            for route in routes_v4 {
                let destination = if route.prefix_len == 0 {
                    String::from("0.0.0.0")
                } else {
                    route.destination.clone()
                };
                let gateway = route.gateway.clone().unwrap_or_else(|| String::from("0.0.0.0"));
                let netmask = prefix_to_netmask(route.prefix_len).to_string();
                let mut flags = String::from("U");
                if gateway != "0.0.0.0" {
                    flags.push('G');
                }
                if route.prefix_len == 32 {
                    flags.push('H');
                }
                println!(
                    "{:<18} {:<18} {:<18} {:<5} {}",
                    destination, gateway, netmask, flags, route.dev
                );
            }
        }
    }
    if show_v6 {
        let routes_v6 = routes
            .iter()
            .filter(|route| route.family == AddressFamily::Inet6)
            .collect::<Vec<_>>();
        if !routes_v6.is_empty() {
            if show_v4 {
                println!();
            }
            println!("Kernel IPv6 routing table");
            println!(
                "{:<32} {:<32} {:<5} Iface",
                "Destination", "Next Hop", "Flags"
            );
            for route in routes_v6 {
                let destination = if route.prefix_len == 0 {
                    String::from("default")
                } else {
                    format!("{}/{}", route.destination, route.prefix_len)
                };
                let gateway = route.gateway.clone().unwrap_or_else(|| String::from("::"));
                let mut flags = String::from("U");
                if gateway != "::" {
                    flags.push('G');
                }
                if route.prefix_len == 128 {
                    flags.push('H');
                }
                println!(
                    "{:<32} {:<32} {:<5} {}",
                    destination, gateway, flags, route.dev
                );
            }
        }
    }
    Ok(())
}

fn include_socket(options: &Options, entry: &SocketEntry) -> bool {
    if let Some(family) = options.family {
        let matches_family = match entry.protocol {
            Protocol::Tcp | Protocol::Udp => family == AddressFamily::Inet4,
            Protocol::Tcp6 | Protocol::Udp6 => family == AddressFamily::Inet6,
        };
        if !matches_family {
            return false;
        }
    }
    if options.listening {
        return is_listener(entry);
    }
    if options.all {
        return true;
    }
    true
}

fn is_listener(entry: &SocketEntry) -> bool {
    match entry.protocol {
        Protocol::Tcp | Protocol::Tcp6 => entry.state == Some("LISTEN"),
        Protocol::Udp | Protocol::Udp6 => entry.remote_port == 0,
    }
}

fn read_socket_table(protocol: Protocol) -> Result<Vec<SocketEntry>, Vec<AppletError>> {
    let path = proc_path(protocol);
    let text = fs::read_to_string(&path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&path.to_string_lossy()), err)])?;
    parse_socket_table(protocol, &text)
}

fn parse_socket_table(protocol: Protocol, text: &str) -> Result<Vec<SocketEntry>, Vec<AppletError>> {
    text.lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| parse_socket_line(protocol, line))
        .collect()
}

fn parse_socket_line(protocol: Protocol, line: &str) -> Result<SocketEntry, Vec<AppletError>> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 5 {
        return Err(vec![AppletError::new(APPLET, "malformed socket table entry")]);
    }

    let (local_addr, local_port) = parse_proc_endpoint(protocol, fields[1])?;
    let (remote_addr, remote_port) = parse_proc_endpoint(protocol, fields[2])?;
    let state = parse_state(protocol, fields[3])?;
    let (send_q, recv_q) = parse_queues(fields[4])?;

    Ok(SocketEntry {
        protocol,
        recv_q,
        send_q,
        local_addr,
        local_port,
        remote_addr,
        remote_port,
        state,
    })
}

fn parse_proc_endpoint(protocol: Protocol, value: &str) -> Result<(String, u16), Vec<AppletError>> {
    let (address, port) = value
        .split_once(':')
        .ok_or_else(|| vec![AppletError::new(APPLET, "malformed socket address")])?;
    let port = u16::from_str_radix(port, 16)
        .map_err(|_| vec![AppletError::new(APPLET, "invalid port in socket table")])?;
    let address = match protocol {
        Protocol::Tcp | Protocol::Udp => parse_ipv4_hex(address)?.to_string(),
        Protocol::Tcp6 | Protocol::Udp6 => parse_ipv6_hex(address)?.to_string(),
    };
    Ok((address, port))
}

fn parse_ipv4_hex(value: &str) -> Result<Ipv4Addr, Vec<AppletError>> {
    if value.len() != 8 {
        return Err(vec![AppletError::new(APPLET, "invalid IPv4 address in socket table")]);
    }
    let raw = u32::from_str_radix(value, 16)
        .map_err(|_| vec![AppletError::new(APPLET, "invalid IPv4 address in socket table")])?;
    Ok(Ipv4Addr::from(raw.to_le_bytes()))
}

fn parse_ipv6_hex(value: &str) -> Result<Ipv6Addr, Vec<AppletError>> {
    if value.len() != 32 {
        return Err(vec![AppletError::new(APPLET, "invalid IPv6 address in socket table")]);
    }
    let mut bytes = [0_u8; 16];
    for (chunk_index, chunk) in value.as_bytes().chunks(8).enumerate() {
        let word = std::str::from_utf8(chunk)
            .map_err(|_| vec![AppletError::new(APPLET, "invalid IPv6 address in socket table")])?;
        let raw = u32::from_str_radix(word, 16)
            .map_err(|_| vec![AppletError::new(APPLET, "invalid IPv6 address in socket table")])?;
        bytes[chunk_index * 4..chunk_index * 4 + 4].copy_from_slice(&raw.to_le_bytes());
    }
    Ok(Ipv6Addr::from(bytes))
}

fn parse_state(protocol: Protocol, value: &str) -> Result<Option<&'static str>, Vec<AppletError>> {
    match protocol {
        Protocol::Udp | Protocol::Udp6 => Ok(None),
        Protocol::Tcp | Protocol::Tcp6 => Ok(Some(match value {
            "01" => "ESTABLISHED",
            "02" => "SYN_SENT",
            "03" => "SYN_RECV",
            "04" => "FIN_WAIT1",
            "05" => "FIN_WAIT2",
            "06" => "TIME_WAIT",
            "07" => "CLOSE",
            "08" => "CLOSE_WAIT",
            "09" => "LAST_ACK",
            "0A" => "LISTEN",
            "0B" => "CLOSING",
            _ => return Err(vec![AppletError::new(APPLET, "unknown TCP state in socket table")]),
        })),
    }
}

fn parse_queues(value: &str) -> Result<(u64, u64), Vec<AppletError>> {
    let (send_q, recv_q) = value
        .split_once(':')
        .ok_or_else(|| vec![AppletError::new(APPLET, "malformed socket queue state")])?;
    Ok((
        u64::from_str_radix(send_q, 16)
            .map_err(|_| vec![AppletError::new(APPLET, "invalid send queue in socket table")])?,
        u64::from_str_radix(recv_q, 16)
            .map_err(|_| vec![AppletError::new(APPLET, "invalid receive queue in socket table")])?,
    ))
}

fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Tcp => "tcp",
        Protocol::Tcp6 => "tcp6",
        Protocol::Udp => "udp",
        Protocol::Udp6 => "udp6",
    }
}

fn format_endpoint(address: &str, port: u16) -> String {
    if port == 0 {
        format!("{address}:*")
    } else if address.contains(':') {
        format!("[{address}]:{port}")
    } else {
        format!("{address}:{port}")
    }
}

fn proc_path(protocol: Protocol) -> PathBuf {
    let env_name = match protocol {
        Protocol::Tcp => "SEED_PROC_NET_TCP",
        Protocol::Tcp6 => "SEED_PROC_NET_TCP6",
        Protocol::Udp => "SEED_PROC_NET_UDP",
        Protocol::Udp6 => "SEED_PROC_NET_UDP6",
    };
    if let Some(path) = env::var_os(env_name) {
        return PathBuf::from(path);
    }
    PathBuf::from(match protocol {
        Protocol::Tcp => "/proc/net/tcp",
        Protocol::Tcp6 => "/proc/net/tcp6",
        Protocol::Udp => "/proc/net/udp",
        Protocol::Udp6 => "/proc/net/udp6",
    })
}

fn io_error(
    action: &'static str,
    path: Option<String>,
) -> impl FnOnce(io::Error) -> Vec<AppletError> {
    move |err| vec![AppletError::from_io(APPLET, action, path.as_deref(), err)]
}

#[cfg(test)]
mod tests {
    use super::{
        AddressFamily, Protocol, format_endpoint, parse_args, parse_ipv4_hex, parse_ipv6_hex,
        parse_socket_line, parse_state,
    };
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn parses_ipv4_proc_address() {
        assert_eq!(parse_ipv4_hex("0100007F").unwrap(), Ipv4Addr::new(127, 0, 0, 1));
    }

    #[test]
    fn parses_ipv6_proc_address() {
        assert_eq!(parse_ipv6_hex("00000000000000000000000001000000").unwrap(), Ipv6Addr::LOCALHOST);
    }

    #[test]
    fn parses_tcp_socket_line() {
        let entry = parse_socket_line(
            Protocol::Tcp,
            "0: 0100007F:1F90 00000000:0000 0A 00000002:00000001 00:00000000 00000000 0 0 1 1",
        )
        .unwrap();
        assert_eq!(entry.local_addr, "127.0.0.1");
        assert_eq!(entry.local_port, 8080);
        assert_eq!(entry.remote_port, 0);
        assert_eq!(entry.state, Some("LISTEN"));
        assert_eq!(entry.send_q, 2);
        assert_eq!(entry.recv_q, 1);
    }

    #[test]
    fn maps_tcp_state_names() {
        assert_eq!(parse_state(Protocol::Tcp, "01").unwrap(), Some("ESTABLISHED"));
        assert!(parse_state(Protocol::Tcp, "ff").is_err());
    }

    #[test]
    fn formats_endpoints() {
        assert_eq!(format_endpoint("127.0.0.1", 80), "127.0.0.1:80");
        assert_eq!(format_endpoint("0.0.0.0", 0), "0.0.0.0:*");
        assert_eq!(format_endpoint("::1", 53), "[::1]:53");
    }

    #[test]
    fn parses_family_flags() {
        let options = parse_args(&["-6".to_string(), "-rn".to_string()]).unwrap();
        assert_eq!(options.family, Some(AddressFamily::Inet6));
        assert!(options.route);
        assert!(options.numeric);
    }
}
