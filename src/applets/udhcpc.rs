use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::common::applet::finish_code;
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::dhcp::{
    BOOTREQUEST, DhcpPacket, MESSAGE_ACK, MESSAGE_DECLINE, MESSAGE_DISCOVER, MESSAGE_NAK,
    MESSAGE_OFFER, MESSAGE_RELEASE, MESSAGE_REQUEST, OPTION_CLIENT_ID, OPTION_DNS,
    OPTION_HOSTNAME, OPTION_LEASE_TIME, OPTION_MESSAGE_TYPE, OPTION_PARAMETER_REQUEST_LIST,
    OPTION_REQUESTED_IP, OPTION_REBINDING_TIME, OPTION_RENEWAL_TIME, OPTION_ROUTER,
    OPTION_SERVER_ID, OPTION_SUBNET_MASK, OPTION_VENDOR_CLASS_ID, client_port, decode_ipv4,
    decode_ipv4_list, decode_u32, default_client_mac, default_server_addr, encode_ipv4,
    encode_u32, option_code, prepare_socket, probe_arp_conflict, receive_packet, send_packet,
    server_port,
};
use crate::common::error::AppletError;

const APPLET: &str = "udhcpc";
const DEFAULT_INTERFACE: &str = "eth0";
const DEFAULT_SCRIPT: &str = "/usr/share/udhcpc/default.script";
const DEFAULT_DISCOVER_ATTEMPTS: u32 = 3;
const DEFAULT_RETRY_INTERVAL_SECS: u64 = 5;
const DEFAULT_RETRY_WAIT_SECS: u64 = 20;
const DEFAULT_ARP_TIMEOUT_MILLIS: u32 = 2000;
const BROADCAST_FLAG: u16 = 0x8000;

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> Result<i32, Vec<AppletError>> {
    let options = parse_args(args)?;
    let _pidfile = options
        .pidfile
        .clone()
        .map(PidFileGuard::create)
        .transpose()?;
    let socket = bind_socket()?;
    let mac = default_client_mac(&options.interface).map_err(|err| vec![err])?;
    let mut lease = obtain_lease(&socket, &options, mac)?;
    run_script(&options, "bound", &lease)?;

    if options.release_on_exit && options.quit_after_lease {
        release_lease(&socket, mac, &lease)?;
    }
    if options.quit_after_lease {
        return Ok(0);
    }

    loop {
        let sleep_secs = u64::from(lease.lease_seconds.max(2) / 2);
        thread::sleep(Duration::from_secs(sleep_secs));
        match renew_lease(&socket, &options, mac, &lease) {
            Ok(next_lease) => {
                lease = next_lease;
                run_script(&options, "renew", &lease)?;
            }
            Err(_) => {
                lease = obtain_lease(&socket, &options, mac)?;
                run_script(&options, "bound", &lease)?;
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    interface: String,
    script: Option<String>,
    pidfile: Option<PathBuf>,
    broadcast: bool,
    discover_attempts: u32,
    retry_interval_secs: u64,
    retry_wait_secs: u64,
    exit_if_no_lease: bool,
    quit_after_lease: bool,
    release_on_exit: bool,
    arp_probe_timeout_ms: Option<u32>,
    request_ip: Option<Ipv4Addr>,
    request_options: Vec<u8>,
    send_options: BTreeMap<u8, Vec<u8>>,
    omit_client_id: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LeaseInfo {
    ip: Ipv4Addr,
    server_id: Ipv4Addr,
    server_addr: SocketAddr,
    subnet: Option<Ipv4Addr>,
    routers: Vec<Ipv4Addr>,
    dns: Vec<Ipv4Addr>,
    lease_seconds: u32,
    renewal_seconds: Option<u32>,
    rebinding_seconds: Option<u32>,
    siaddr: Option<Ipv4Addr>,
    sname: Option<String>,
    boot_file: Option<String>,
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut interface = String::from(DEFAULT_INTERFACE);
    let mut script = None;
    let mut pidfile = None;
    let mut broadcast = false;
    let mut discover_attempts = DEFAULT_DISCOVER_ATTEMPTS;
    let mut retry_interval_secs = DEFAULT_RETRY_INTERVAL_SECS;
    let mut retry_wait_secs = DEFAULT_RETRY_WAIT_SECS;
    let mut exit_if_no_lease = false;
    let mut quit_after_lease = false;
    let mut release_on_exit = false;
    let mut arp_probe_timeout_ms = None;
    let mut request_ip = None;
    let mut request_options = Vec::new();
    let mut suppress_default_request_options = false;
    let mut send_options = BTreeMap::new();
    let mut omit_client_id = false;

    while let Some(arg) = cursor.next_arg() {
        match arg {
            ArgToken::Operand(value) => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unexpected operand '{value}'"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'f' | 'S' | 'b' => {}
                        'B' => broadcast = true,
                        'n' => exit_if_no_lease = true,
                        'q' => quit_after_lease = true,
                        'R' => release_on_exit = true,
                        'C' => omit_client_id = true,
                        'i' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            interface = cursor
                                .next_value_or_attached(attached, APPLET, "i")?
                                .to_string();
                            break;
                        }
                        's' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            script = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "s")?
                                    .to_string(),
                            );
                            break;
                        }
                        'p' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            pidfile = Some(PathBuf::from(cursor.next_value_or_attached(
                                attached, APPLET, "p",
                            )?));
                            break;
                        }
                        't' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            discover_attempts = parse_u32(
                                cursor.next_value_or_attached(attached, APPLET, "t")?,
                                "t",
                            )?;
                            break;
                        }
                        'T' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            retry_interval_secs = u64::from(parse_u32(
                                cursor.next_value_or_attached(attached, APPLET, "T")?,
                                "T",
                            )?);
                            break;
                        }
                        'A' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            retry_wait_secs = u64::from(parse_u32(
                                cursor.next_value_or_attached(attached, APPLET, "A")?,
                                "A",
                            )?);
                            break;
                        }
                        'r' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, "r")?;
                            request_ip = Some(parse_ipv4(value)?);
                            break;
                        }
                        'o' => {
                            request_options.clear();
                            suppress_default_request_options = true;
                        }
                        'O' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, "O")?;
                            let Some(code) = option_code(value) else {
                                return Err(vec![AppletError::new(
                                    APPLET,
                                    format!("unknown DHCP option '{value}'"),
                                )]);
                            };
                            if !request_options.contains(&code) {
                                request_options.push(code);
                            }
                            break;
                        }
                        'x' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, "x")?;
                            let (code, encoded) = parse_extra_option(value)?;
                            send_options.insert(code, encoded);
                            break;
                        }
                        'F' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, "F")?;
                            send_options.insert(OPTION_HOSTNAME, value.as_bytes().to_vec());
                            break;
                        }
                        'V' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, APPLET, "V")?;
                            send_options.insert(OPTION_VENDOR_CLASS_ID, value.as_bytes().to_vec());
                            break;
                        }
                        'a' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            arp_probe_timeout_ms = Some(if attached.is_empty() {
                                DEFAULT_ARP_TIMEOUT_MILLIS
                            } else if attached.bytes().all(|byte| byte.is_ascii_digit()) {
                                parse_u32(attached, "a")?
                            } else {
                                DEFAULT_ARP_TIMEOUT_MILLIS
                            });
                            if !attached.is_empty()
                                && attached.bytes().all(|byte| byte.is_ascii_digit())
                            {
                                break;
                            }
                        }
                        other => return Err(vec![AppletError::invalid_option(APPLET, other)]),
                    }
                    offset += flag.len_utf8();
                }
            }
        }
    }

    if discover_attempts == 0 {
        discover_attempts = DEFAULT_DISCOVER_ATTEMPTS;
    }
    if request_options.is_empty() && !suppress_default_request_options {
        request_options.extend([OPTION_SUBNET_MASK, OPTION_ROUTER, OPTION_DNS]);
    }
    if script.is_none() && Path::new(DEFAULT_SCRIPT).exists() {
        script = Some(String::from(DEFAULT_SCRIPT));
    }

    Ok(Options {
        interface,
        script,
        pidfile,
        broadcast,
        discover_attempts,
        retry_interval_secs,
        retry_wait_secs,
        exit_if_no_lease,
        quit_after_lease,
        release_on_exit,
        arp_probe_timeout_ms,
        request_ip,
        request_options,
        send_options,
        omit_client_id,
    })
}

