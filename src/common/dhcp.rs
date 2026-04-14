use std::collections::BTreeMap;
use std::env;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::time::Duration;

use crate::common::error::AppletError;

pub(crate) const DHCP_MAGIC_COOKIE: [u8; 4] = [99, 130, 83, 99];
pub(crate) const DEFAULT_SERVER_PORT: u16 = 67;
pub(crate) const DEFAULT_CLIENT_PORT: u16 = 68;
pub(crate) const DEFAULT_READ_TIMEOUT_SECS: u64 = 3;
pub(crate) const BOOTREQUEST: u8 = 1;
pub(crate) const BOOTREPLY: u8 = 2;

pub(crate) const OPTION_SUBNET_MASK: u8 = 1;
pub(crate) const OPTION_ROUTER: u8 = 3;
pub(crate) const OPTION_DNS: u8 = 6;
pub(crate) const OPTION_HOSTNAME: u8 = 12;
pub(crate) const OPTION_REQUESTED_IP: u8 = 50;
pub(crate) const OPTION_LEASE_TIME: u8 = 51;
pub(crate) const OPTION_MESSAGE_TYPE: u8 = 53;
pub(crate) const OPTION_SERVER_ID: u8 = 54;
pub(crate) const OPTION_PARAMETER_REQUEST_LIST: u8 = 55;
pub(crate) const OPTION_RENEWAL_TIME: u8 = 58;
pub(crate) const OPTION_REBINDING_TIME: u8 = 59;
pub(crate) const OPTION_VENDOR_CLASS_ID: u8 = 60;
pub(crate) const OPTION_CLIENT_ID: u8 = 61;
pub(crate) const OPTION_END: u8 = 255;

pub(crate) const MESSAGE_DISCOVER: u8 = 1;
pub(crate) const MESSAGE_OFFER: u8 = 2;
pub(crate) const MESSAGE_REQUEST: u8 = 3;
pub(crate) const MESSAGE_ACK: u8 = 5;
pub(crate) const MESSAGE_NAK: u8 = 6;
pub(crate) const MESSAGE_RELEASE: u8 = 7;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DhcpPacket {
    pub(crate) op: u8,
    pub(crate) htype: u8,
    pub(crate) hlen: u8,
    pub(crate) hops: u8,
    pub(crate) xid: u32,
    pub(crate) secs: u16,
    pub(crate) flags: u16,
    pub(crate) ciaddr: Ipv4Addr,
    pub(crate) yiaddr: Ipv4Addr,
    pub(crate) siaddr: Ipv4Addr,
    pub(crate) giaddr: Ipv4Addr,
    pub(crate) chaddr: [u8; 16],
    pub(crate) options: BTreeMap<u8, Vec<u8>>,
}

impl DhcpPacket {
    pub(crate) fn new(op: u8, xid: u32, mac: [u8; 6]) -> Self {
        let mut chaddr = [0_u8; 16];
        chaddr[..6].copy_from_slice(&mac);
        Self {
            op,
            htype: 1,
            hlen: 6,
            hops: 0,
            xid,
            secs: 0,
            flags: 0,
            ciaddr: Ipv4Addr::UNSPECIFIED,
            yiaddr: Ipv4Addr::UNSPECIFIED,
            siaddr: Ipv4Addr::UNSPECIFIED,
            giaddr: Ipv4Addr::UNSPECIFIED,
            chaddr,
            options: BTreeMap::new(),
        }
    }

    pub(crate) fn mac(&self) -> [u8; 6] {
        let mut mac = [0_u8; 6];
        mac.copy_from_slice(&self.chaddr[..6]);
        mac
    }

    pub(crate) fn set_option(&mut self, code: u8, value: Vec<u8>) {
        self.options.insert(code, value);
    }

    pub(crate) fn option(&self, code: u8) -> Option<&[u8]> {
        self.options.get(&code).map(Vec::as_slice)
    }