fn bind_socket() -> Result<UdpSocket, Vec<AppletError>> {
    let socket = UdpSocket::bind(("0.0.0.0", client_port()))
        .map_err(|err| vec![AppletError::from_io(APPLET, "binding", None, err)])?;
    prepare_socket(&socket).map_err(|err| vec![AppletError::from_io(APPLET, "configuring", None, err)])?;
    Ok(socket)
}

fn obtain_lease(
    socket: &UdpSocket,
    options: &Options,
    mac: [u8; 6],
) -> Result<LeaseInfo, Vec<AppletError>> {
    loop {
        let xid = transaction_id().map_err(|err| vec![err])?;
        for _ in 0..options.discover_attempts {
            let discover = build_discover(options, xid, mac);
            let target = default_server_addr().map_err(|err| vec![err])?;
            send_packet(socket, &discover, target)
                .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;

            let Some((offer, from)) = wait_for_reply(socket, xid, mac, &[MESSAGE_OFFER])? else {
                thread::sleep(Duration::from_secs(options.retry_interval_secs));
                continue;
            };
            let Some(offer_ip) = nonzero_ip(offer.yiaddr) else {
                continue;
            };
            let server_id = offer
                .option(OPTION_SERVER_ID)
                .and_then(decode_ipv4)
                .unwrap_or_else(|| ipv4_from_socket_addr(from).unwrap_or(Ipv4Addr::new(127, 0, 0, 1)));
            let request = build_request(options, xid, mac, offer_ip, server_id);
            send_packet(socket, &request, from)
                .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;

            if let Some((reply, reply_from)) =
                wait_for_reply(socket, xid, mac, &[MESSAGE_ACK, MESSAGE_NAK])?
            {
                if reply.message_type() == Some(MESSAGE_NAK) {
                    continue;
                }
                if let Some(conflict_ip) = nonzero_ip(reply.yiaddr)
                    && offered_address_is_in_use(options, mac, conflict_ip)
                {
                    send_decline(socket, mac, conflict_ip, &reply, reply_from)?;
                    continue;
                }
                return lease_from_ack(&reply, reply_from);
            }
        }

        if options.exit_if_no_lease {
            return Err(vec![AppletError::new(APPLET, "no lease, failing")]);
        }
        thread::sleep(Duration::from_secs(options.retry_wait_secs));
    }
}