    pub(crate) fn message_type(&self) -> Option<u8> {
        self.option(OPTION_MESSAGE_TYPE)
            .and_then(|value| value.first().copied())
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![0_u8; 240];
        bytes[0] = self.op;
        bytes[1] = self.htype;
        bytes[2] = self.hlen;
        bytes[3] = self.hops;
        bytes[4..8].copy_from_slice(&self.xid.to_be_bytes());
        bytes[8..10].copy_from_slice(&self.secs.to_be_bytes());
        bytes[10..12].copy_from_slice(&self.flags.to_be_bytes());
        bytes[12..16].copy_from_slice(&self.ciaddr.octets());
        bytes[16..20].copy_from_slice(&self.yiaddr.octets());
        bytes[20..24].copy_from_slice(&self.siaddr.octets());
        bytes[24..28].copy_from_slice(&self.giaddr.octets());
        bytes[28..44].copy_from_slice(&self.chaddr);
        bytes[236..240].copy_from_slice(&DHCP_MAGIC_COOKIE);
        for (code, value) in &self.options {
            bytes.push(*code);
            bytes.push(value.len() as u8);
            bytes.extend_from_slice(value);
        }
        bytes.push(OPTION_END);
        bytes
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, AppletError> {
        if bytes.len() < 240 {
            return Err(AppletError::new("dhcp", "short DHCP packet"));
        }
        if bytes[236..240] != DHCP_MAGIC_COOKIE {
            return Err(AppletError::new("dhcp", "invalid DHCP magic cookie"));
        }
        let mut options = BTreeMap::new();
        let mut index = 240;
        while index < bytes.len() {
            let code = bytes[index];
            index += 1;
            if code == 0 {
                continue;
            }
            if code == OPTION_END {
                break;
            }
            if index >= bytes.len() {
                return Err(AppletError::new("dhcp", "truncated DHCP option length"));
            }
            let length = usize::from(bytes[index]);
            index += 1;
            if index + length > bytes.len() {
                return Err(AppletError::new("dhcp", "truncated DHCP option"));
            }
            options.insert(code, bytes[index..index + length].to_vec());
            index += length;
        }

        let mut chaddr = [0_u8; 16];
        chaddr.copy_from_slice(&bytes[28..44]);
        Ok(Self {
            op: bytes[0],
            htype: bytes[1],
            hlen: bytes[2],
            hops: bytes[3],
            xid: u32::from_be_bytes(bytes[4..8].try_into().expect("slice length")),
            secs: u16::from_be_bytes(bytes[8..10].try_into().expect("slice length")),
            flags: u16::from_be_bytes(bytes[10..12].try_into().expect("slice length")),
            ciaddr: Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[12..16]).expect("slice length")),
            yiaddr: Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[16..20]).expect("slice length")),
            siaddr: Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[20..24]).expect("slice length")),
            giaddr: Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[24..28]).expect("slice length")),
            chaddr,
            options,
        })
    }
}

pub(crate) fn encode_ipv4(address: Ipv4Addr) -> Vec<u8> {
    address.octets().to_vec()
}

pub(crate) fn encode_ipv4_list(addresses: &[Ipv4Addr]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(addresses.len() * 4);
    for address in addresses {
        bytes.extend_from_slice(&address.octets());
    }
    bytes
}

pub(crate) fn decode_ipv4(bytes: &[u8]) -> Option<Ipv4Addr> {
    let octets: [u8; 4] = bytes.try_into().ok()?;
    Some(Ipv4Addr::from(octets))
}

pub(crate) fn decode_ipv4_list(bytes: &[u8]) -> Option<Vec<Ipv4Addr>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut addresses = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        addresses.push(Ipv4Addr::from(<[u8; 4]>::try_from(chunk).ok()?));
    }
    Some(addresses)
}