fn renew_lease(
    socket: &UdpSocket,
    options: &Options,
    mac: [u8; 6],
    lease: &LeaseInfo,
) -> Result<LeaseInfo, Vec<AppletError>> {
    let xid = transaction_id().map_err(|err| vec![err])?;
    let request = build_request(options, xid, mac, lease.ip, lease.server_id);
    send_packet(socket, &request, lease.server_addr)
        .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
    let Some((reply, from)) = wait_for_reply(socket, xid, mac, &[MESSAGE_ACK, MESSAGE_NAK])? else {
        return Err(vec![AppletError::new(APPLET, "renewal timed out")]);
    };
    if reply.message_type() == Some(MESSAGE_NAK) {
        return Err(vec![AppletError::new(APPLET, "renewal rejected")]);
    }
    if let Some(conflict_ip) = nonzero_ip(reply.yiaddr)
        && offered_address_is_in_use(options, mac, conflict_ip)
    {
        send_decline(socket, mac, conflict_ip, &reply, from)?;
        return Err(vec![AppletError::new(
            APPLET,
            "renewal declined after ARP conflict check",
        )]);
    }
    lease_from_ack(&reply, from)
}

fn release_lease(
    socket: &UdpSocket,
    mac: [u8; 6],
    lease: &LeaseInfo,
) -> Result<(), Vec<AppletError>> {
    let xid = transaction_id().map_err(|err| vec![err])?;
    let mut packet = DhcpPacket::new(BOOTREQUEST, xid, mac);
    packet.ciaddr = lease.ip;
    packet.set_option(OPTION_MESSAGE_TYPE, vec![MESSAGE_RELEASE]);
    packet.set_option(OPTION_SERVER_ID, encode_ipv4(lease.server_id));
    send_packet(socket, &packet, lease.server_addr)
        .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
    Ok(())
}

fn send_decline(
    socket: &UdpSocket,
    mac: [u8; 6],
    requested_ip: Ipv4Addr,
    reply: &DhcpPacket,
    target: SocketAddr,
) -> Result<(), Vec<AppletError>> {
    let mut packet = DhcpPacket::new(BOOTREQUEST, reply.xid, mac);
    packet.set_option(OPTION_MESSAGE_TYPE, vec![MESSAGE_DECLINE]);
    packet.set_option(OPTION_REQUESTED_IP, encode_ipv4(requested_ip));
    if let Some(server_id) = reply
        .option(OPTION_SERVER_ID)
        .and_then(decode_ipv4)
        .or_else(|| ipv4_from_socket_addr(target))
    {
        packet.set_option(OPTION_SERVER_ID, encode_ipv4(server_id));
    }
    send_packet(socket, &packet, target)
        .map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
    Ok(())
}

fn build_discover(options: &Options, xid: u32, mac: [u8; 6]) -> DhcpPacket {
    let mut packet = DhcpPacket::new(BOOTREQUEST, xid, mac);
    if options.broadcast {
        packet.flags = BROADCAST_FLAG;
    }
    apply_client_options(options, &mut packet, mac);
    if let Some(request_ip) = options.request_ip {
        packet.set_option(OPTION_REQUESTED_IP, encode_ipv4(request_ip));
    }
    if !options.request_options.is_empty() {
        packet.set_option(
            OPTION_PARAMETER_REQUEST_LIST,
            options.request_options.clone(),
        );
    }
    packet.set_option(OPTION_MESSAGE_TYPE, vec![MESSAGE_DISCOVER]);
    packet
}

fn build_request(
    options: &Options,
    xid: u32,
    mac: [u8; 6],
    requested_ip: Ipv4Addr,
    server_id: Ipv4Addr,
) -> DhcpPacket {
    let mut packet = DhcpPacket::new(BOOTREQUEST, xid, mac);
    if options.broadcast {
        packet.flags = BROADCAST_FLAG;
    }
    apply_client_options(options, &mut packet, mac);
    packet.set_option(OPTION_REQUESTED_IP, encode_ipv4(requested_ip));
    packet.set_option(OPTION_SERVER_ID, encode_ipv4(server_id));
    packet.set_option(OPTION_MESSAGE_TYPE, vec![MESSAGE_REQUEST]);
    packet
}

fn offered_address_is_in_use(options: &Options, mac: [u8; 6], address: Ipv4Addr) -> bool {
    let Some(timeout_ms) = options.arp_probe_timeout_ms else {
        return false;
    };
    probe_arp_conflict(&options.interface, address, mac, timeout_ms).unwrap_or(false)
}

fn apply_client_options(options: &Options, packet: &mut DhcpPacket, mac: [u8; 6]) {
    for (code, value) in &options.send_options {
        packet.set_option(*code, value.clone());
    }
    if !options.omit_client_id && !packet.options.contains_key(&OPTION_CLIENT_ID) {
        let mut value = Vec::with_capacity(7);
        value.push(1);
        value.extend_from_slice(&mac);
        packet.set_option(OPTION_CLIENT_ID, value);
    }
}

fn wait_for_reply(
    socket: &UdpSocket,
    xid: u32,
    mac: [u8; 6],
    message_types: &[u8],
) -> Result<Option<(DhcpPacket, SocketAddr)>, Vec<AppletError>> {
    loop {
        let Some((packet, from)) = receive_packet(socket).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])? else {
            return Ok(None);
        };
        if packet.op != 2 || packet.xid != xid || packet.mac() != mac {
            continue;
        }
        if packet
            .message_type()
            .is_some_and(|message_type| message_types.contains(&message_type))
        {
            return Ok(Some((packet, from)));
        }
    }
}