pub(crate) fn encode_u32(value: u32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

pub(crate) fn decode_u32(bytes: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

pub(crate) fn parse_mac(text: &str) -> Result<[u8; 6], AppletError> {
    let parts = text.split([':', '-']).collect::<Vec<_>>();
    if parts.len() != 6 {
        return Err(AppletError::new(
            "dhcp",
            format!("invalid MAC address '{text}'"),
        ));
    }

    let mut mac = [0_u8; 6];
    for (index, part) in parts.iter().enumerate() {
        mac[index] = u8::from_str_radix(part, 16)
            .map_err(|_| AppletError::new("dhcp", format!("invalid MAC address '{text}'")))?;
    }
    Ok(mac)
}

pub(crate) fn format_mac(mac: [u8; 6]) -> String {
    mac.iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(target_os = "linux")]
fn interface_mac_path(interface: &str) -> PathBuf {
    PathBuf::from(format!("/sys/class/net/{interface}/address"))
}

pub(crate) fn default_client_mac(interface: &str) -> Result<[u8; 6], AppletError> {
    if let Some(value) = env::var_os("SEED_DHCP_CLIENT_MAC") {
        return parse_mac(&value.to_string_lossy());
    }

    #[cfg(target_os = "linux")]
    {
        let path = interface_mac_path(interface);
        if let Ok(text) = std::fs::read_to_string(&path) {
            return parse_mac(text.trim());
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = interface;

    let mut mac = [0_u8; 6];
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let mut seed = pid ^ nanos;
    for byte in &mut mac {
        *byte = (seed & 0xff) as u8;
        seed = seed.rotate_left(5) ^ 0xa5a5_5a5a;
    }
    mac[0] = (mac[0] | 0x02) & 0xfe;
    Ok(mac)
}

pub(crate) fn option_code(name: &str) -> Option<u8> {
    match name {
        "subnet" | "subnet-mask" => Some(OPTION_SUBNET_MASK),
        "router" | "routers" | "gateway" => Some(OPTION_ROUTER),
        "dns" | "dns-server" | "domain-server" => Some(OPTION_DNS),
        "hostname" | "host-name" => Some(OPTION_HOSTNAME),
        "requested-ip" | "requestip" => Some(OPTION_REQUESTED_IP),
        "lease" | "lease-time" => Some(OPTION_LEASE_TIME),
        "message-type" | "message" => Some(OPTION_MESSAGE_TYPE),
        "serverid" | "server-id" | "server-identifier" => Some(OPTION_SERVER_ID),
        "param-request-list" | "parameter-request-list" => Some(OPTION_PARAMETER_REQUEST_LIST),
        "renewal-time" | "t1" => Some(OPTION_RENEWAL_TIME),
        "rebinding-time" | "t2" => Some(OPTION_REBINDING_TIME),
        "vendor" | "vendor-class" | "vendor-class-id" => Some(OPTION_VENDOR_CLASS_ID),
        "clientid" | "client-id" => Some(OPTION_CLIENT_ID),
        _ => {
            if let Some(value) = name.strip_prefix("0x") {
                u8::from_str_radix(value, 16).ok()
            } else {
                name.parse::<u8>().ok()
            }
        }
    }
}

pub(crate) fn encode_option_value(code: u8, values: &[&str]) -> Result<Vec<u8>, AppletError> {
    match code {
        OPTION_SUBNET_MASK | OPTION_REQUESTED_IP | OPTION_SERVER_ID => {
            if values.len() != 1 {
                return Err(AppletError::new(
                    "dhcp",
                    "expected exactly one IPv4 address",
                ));
            }
            let address = values[0].parse::<Ipv4Addr>().map_err(|_| {
                AppletError::new("dhcp", format!("invalid IPv4 address '{}'", values[0]))
            })?;
            Ok(encode_ipv4(address))
        }
        OPTION_ROUTER | OPTION_DNS => {
            let mut addresses = Vec::with_capacity(values.len());
            for value in values {
                let address = value.parse::<Ipv4Addr>().map_err(|_| {
                    AppletError::new("dhcp", format!("invalid IPv4 address '{value}'"))
                })?;
                addresses.push(address);
            }
            Ok(encode_ipv4_list(&addresses))
        }
        OPTION_LEASE_TIME | OPTION_RENEWAL_TIME | OPTION_REBINDING_TIME => {
            if values.len() != 1 {
                return Err(AppletError::new(
                    "dhcp",
                    "expected exactly one integer value",
                ));
            }
            let value = values[0].parse::<u32>().map_err(|_| {
                AppletError::new("dhcp", format!("invalid integer '{}'", values[0]))
            })?;
            Ok(encode_u32(value))
        }
        _ => {
            if values.len() != 1 {
                return Err(AppletError::new(
                    "dhcp",
                    "expected exactly one option value",
                ));
            }
            let value = values[0];
            if let Some(hex) = value.strip_prefix("0x") {
                return decode_hex(hex);
            }
            Ok(value.as_bytes().to_vec())
        }
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>, AppletError> {
    if !text.len().is_multiple_of(2) {
        return Err(AppletError::new(
            "dhcp",
            format!("invalid hex value '{text}'"),
        ));
    }
    let mut bytes = Vec::with_capacity(text.len() / 2);
    let mut index = 0;
    while index < text.len() {
        let byte = u8::from_str_radix(&text[index..index + 2], 16)
            .map_err(|_| AppletError::new("dhcp", format!("invalid hex value '{text}'")))?;
        bytes.push(byte);
        index += 2;
    }
    Ok(bytes)
}

pub(crate) fn default_server_identifier() -> Result<Ipv4Addr, AppletError> {
    if let Some(value) = env::var_os("SEED_DHCP_SERVER_IP") {
        let value = value.to_string_lossy();
        return value
            .parse::<Ipv4Addr>()
            .map_err(|_| AppletError::new("dhcp", format!("invalid server IP '{value}'")));
    }
    Ok(Ipv4Addr::new(127, 0, 0, 1))
}

pub(crate) fn default_server_addr() -> Result<SocketAddr, AppletError> {
    if let Some(value) = env::var_os("SEED_DHCP_SERVER_ADDR") {
        let value = value.to_string_lossy();
        return value
            .parse::<SocketAddr>()
            .or_else(|_| {
                (value.as_ref(), server_port())
                    .to_socket_addrs()
                    .map_err(|_| ())
                    .and_then(|mut addrs| addrs.next().ok_or(()))
            })
            .map_err(|_| AppletError::new("dhcp", format!("invalid server address '{value}'")));
    }
    Ok(SocketAddr::V4(SocketAddrV4::new(
        Ipv4Addr::BROADCAST,
        server_port(),
    )))
}

pub(crate) fn server_port() -> u16 {
    env::var("SEED_DHCP_SERVER_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_SERVER_PORT)
}

pub(crate) fn client_port() -> u16 {
    env::var("SEED_DHCP_CLIENT_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_CLIENT_PORT)
}

pub(crate) fn prepare_socket(socket: &UdpSocket) -> io::Result<()> {
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_secs(DEFAULT_READ_TIMEOUT_SECS)))?;
    Ok(())
}

pub(crate) fn receive_packet(
    socket: &UdpSocket,
) -> Result<Option<(DhcpPacket, SocketAddr)>, AppletError> {
    let mut buffer = [0_u8; 1500];
    match socket.recv_from(&mut buffer) {
        Ok((read, from)) => DhcpPacket::decode(&buffer[..read])
            .map(|packet| Some((packet, from)))
            .map_err(|err| AppletError::new("dhcp", err.to_string())),
        Err(err)
            if matches!(
                err.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
            ) =>
        {
            Ok(None)
        }
        Err(err) => Err(AppletError::from_io("dhcp", "receiving", None, err)),
    }
}

pub(crate) fn send_packet(
    socket: &UdpSocket,
    packet: &DhcpPacket,
    target: SocketAddr,
) -> Result<(), AppletError> {
    socket
        .send_to(&packet.encode(), target)
        .map(|_| ())
        .map_err(|err| AppletError::from_io("dhcp", "sending", None, err))
}

#[cfg(test)]
mod tests {
    use super::{
        DhcpPacket, MESSAGE_DISCOVER, OPTION_DNS, OPTION_HOSTNAME, OPTION_MESSAGE_TYPE,
        decode_ipv4, decode_ipv4_list, default_client_mac, encode_ipv4, encode_ipv4_list,
        encode_option_value, format_mac, option_code, parse_mac,
    };
    use std::net::Ipv4Addr;

    #[test]
    fn packet_round_trip() {
        let mut packet = DhcpPacket::new(1, 0x1234_5678, [1, 2, 3, 4, 5, 6]);
        packet.set_option(OPTION_MESSAGE_TYPE, vec![MESSAGE_DISCOVER]);
        packet.set_option(OPTION_HOSTNAME, b"seed".to_vec());
        let decoded = DhcpPacket::decode(&packet.encode()).unwrap();
        assert_eq!(decoded.xid, 0x1234_5678);
        assert_eq!(decoded.mac(), [1, 2, 3, 4, 5, 6]);
        assert_eq!(decoded.message_type(), Some(MESSAGE_DISCOVER));
        assert_eq!(decoded.option(OPTION_HOSTNAME), Some("seed".as_bytes()));
    }

    #[test]
    fn ipv4_helpers_round_trip() {
        let address = Ipv4Addr::new(10, 0, 0, 1);
        assert_eq!(decode_ipv4(&encode_ipv4(address)), Some(address));
    }

    #[test]
    fn ipv4_list_helpers_round_trip() {
        let addresses = [Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)];
        assert_eq!(
            decode_ipv4_list(&encode_ipv4_list(&addresses)),
            Some(addresses.to_vec())
        );
    }

    #[test]
    fn parses_and_formats_mac_addresses() {
        let mac = parse_mac("02:00:de:ad:be:ef").expect("parse MAC");
        assert_eq!(format_mac(mac), "02:00:de:ad:be:ef");
    }

    #[test]
    fn encodes_known_option_values() {
        assert_eq!(option_code("dns"), Some(OPTION_DNS));
        assert_eq!(
            decode_ipv4_list(&encode_option_value(OPTION_DNS, &["1.1.1.1", "8.8.8.8"]).unwrap()),
            Some(vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(8, 8, 8, 8)])
        );
    }

    #[test]
    fn fallback_client_mac_is_locally_administered() {
        let mac = default_client_mac("missing0").expect("default MAC");
        assert_eq!(mac[0] & 0x02, 0x02);
        assert_eq!(mac[0] & 0x01, 0);
    }
}