fn lease_from_ack(packet: &DhcpPacket, from: SocketAddr) -> Result<LeaseInfo, Vec<AppletError>> {
    let ip = nonzero_ip(packet.yiaddr)
        .ok_or_else(|| vec![AppletError::new(APPLET, "DHCPACK missing lease address")])?;
    let server_id = packet
        .option(OPTION_SERVER_ID)
        .and_then(decode_ipv4)
        .or_else(|| ipv4_from_socket_addr(from))
        .ok_or_else(|| vec![AppletError::new(APPLET, "DHCPACK missing server identifier")])?;
    Ok(LeaseInfo {
        ip,
        server_id,
        server_addr: SocketAddr::V4(SocketAddrV4::new(server_id, server_port())),
        subnet: packet.option(OPTION_SUBNET_MASK).and_then(decode_ipv4),
        routers: packet
            .option(OPTION_ROUTER)
            .and_then(decode_ipv4_list)
            .unwrap_or_default(),
        dns: packet
            .option(OPTION_DNS)
            .and_then(decode_ipv4_list)
            .unwrap_or_default(),
        lease_seconds: packet
            .option(OPTION_LEASE_TIME)
            .and_then(decode_u32)
            .unwrap_or(600),
        renewal_seconds: packet.option(OPTION_RENEWAL_TIME).and_then(decode_u32),
        rebinding_seconds: packet.option(OPTION_REBINDING_TIME).and_then(decode_u32),
        siaddr: nonzero_ip(packet.siaddr),
        sname: packet.sname.clone(),
        boot_file: packet.file.clone(),
    })
}

fn run_script(options: &Options, reason: &str, lease: &LeaseInfo) -> Result<(), Vec<AppletError>> {
    let Some(script) = &options.script else {
        return Ok(());
    };
    let mut command = Command::new(script);
    command.arg(reason);
    command.env("interface", &options.interface);
    command.env("ip", lease.ip.to_string());
    command.env("lease", lease.lease_seconds.to_string());
    command.env("serverid", lease.server_id.to_string());
    if let Some(subnet) = lease.subnet {
        command.env("subnet", subnet.to_string());
        command.env("mask", subnet.to_string());
        command.env("broadcast", broadcast_address(lease.ip, subnet).to_string());
    }
    if !lease.routers.is_empty() {
        command.env("router", join_ips(&lease.routers));
    }
    if !lease.dns.is_empty() {
        command.env("dns", join_ips(&lease.dns));
    }
    if let Some(siaddr) = lease.siaddr {
        command.env("siaddr", siaddr.to_string());
    }
    if let Some(sname) = &lease.sname {
        command.env("sname", sname);
    }
    if let Some(boot_file) = &lease.boot_file {
        command.env("boot_file", boot_file);
    }
    if let Some(renewal) = lease.renewal_seconds {
        command.env("renewal", renewal.to_string());
    }
    if let Some(rebinding) = lease.rebinding_seconds {
        command.env("rebind", rebinding.to_string());
    }

    let status = command.status().map_err(|err| {
        vec![AppletError::from_io(APPLET, "executing", Some(script), err)]
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("script exited with status {}", status.code().unwrap_or(1)),
        )])
    }
}

fn parse_extra_option(spec: &str) -> Result<(u8, Vec<u8>), Vec<AppletError>> {
    let Some((name, value)) = spec.split_once(':') else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("invalid DHCP option '{spec}'"),
        )]);
    };
    let Some(code) = option_code(name) else {
        return Err(vec![AppletError::new(
            APPLET,
            format!("unknown DHCP option '{name}'"),
        )]);
    };
    if (name.starts_with("0x") || name.chars().all(|character| character.is_ascii_digit()))
        && value.len() % 2 == 0
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return decode_hex(value).map(|encoded| (code, encoded));
    }
    let encoded = match code {
        OPTION_REQUESTED_IP | OPTION_SERVER_ID | OPTION_SUBNET_MASK => {
            encode_ipv4(parse_ipv4(value)?)
        }
        OPTION_LEASE_TIME | OPTION_RENEWAL_TIME | OPTION_REBINDING_TIME => {
            encode_u32(parse_u32(value, "x")?)
        }
        _ => value.as_bytes().to_vec(),
    };
    Ok((code, encoded))
}

fn parse_ipv4(value: &str) -> Result<Ipv4Addr, Vec<AppletError>> {
    value.parse::<Ipv4Addr>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid IPv4 address '{value}'"),
        )]
    })
}

fn parse_u32(value: &str, option: &str) -> Result<u32, Vec<AppletError>> {
    value.parse::<u32>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid value for -{option}: '{value}'"),
        )]
    })
}

fn decode_hex(text: &str) -> Result<Vec<u8>, Vec<AppletError>> {
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let mut index = 0;
    while index < text.len() {
        let byte = u8::from_str_radix(&text[index..index + 2], 16).map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid hex option value '{text}'"),
            )]
        })?;
        bytes.push(byte);
        index += 2;
    }
    Ok(bytes)
}

fn transaction_id() -> Result<u32, AppletError> {
    let mut bytes = [0_u8; 4];
    if fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_ok()
    {
        return Ok(u32::from_be_bytes(bytes));
    }
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    Ok((duration.as_secs() as u32) ^ duration.subsec_nanos() ^ std::process::id())
}

fn ipv4_from_socket_addr(address: SocketAddr) -> Option<Ipv4Addr> {
    match address {
        SocketAddr::V4(v4) => Some(*v4.ip()),
        SocketAddr::V6(_) => None,
    }
}

fn join_ips(addresses: &[Ipv4Addr]) -> String {
    addresses
        .iter()
        .map(Ipv4Addr::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

fn nonzero_ip(ip: Ipv4Addr) -> Option<Ipv4Addr> {
    (ip != Ipv4Addr::UNSPECIFIED).then_some(ip)
}

fn broadcast_address(ip: Ipv4Addr, subnet: Ipv4Addr) -> Ipv4Addr {
    let ip = u32::from_be_bytes(ip.octets());
    let subnet = u32::from_be_bytes(subnet.octets());
    Ipv4Addr::from((ip | !subnet).to_be_bytes())
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
    use super::{DEFAULT_ARP_TIMEOUT_MILLIS, OPTION_HOSTNAME, parse_args, parse_extra_option};
    use std::path::PathBuf;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_basic_options() {
        let options = parse_args(&args(&[
            "-q",
            "-n",
            "-i",
            "eth1",
            "-s",
            "/tmp/script",
            "-p",
            "/tmp/pid",
            "-r",
            "10.0.0.10",
        ]))
        .unwrap();
        assert!(options.quit_after_lease);
        assert!(options.exit_if_no_lease);
        assert_eq!(options.interface, "eth1");
        assert_eq!(options.script.as_deref(), Some("/tmp/script"));
        assert_eq!(options.pidfile, Some(PathBuf::from("/tmp/pid")));
        assert_eq!(options.request_ip.unwrap().to_string(), "10.0.0.10");
    }

    #[test]
    fn parses_arp_validation_option() {
        let options = parse_args(&args(&["-aq"])).unwrap();
        assert_eq!(options.arp_probe_timeout_ms, Some(DEFAULT_ARP_TIMEOUT_MILLIS));
        assert!(options.quit_after_lease);

        let options = parse_args(&args(&["-a1500"])).unwrap();
        assert_eq!(options.arp_probe_timeout_ms, Some(1500));
    }

    #[test]
    fn parses_named_extra_option() {
        let (code, value) = parse_extra_option("hostname:seed").unwrap();
        assert_eq!(code, OPTION_HOSTNAME);
        assert_eq!(value, b"seed");
    }

    #[test]
    fn parses_hex_extra_option() {
        let (code, value) = parse_extra_option("0x3d:010203").unwrap();
        assert_eq!(code, 0x3d);
        assert_eq!(value, vec![1, 2, 3]);
    }
}
