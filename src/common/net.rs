#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

#[cfg(target_os = "linux")]
use std::ffi::{CStr, CString};
#[cfg(target_os = "linux")]
use std::fs;
use std::io;
#[cfg(target_os = "linux")]
use std::mem;
#[cfg(target_os = "linux")]
use std::net::Ipv6Addr;
use std::net::{IpAddr, Ipv4Addr};
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod state;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum AddressFamily {
    Inet4,
    Inet6,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InterfaceAddress {
    pub(crate) family: AddressFamily,
    pub(crate) address: String,
    pub(crate) prefix_len: u8,
    pub(crate) peer: Option<String>,
    pub(crate) broadcast: Option<String>,
    pub(crate) label: Option<String>,
    pub(crate) scope: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct InterfaceStats {
    pub(crate) rx_bytes: u64,
    pub(crate) rx_packets: u64,
    pub(crate) tx_bytes: u64,
    pub(crate) tx_packets: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InterfaceInfo {
    pub(crate) index: u32,
    pub(crate) name: String,
    pub(crate) flags: u32,
    pub(crate) mtu: u32,
    pub(crate) metric: u32,
    pub(crate) tx_queue_len: u32,
    pub(crate) mem_start: u64,
    pub(crate) io_addr: u16,
    pub(crate) irq: u8,
    pub(crate) mac: Option<String>,
    pub(crate) addresses: Vec<InterfaceAddress>,
    pub(crate) stats: InterfaceStats,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RouteInfo {
    pub(crate) family: AddressFamily,
    pub(crate) destination: String,
    pub(crate) prefix_len: u8,
    pub(crate) gateway: Option<String>,
    pub(crate) dev: String,
    pub(crate) source: Option<String>,
    pub(crate) metric: Option<u32>,
    pub(crate) table: Option<u32>,
    pub(crate) protocol: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NeighborInfo {
    pub(crate) family: AddressFamily,
    pub(crate) address: String,
    pub(crate) dev: String,
    pub(crate) lladdr: Option<String>,
    pub(crate) state: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuleInfo {
    pub(crate) family: AddressFamily,
    pub(crate) from: Option<String>,
    pub(crate) from_prefix_len: u8,
    pub(crate) to: Option<String>,
    pub(crate) to_prefix_len: u8,
    pub(crate) iif: Option<String>,
    pub(crate) oif: Option<String>,
    pub(crate) fwmark: Option<u32>,
    pub(crate) priority: Option<u32>,
    pub(crate) table: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LinkChange {
    pub(crate) up: Option<bool>,
    pub(crate) mtu: Option<u32>,
    pub(crate) metric: Option<u32>,
    pub(crate) tx_queue_len: Option<u32>,
    pub(crate) address: Option<String>,
    pub(crate) new_name: Option<String>,
    pub(crate) arp: Option<bool>,
    pub(crate) multicast: Option<bool>,
    pub(crate) allmulti: Option<bool>,
    pub(crate) promisc: Option<bool>,
    pub(crate) dynamic: Option<bool>,
    pub(crate) trailers: Option<bool>,
    pub(crate) mem_start: Option<u64>,
    pub(crate) io_addr: Option<u16>,
    pub(crate) irq: Option<u8>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Ipv4Change {
    pub(crate) address: Option<Ipv4Addr>,
    pub(crate) prefix_len: Option<u8>,
    pub(crate) broadcast: Option<Option<Ipv4Addr>>,
    pub(crate) peer: Option<Ipv4Addr>,
}

#[cfg(target_os = "linux")]
enum Backend {
    Live,
    State(PathBuf),
}

#[cfg(target_os = "linux")]
fn backend() -> Backend {
    state_path().map_or(Backend::Live, Backend::State)
}

#[cfg(target_os = "linux")]
pub(crate) fn add_address(name: &str, address: &InterfaceAddress) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::add_address(&path, name, address),
        Backend::Live => match address.family {
            AddressFamily::Inet4 => live_add_ipv4(name, address),
            AddressFamily::Inet6 => live_add_ipv6(name, address),
        },
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn remove_address(name: &str, family: AddressFamily, address: &str) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::remove_address(&path, name, family, address),
        Backend::Live => match family {
            AddressFamily::Inet4 => live_remove_ipv4(name, address),
            AddressFamily::Inet6 => live_remove_ipv6(name, address),
        },
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn list_interfaces() -> io::Result<Vec<InterfaceInfo>> {
    match backend() {
        Backend::State(path) => state::list_interfaces(&path),
        Backend::Live => live_list_interfaces(),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn list_routes() -> io::Result<Vec<RouteInfo>> {
    match backend() {
        Backend::State(path) => state::list_routes(&path),
        Backend::Live => live_list_routes(),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn list_neighbors() -> io::Result<Vec<NeighborInfo>> {
    match backend() {
        Backend::State(path) => state::list_neighbors(&path),
        Backend::Live => live_list_neighbors(),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn list_rules() -> io::Result<Vec<RuleInfo>> {
    match backend() {
        Backend::State(path) => state::list_rules(&path),
        Backend::Live => live_list_rules(),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn del_neighbor(neighbor: &NeighborInfo) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::del_neighbor(&path, neighbor),
        Backend::Live => live_del_neighbor(neighbor),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn add_rule(rule: &RuleInfo) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::add_rule(&path, rule),
        Backend::Live => live_rule_change(rule, libc::RTM_NEWRULE),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn del_rule(rule: &RuleInfo) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::del_rule(&path, rule),
        Backend::Live => live_rule_change(rule, libc::RTM_DELRULE),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn apply_link_change(name: &str, change: &LinkChange) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::apply_link_change(&path, name, change),
        Backend::Live => live_apply_link_change(name, change),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn set_ipv4(name: &str, change: &Ipv4Change) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::set_ipv4(&path, name, change),
        Backend::Live => live_set_ipv4(name, change),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn add_route(route: &RouteInfo) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::add_route(&path, route),
        Backend::Live => live_route_change(route, libc::SIOCADDRT),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn del_route(route: &RouteInfo) -> io::Result<()> {
    match backend() {
        Backend::State(path) => state::del_route(&path, route),
        Backend::Live => live_route_change(route, libc::SIOCDELRT),
    }
}

pub(crate) fn parse_prefix(spec: &str) -> io::Result<(Ipv4Addr, u8)> {
    let (address, prefix) = spec.split_once('/').unwrap_or((spec, "32"));
    let address = address.parse::<Ipv4Addr>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid IPv4 address '{address}'"),
        )
    })?;
    let prefix = prefix
        .parse::<u8>()
        .ok()
        .filter(|prefix| *prefix <= 32)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid prefix '{prefix}'"),
            )
        })?;
    Ok((address, prefix))
}

pub(crate) fn prefix_to_netmask(prefix_len: u8) -> Ipv4Addr {
    let value = if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len)
    };
    Ipv4Addr::from(value)
}

pub(crate) fn broadcast_for(address: Ipv4Addr, prefix_len: u8) -> Ipv4Addr {
    let mask = u32::from(prefix_to_netmask(prefix_len));
    Ipv4Addr::from(u32::from(address) | !mask)
}

fn parse_scope_value(scope: Option<&str>) -> io::Result<u8> {
    match scope.unwrap_or("global") {
        "host" => Ok(libc::RT_SCOPE_HOST),
        "link" => Ok(libc::RT_SCOPE_LINK),
        "global" => Ok(libc::RT_SCOPE_UNIVERSE),
        value => value.parse::<u8>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid scope '{value}'"),
            )
        }),
    }
}

#[cfg(target_os = "linux")]
fn ipv4_scope(address: Ipv4Addr, flags: u32) -> &'static str {
    if address.is_loopback() || flags & libc::IFF_LOOPBACK as u32 != 0 {
        "host"
    } else if address.is_link_local() {
        "link"
    } else {
        "global"
    }
}

#[cfg(target_os = "linux")]
fn ipv6_scope(address: Ipv6Addr) -> &'static str {
    if address.is_loopback() {
        "host"
    } else if address.is_unicast_link_local() {
        "link"
    } else {
        "global"
    }
}

pub(crate) fn format_mac(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub(crate) fn parse_mac(value: &str) -> io::Result<[u8; 6]> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 6 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid MAC address '{value}'"),
        ));
    }
    let mut bytes = [0_u8; 6];
    for (index, part) in parts.iter().enumerate() {
        bytes[index] = u8::from_str_radix(part, 16).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid MAC address '{value}'"),
            )
        })?;
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
pub(crate) fn interface_flag_names(flags: u32) -> Vec<&'static str> {
    let mut names = Vec::new();
    if flags & libc::IFF_UP as u32 != 0 {
        names.push("UP");
    }
    if flags & libc::IFF_BROADCAST as u32 != 0 {
        names.push("BROADCAST");
    }
    if flags & libc::IFF_LOOPBACK as u32 != 0 {
        names.push("LOOPBACK");
    }
    if flags & libc::IFF_RUNNING as u32 != 0 {
        names.push("RUNNING");
    }
    if flags & libc::IFF_MULTICAST as u32 != 0 {
        names.push("MULTICAST");
    }
    if flags & libc::IFF_ALLMULTI as u32 != 0 {
        names.push("ALLMULTI");
    }
    if flags & libc::IFF_PROMISC as u32 != 0 {
        names.push("PROMISC");
    }
    if flags & libc::IFF_NOARP as u32 != 0 {
        names.push("NOARP");
    }
    if flags & libc::IFF_DYNAMIC as u32 != 0 {
        names.push("DYNAMIC");
    }
    if flags & libc::IFF_NOTRAILERS as u32 != 0 {
        names.push("NOTRAILERS");
    }
    if flags & libc::IFF_POINTOPOINT as u32 != 0 {
        names.push("POINTOPOINT");
    }
    names
}

#[cfg(target_os = "linux")]
fn state_path() -> Option<PathBuf> {
    std::env::var_os("SEED_NET_STATE").map(PathBuf::from)
}

#[cfg(target_os = "linux")]
fn live_list_interfaces() -> io::Result<Vec<InterfaceInfo>> {
    let mut interfaces = read_sys_interfaces()?;
    let addresses = live_addresses()?;
    for (name, addresses) in addresses {
        if let Some(interface) = interfaces
            .iter_mut()
            .find(|interface| interface.name == name)
        {
            interface.addresses = addresses;
        }
    }
    interfaces.sort_by(|left, right| {
        left.index
            .cmp(&right.index)
            .then(left.name.cmp(&right.name))
    });
    Ok(interfaces)
}

#[cfg(target_os = "linux")]
fn read_sys_interfaces() -> io::Result<Vec<InterfaceInfo>> {
    let mut interfaces = Vec::new();
    for entry in fs::read_dir("/sys/class/net")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let index = fs::read_to_string(path.join("ifindex"))
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0);
        let flags = read_sys_u32(&path.join("flags")).unwrap_or(0);
        let mtu = read_sys_u32(&path.join("mtu")).unwrap_or(0);
        let tx_queue_len = read_sys_u32(&path.join("tx_queue_len")).unwrap_or(0);
        let (mem_start, io_addr, irq) = get_ifmap(&name).unwrap_or((0, 0, 0));
        let mac = fs::read_to_string(path.join("address"))
            .ok()
            .map(|value| value.trim().to_string());
        let stats = InterfaceStats {
            rx_bytes: read_sys_u64(&path.join("statistics/rx_bytes")).unwrap_or(0),
            rx_packets: read_sys_u64(&path.join("statistics/rx_packets")).unwrap_or(0),
            tx_bytes: read_sys_u64(&path.join("statistics/tx_bytes")).unwrap_or(0),
            tx_packets: read_sys_u64(&path.join("statistics/tx_packets")).unwrap_or(0),
        };
        interfaces.push(InterfaceInfo {
            index,
            name,
            flags,
            mtu,
            metric: get_metric(
                path.file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(""),
            )
            .unwrap_or(0),
            tx_queue_len,
            mem_start,
            io_addr,
            irq,
            mac,
            addresses: Vec::new(),
            stats,
        });
    }
    Ok(interfaces)
}

#[cfg(target_os = "linux")]
fn read_sys_u32(path: &Path) -> Option<u32> {
    let value = fs::read_to_string(path).ok()?;
    let trimmed = value.trim();
    trimmed.strip_prefix("0x").map_or_else(
        || trimmed.parse().ok(),
        |hex| u32::from_str_radix(hex, 16).ok(),
    )
}

#[cfg(target_os = "linux")]
fn read_sys_u64(path: &Path) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn live_addresses() -> io::Result<Vec<(String, Vec<InterfaceAddress>)>> {
    let mut map = std::collections::BTreeMap::<String, Vec<InterfaceAddress>>::new();
    let mut addrs = std::ptr::null_mut::<libc::ifaddrs>();
    // SAFETY: `addrs` is a valid out-pointer for getifaddrs.
    if unsafe { libc::getifaddrs(&mut addrs) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut cursor = addrs;
    while !cursor.is_null() {
        // SAFETY: `cursor` is a valid node from getifaddrs until freeifaddrs.
        let ifa = unsafe { &*cursor };
        if !ifa.ifa_addr.is_null() {
            let family = unsafe { (*ifa.ifa_addr).sa_family as i32 };
            let name = unsafe { CStr::from_ptr(ifa.ifa_name) }
                .to_string_lossy()
                .into_owned();
            if let Some(address) = convert_ifaddr(ifa, family) {
                map.entry(name).or_default().push(address);
            }
        }
        // SAFETY: advances along the linked list returned by getifaddrs.
        cursor = unsafe { (*cursor).ifa_next };
    }
    // SAFETY: `addrs` came from getifaddrs and has not been freed yet.
    unsafe { libc::freeifaddrs(addrs) };
    Ok(map.into_iter().collect())
}

#[cfg(target_os = "linux")]
fn convert_ifaddr(ifa: &libc::ifaddrs, family: i32) -> Option<InterfaceAddress> {
    match family {
        libc::AF_INET => {
            let address = ipv4_from_sockaddr(ifa.ifa_addr)?;
            let prefix_len = ipv4_prefix_len(ifa.ifa_netmask)?;
            let peer_or_broadcast = if ifa.ifa_ifu.is_null() {
                None
            } else {
                ipv4_from_sockaddr(ifa.ifa_ifu as *const libc::sockaddr)
            };
            let flags = ifa.ifa_flags;
            Some(InterfaceAddress {
                family: AddressFamily::Inet4,
                address: address.to_string(),
                prefix_len,
                peer: (flags & libc::IFF_POINTOPOINT as u32 != 0)
                    .then(|| peer_or_broadcast.map(|value| value.to_string()))
                    .flatten(),
                broadcast: (flags & libc::IFF_BROADCAST as u32 != 0)
                    .then(|| peer_or_broadcast.map(|value| value.to_string()))
                    .flatten(),
                label: None,
                scope: Some(ipv4_scope(address, flags).to_string()),
            })
        }
        libc::AF_INET6 => {
            let address = ipv6_from_sockaddr(ifa.ifa_addr)?;
            let prefix_len = ipv6_prefix_len(ifa.ifa_netmask)?;
            Some(InterfaceAddress {
                family: AddressFamily::Inet6,
                address: address.to_string(),
                prefix_len,
                peer: None,
                broadcast: None,
                label: None,
                scope: Some(ipv6_scope(address).to_string()),
            })
        }
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn ipv4_from_sockaddr(addr: *const libc::sockaddr) -> Option<Ipv4Addr> {
    if addr.is_null() {
        return None;
    }
    // SAFETY: caller ensures `addr` points to a sockaddr_in for AF_INET.
    let addr = unsafe { &*(addr as *const libc::sockaddr_in) };
    Some(Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr)))
}

#[cfg(target_os = "linux")]
fn ipv6_from_sockaddr(addr: *const libc::sockaddr) -> Option<Ipv6Addr> {
    if addr.is_null() {
        return None;
    }
    // SAFETY: caller ensures `addr` points to a sockaddr_in6 for AF_INET6.
    let addr = unsafe { &*(addr as *const libc::sockaddr_in6) };
    Some(Ipv6Addr::from(addr.sin6_addr.s6_addr))
}

#[cfg(target_os = "linux")]
fn ipv4_prefix_len(addr: *const libc::sockaddr) -> Option<u8> {
    let mask = ipv4_from_sockaddr(addr)?;
    Some(u32::from(mask).count_ones() as u8)
}

#[cfg(target_os = "linux")]
fn ipv6_prefix_len(addr: *const libc::sockaddr) -> Option<u8> {
    let mask = ipv6_from_sockaddr(addr)?;
    Some(
        mask.octets()
            .iter()
            .map(|byte| byte.count_ones())
            .sum::<u32>() as u8,
    )
}

#[cfg(target_os = "linux")]
fn live_list_routes() -> io::Result<Vec<RouteInfo>> {
    let mut routes = dump_routes(libc::AF_INET as u8).or_else(|_| parse_ipv4_routes())?;
    routes.extend(dump_routes(libc::AF_INET6 as u8).or_else(|_| parse_ipv6_routes())?);
    routes.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.table.cmp(&right.table))
            .then(left.destination.cmp(&right.destination))
            .then(left.prefix_len.cmp(&right.prefix_len))
            .then(left.dev.cmp(&right.dev))
    });
    Ok(routes)
}

#[cfg(target_os = "linux")]
fn dump_routes(family: u8) -> io::Result<Vec<RouteInfo>> {
    let fd = rtnetlink_socket_fd()?;
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &RtMsg {
            rtm_family: family,
            rtm_dst_len: 0,
            rtm_src_len: 0,
            rtm_tos: 0,
            rtm_table: libc::RT_TABLE_UNSPEC,
            rtm_protocol: libc::RTPROT_UNSPEC,
            rtm_scope: libc::RT_SCOPE_UNIVERSE,
            rtm_type: libc::RTN_UNSPEC,
            rtm_flags: 0,
        },
    );

    let mut message = Vec::with_capacity(mem::size_of::<libc::nlmsghdr>() + payload.len());
    append_bytes(
        &mut message,
        &libc::nlmsghdr {
            nlmsg_len: (mem::size_of::<libc::nlmsghdr>() + payload.len()) as u32,
            nlmsg_type: libc::RTM_GETROUTE,
            nlmsg_flags: (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16,
            nlmsg_seq: 1,
            nlmsg_pid: 0,
        },
    );
    message.extend_from_slice(&payload);

    let mut kernel = unsafe { mem::zeroed::<libc::sockaddr_nl>() };
    kernel.nl_family = libc::AF_NETLINK as libc::sa_family_t;

    // SAFETY: buffer and sockaddr point to initialized memory for sendto.
    let rc = unsafe {
        libc::sendto(
            fd,
            message.as_ptr() as *const libc::c_void,
            message.len(),
            0,
            &kernel as *const libc::sockaddr_nl as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        let err = io::Error::last_os_error();
        close_fd(fd);
        return Err(err);
    }

    let mut routes = Vec::new();
    let mut buffer = vec![0_u8; 8192];
    loop {
        // SAFETY: `buffer` is valid writable memory for recv.
        let len = unsafe {
            libc::recv(
                fd,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
                0,
            )
        };
        if len < 0 {
            let err = io::Error::last_os_error();
            close_fd(fd);
            return Err(err);
        }
        if len == 0 {
            close_fd(fd);
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short netlink reply",
            ));
        }
        if parse_route_dump_chunk(&buffer[..len as usize], &mut routes)? {
            break;
        }
    }
    close_fd(fd);
    Ok(routes)
}

#[cfg(target_os = "linux")]
fn parse_route_dump_chunk(buffer: &[u8], routes: &mut Vec<RouteInfo>) -> io::Result<bool> {
    let header_len = mem::size_of::<libc::nlmsghdr>();
    let error_len = mem::size_of::<libc::nlmsgerr>();
    let mut offset = 0;
    while offset + header_len <= buffer.len() {
        // SAFETY: bounds checked above and aligned access is fine for nlmsghdr.
        let header = unsafe { &*(buffer[offset..].as_ptr() as *const libc::nlmsghdr) };
        if header.nlmsg_len < header_len as u32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed netlink reply",
            ));
        }
        let message_end = offset + header.nlmsg_len as usize;
        if message_end > buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated netlink reply",
            ));
        }
        match header.nlmsg_type {
            value if value == libc::NLMSG_DONE as u16 => return Ok(true),
            value if value == libc::NLMSG_ERROR as u16 => {
                if header.nlmsg_len < (header_len + error_len) as u32 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "truncated netlink error",
                    ));
                }
                // SAFETY: bounds checked above for nlmsgerr payload.
                let error =
                    unsafe { &*(buffer[offset + header_len..].as_ptr() as *const libc::nlmsgerr) };
                if error.error != 0 {
                    return Err(io::Error::from_raw_os_error(-error.error));
                }
            }
            value if value == libc::RTM_NEWROUTE => {
                if let Some(route) = parse_route_message(&buffer[offset + header_len..message_end])?
                {
                    routes.push(route);
                }
            }
            _ => {}
        }
        offset += nlmsg_align(header.nlmsg_len as usize);
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
fn parse_route_message(payload: &[u8]) -> io::Result<Option<RouteInfo>> {
    let msg_len = mem::size_of::<RtMsg>();
    if payload.len() < msg_len {
        return Ok(None);
    }
    // SAFETY: bounds checked above and RtMsg is POD.
    let message = unsafe { &*(payload.as_ptr() as *const RtMsg) };
    let family = match message.rtm_family as i32 {
        libc::AF_INET => AddressFamily::Inet4,
        libc::AF_INET6 => AddressFamily::Inet6,
        _ => return Ok(None),
    };
    if message.rtm_type != libc::RTN_UNICAST {
        return Ok(None);
    }

    let mut destination = match family {
        AddressFamily::Inet4 => Ipv4Addr::UNSPECIFIED.to_string(),
        AddressFamily::Inet6 => Ipv6Addr::UNSPECIFIED.to_string(),
    };
    let mut gateway = None;
    let mut dev = None;
    let mut source = None;
    let mut metric = None;
    let mut table = Some(message.rtm_table as u32);
    let protocol = Some(message.rtm_protocol);
    let mut offset = msg_len;
    let attr_len = mem::size_of::<RtAttr>();
    while offset + attr_len <= payload.len() {
        // SAFETY: bounds checked above and RtAttr is POD.
        let attr = unsafe { &*(payload[offset..].as_ptr() as *const RtAttr) };
        if attr.rta_len < attr_len as u16 {
            break;
        }
        let end = offset + attr.rta_len as usize;
        if end > payload.len() {
            break;
        }
        let data = &payload[offset + attr_len..end];
        match attr.rta_type {
            value if value == libc::RTA_DST => {
                destination = parse_route_address(family, data)?.to_string();
            }
            value if value == libc::RTA_GATEWAY => {
                gateway = Some(parse_route_address(family, data)?.to_string());
            }
            value if value == libc::RTA_PREFSRC => {
                source = Some(parse_route_address(family, data)?.to_string());
            }
            value if value == libc::RTA_PRIORITY && data.len() >= 4 => {
                metric = Some(u32::from_ne_bytes(data[..4].try_into().unwrap_or([0; 4])));
            }
            value if value == libc::RTA_OIF && data.len() >= 4 => {
                let index = u32::from_ne_bytes(data[..4].try_into().unwrap_or([0; 4]));
                dev = Some(if_name(index)?);
            }
            value if value == libc::RTA_TABLE && data.len() >= 4 => {
                table = Some(u32::from_ne_bytes(data[..4].try_into().unwrap_or([0; 4])));
            }
            _ => {}
        }
        offset += nlmsg_align(attr.rta_len as usize);
    }

    let Some(dev) = dev else {
        return Ok(None);
    };
    Ok(Some(RouteInfo {
        family,
        destination,
        prefix_len: message.rtm_dst_len,
        gateway,
        dev,
        source,
        metric,
        table,
        protocol,
    }))
}

#[cfg(target_os = "linux")]
fn parse_route_address(family: AddressFamily, data: &[u8]) -> io::Result<IpAddr> {
    match family {
        AddressFamily::Inet4 if data.len() >= 4 => Ok(IpAddr::V4(Ipv4Addr::from(
            <[u8; 4]>::try_from(&data[..4]).unwrap_or([0; 4]),
        ))),
        AddressFamily::Inet6 if data.len() >= 16 => Ok(IpAddr::V6(Ipv6Addr::from(
            <[u8; 16]>::try_from(&data[..16]).unwrap_or([0; 16]),
        ))),
        AddressFamily::Inet4 => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "short IPv4 route attribute",
        )),
        AddressFamily::Inet6 => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "short IPv6 route attribute",
        )),
    }
}

#[cfg(target_os = "linux")]
fn if_name(index: u32) -> io::Result<String> {
    let mut buffer = [0; libc::IFNAMSIZ];
    // SAFETY: buffer points to writable memory for if_indextoname.
    let name = unsafe { libc::if_indextoname(index, buffer.as_mut_ptr()) };
    if name.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `name` points into `buffer`, which remains live for this conversion.
    Ok(unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned())
}

#[cfg(target_os = "linux")]
fn parse_ipv4_routes() -> io::Result<Vec<RouteInfo>> {
    let text = fs::read_to_string("/proc/net/route")?;
    Ok(text
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 8 {
                return None;
            }
            let destination = hex_ipv4(fields[1])?;
            let gateway = hex_ipv4(fields[2]);
            let prefix_len = hex_ipv4(fields[7]).map(|mask| u32::from(mask).count_ones() as u8)?;
            Some(RouteInfo {
                family: AddressFamily::Inet4,
                destination: destination.to_string(),
                prefix_len,
                gateway: gateway
                    .filter(|gateway| *gateway != Ipv4Addr::UNSPECIFIED)
                    .map(|value| value.to_string()),
                dev: fields[0].to_string(),
                source: None,
                metric: fields.get(6).and_then(|value| value.parse().ok()),
                table: Some(libc::RT_TABLE_MAIN as u32),
                protocol: Some(libc::RTPROT_UNSPEC),
            })
        })
        .collect())
}

#[cfg(target_os = "linux")]
fn parse_ipv6_routes() -> io::Result<Vec<RouteInfo>> {
    let text = match fs::read_to_string("/proc/net/ipv6_route") {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    Ok(text
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 10 {
                return None;
            }
            let destination = parse_ipv6_hex(fields[0])?;
            let prefix_len = u8::from_str_radix(fields[1], 16).ok()?;
            let gateway = parse_ipv6_hex(fields[4])?;
            Some(RouteInfo {
                family: AddressFamily::Inet6,
                destination: destination.to_string(),
                prefix_len,
                gateway: (gateway != Ipv6Addr::UNSPECIFIED).then(|| gateway.to_string()),
                dev: fields[9].to_string(),
                source: None,
                metric: fields
                    .get(5)
                    .and_then(|value| u32::from_str_radix(value, 16).ok()),
                table: Some(libc::RT_TABLE_MAIN as u32),
                protocol: Some(libc::RTPROT_UNSPEC),
            })
        })
        .collect())
}

#[cfg(target_os = "linux")]
fn live_list_neighbors() -> io::Result<Vec<NeighborInfo>> {
    let mut neighbors = dump_neighbors(libc::AF_INET as u8)?;
    neighbors.extend(dump_neighbors(libc::AF_INET6 as u8)?);
    neighbors.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.dev.cmp(&right.dev))
            .then(left.address.cmp(&right.address))
    });
    Ok(neighbors)
}

#[cfg(target_os = "linux")]
fn dump_neighbors(family: u8) -> io::Result<Vec<NeighborInfo>> {
    let fd = rtnetlink_socket_fd()?;
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &NdMsg {
            ndm_family: family,
            ndm_pad1: 0,
            ndm_pad2: 0,
            ndm_ifindex: 0,
            ndm_state: 0,
            ndm_flags: 0,
            ndm_type: 0,
        },
    );

    let mut message = Vec::with_capacity(mem::size_of::<libc::nlmsghdr>() + payload.len());
    append_bytes(
        &mut message,
        &libc::nlmsghdr {
            nlmsg_len: (mem::size_of::<libc::nlmsghdr>() + payload.len()) as u32,
            nlmsg_type: libc::RTM_GETNEIGH,
            nlmsg_flags: (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16,
            nlmsg_seq: 1,
            nlmsg_pid: 0,
        },
    );
    message.extend_from_slice(&payload);

    let mut kernel = unsafe { mem::zeroed::<libc::sockaddr_nl>() };
    kernel.nl_family = libc::AF_NETLINK as libc::sa_family_t;

    // SAFETY: buffer and sockaddr point to initialized memory for sendto.
    let rc = unsafe {
        libc::sendto(
            fd,
            message.as_ptr() as *const libc::c_void,
            message.len(),
            0,
            &kernel as *const libc::sockaddr_nl as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        let err = io::Error::last_os_error();
        close_fd(fd);
        return Err(err);
    }

    let mut neighbors = Vec::new();
    let mut buffer = vec![0_u8; 8192];
    loop {
        // SAFETY: `buffer` is valid writable memory for recv.
        let len = unsafe {
            libc::recv(
                fd,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
                0,
            )
        };
        if len < 0 {
            let err = io::Error::last_os_error();
            close_fd(fd);
            return Err(err);
        }
        if len == 0 {
            close_fd(fd);
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short netlink reply",
            ));
        }
        if parse_neighbor_dump_chunk(&buffer[..len as usize], &mut neighbors)? {
            break;
        }
    }
    close_fd(fd);
    Ok(neighbors)
}

#[cfg(target_os = "linux")]
fn parse_neighbor_dump_chunk(buffer: &[u8], neighbors: &mut Vec<NeighborInfo>) -> io::Result<bool> {
    let header_len = mem::size_of::<libc::nlmsghdr>();
    let error_len = mem::size_of::<libc::nlmsgerr>();
    let mut offset = 0;
    while offset + header_len <= buffer.len() {
        // SAFETY: bounds checked above and aligned access is fine for nlmsghdr.
        let header = unsafe { &*(buffer[offset..].as_ptr() as *const libc::nlmsghdr) };
        if header.nlmsg_len < header_len as u32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed netlink reply",
            ));
        }
        let message_end = offset + header.nlmsg_len as usize;
        if message_end > buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated netlink reply",
            ));
        }
        match header.nlmsg_type {
            value if value == libc::NLMSG_DONE as u16 => return Ok(true),
            value if value == libc::NLMSG_ERROR as u16 => {
                if header.nlmsg_len < (header_len + error_len) as u32 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "truncated netlink error",
                    ));
                }
                // SAFETY: bounds checked above for nlmsgerr payload.
                let error =
                    unsafe { &*(buffer[offset + header_len..].as_ptr() as *const libc::nlmsgerr) };
                if error.error != 0 {
                    return Err(io::Error::from_raw_os_error(-error.error));
                }
            }
            value if value == libc::RTM_NEWNEIGH => {
                if let Some(neighbor) =
                    parse_neighbor_message(&buffer[offset + header_len..message_end])?
                {
                    neighbors.push(neighbor);
                }
            }
            _ => {}
        }
        offset += nlmsg_align(header.nlmsg_len as usize);
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
fn parse_neighbor_message(payload: &[u8]) -> io::Result<Option<NeighborInfo>> {
    let msg_len = mem::size_of::<NdMsg>();
    if payload.len() < msg_len {
        return Ok(None);
    }
    // SAFETY: bounds checked above and NdMsg is POD.
    let message = unsafe { &*(payload.as_ptr() as *const NdMsg) };
    let family = match message.ndm_family as i32 {
        libc::AF_INET => AddressFamily::Inet4,
        libc::AF_INET6 => AddressFamily::Inet6,
        _ => return Ok(None),
    };
    let mut address = None;
    let mut lladdr = None;
    let mut offset = msg_len;
    let attr_len = mem::size_of::<RtAttr>();
    while offset + attr_len <= payload.len() {
        // SAFETY: bounds checked above and RtAttr is POD.
        let attr = unsafe { &*(payload[offset..].as_ptr() as *const RtAttr) };
        if attr.rta_len < attr_len as u16 {
            break;
        }
        let end = offset + attr.rta_len as usize;
        if end > payload.len() {
            break;
        }
        let data = &payload[offset + attr_len..end];
        match attr.rta_type {
            value if value == NDA_DST => {
                address = Some(parse_route_address(family, data)?.to_string());
            }
            value if value == NDA_LLADDR => {
                lladdr = Some(format_mac(data));
            }
            _ => {}
        }
        offset += nlmsg_align(attr.rta_len as usize);
    }

    let Some(address) = address else {
        return Ok(None);
    };
    Ok(Some(NeighborInfo {
        family,
        address,
        dev: if_name(message.ndm_ifindex as u32)?,
        lladdr,
        state: message.ndm_state,
    }))
}

#[cfg(target_os = "linux")]
fn live_del_neighbor(neighbor: &NeighborInfo) -> io::Result<()> {
    let index = if_index(&neighbor.dev)?;
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &NdMsg {
            ndm_family: match neighbor.family {
                AddressFamily::Inet4 => libc::AF_INET as u8,
                AddressFamily::Inet6 => libc::AF_INET6 as u8,
            },
            ndm_pad1: 0,
            ndm_pad2: 0,
            ndm_ifindex: index as i32,
            ndm_state: neighbor.state,
            ndm_flags: 0,
            ndm_type: 0,
        },
    );
    append_route_addr(&mut payload, neighbor.family, NDA_DST, &neighbor.address)?;
    rtnetlink_request(
        libc::RTM_DELNEIGH,
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK) as u16,
        &payload,
    )
}

#[cfg(target_os = "linux")]
fn live_list_rules() -> io::Result<Vec<RuleInfo>> {
    let mut rules = dump_rules(libc::AF_INET as u8)?;
    rules.extend(dump_rules(libc::AF_INET6 as u8)?);
    rules.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then(left.family.cmp(&right.family))
            .then(left.table.cmp(&right.table))
    });
    Ok(rules)
}

#[cfg(target_os = "linux")]
fn dump_rules(family: u8) -> io::Result<Vec<RuleInfo>> {
    let fd = rtnetlink_socket_fd()?;
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &FibRuleHdr {
            family,
            dst_len: 0,
            src_len: 0,
            tos: 0,
            table: libc::RT_TABLE_UNSPEC,
            res1: 0,
            res2: 0,
            action: FR_ACT_TO_TBL,
            flags: 0,
        },
    );

    let mut message = Vec::with_capacity(mem::size_of::<libc::nlmsghdr>() + payload.len());
    append_bytes(
        &mut message,
        &libc::nlmsghdr {
            nlmsg_len: (mem::size_of::<libc::nlmsghdr>() + payload.len()) as u32,
            nlmsg_type: libc::RTM_GETRULE,
            nlmsg_flags: (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16,
            nlmsg_seq: 1,
            nlmsg_pid: 0,
        },
    );
    message.extend_from_slice(&payload);

    let mut kernel = unsafe { mem::zeroed::<libc::sockaddr_nl>() };
    kernel.nl_family = libc::AF_NETLINK as libc::sa_family_t;

    // SAFETY: buffer and sockaddr point to initialized memory for sendto.
    let rc = unsafe {
        libc::sendto(
            fd,
            message.as_ptr() as *const libc::c_void,
            message.len(),
            0,
            &kernel as *const libc::sockaddr_nl as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        let err = io::Error::last_os_error();
        close_fd(fd);
        return Err(err);
    }

    let mut rules = Vec::new();
    let mut buffer = vec![0_u8; 8192];
    loop {
        // SAFETY: `buffer` is valid writable memory for recv.
        let len = unsafe {
            libc::recv(
                fd,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
                0,
            )
        };
        if len < 0 {
            let err = io::Error::last_os_error();
            close_fd(fd);
            return Err(err);
        }
        if len == 0 {
            close_fd(fd);
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short netlink reply",
            ));
        }
        if parse_rule_dump_chunk(&buffer[..len as usize], &mut rules)? {
            break;
        }
    }
    close_fd(fd);
    Ok(rules)
}

#[cfg(target_os = "linux")]
fn parse_rule_dump_chunk(buffer: &[u8], rules: &mut Vec<RuleInfo>) -> io::Result<bool> {
    let header_len = mem::size_of::<libc::nlmsghdr>();
    let error_len = mem::size_of::<libc::nlmsgerr>();
    let mut offset = 0;
    while offset + header_len <= buffer.len() {
        // SAFETY: bounds checked above and aligned access is fine for nlmsghdr.
        let header = unsafe { &*(buffer[offset..].as_ptr() as *const libc::nlmsghdr) };
        if header.nlmsg_len < header_len as u32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed netlink reply",
            ));
        }
        let message_end = offset + header.nlmsg_len as usize;
        if message_end > buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated netlink reply",
            ));
        }
        match header.nlmsg_type {
            value if value == libc::NLMSG_DONE as u16 => return Ok(true),
            value if value == libc::NLMSG_ERROR as u16 => {
                if header.nlmsg_len < (header_len + error_len) as u32 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "truncated netlink error",
                    ));
                }
                // SAFETY: bounds checked above for nlmsgerr payload.
                let error =
                    unsafe { &*(buffer[offset + header_len..].as_ptr() as *const libc::nlmsgerr) };
                if error.error != 0 {
                    return Err(io::Error::from_raw_os_error(-error.error));
                }
            }
            value if value == libc::RTM_NEWRULE => {
                if let Some(rule) = parse_rule_message(&buffer[offset + header_len..message_end])? {
                    rules.push(rule);
                }
            }
            _ => {}
        }
        offset += nlmsg_align(header.nlmsg_len as usize);
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
fn parse_rule_message(payload: &[u8]) -> io::Result<Option<RuleInfo>> {
    let msg_len = mem::size_of::<FibRuleHdr>();
    if payload.len() < msg_len {
        return Ok(None);
    }
    // SAFETY: bounds checked above and FibRuleHdr is POD.
    let message = unsafe { &*(payload.as_ptr() as *const FibRuleHdr) };
    let family = match message.family as i32 {
        libc::AF_INET => AddressFamily::Inet4,
        libc::AF_INET6 => AddressFamily::Inet6,
        _ => return Ok(None),
    };
    if message.action != FR_ACT_TO_TBL {
        return Ok(None);
    }
    let mut from = None;
    let mut to = None;
    let mut iif = None;
    let mut oif = None;
    let mut fwmark = None;
    let mut priority = None;
    let mut table = message.table as u32;
    let mut offset = msg_len;
    let attr_len = mem::size_of::<RtAttr>();
    while offset + attr_len <= payload.len() {
        // SAFETY: bounds checked above and RtAttr is POD.
        let attr = unsafe { &*(payload[offset..].as_ptr() as *const RtAttr) };
        if attr.rta_len < attr_len as u16 {
            break;
        }
        let end = offset + attr.rta_len as usize;
        if end > payload.len() {
            break;
        }
        let data = &payload[offset + attr_len..end];
        match attr.rta_type {
            value if value == FRA_SRC => {
                from = Some(parse_route_address(family, data)?.to_string());
            }
            value if value == FRA_DST => {
                to = Some(parse_route_address(family, data)?.to_string());
            }
            value if value == FRA_IIFNAME => {
                iif = parse_attr_c_string(data);
            }
            value if value == FRA_OIFNAME => {
                oif = parse_attr_c_string(data);
            }
            value if value == FRA_FWMARK && data.len() >= 4 => {
                fwmark = Some(u32::from_ne_bytes(data[..4].try_into().unwrap_or([0; 4])));
            }
            value if value == FRA_PRIORITY && data.len() >= 4 => {
                priority = Some(u32::from_ne_bytes(data[..4].try_into().unwrap_or([0; 4])));
            }
            value if value == FRA_TABLE && data.len() >= 4 => {
                table = u32::from_ne_bytes(data[..4].try_into().unwrap_or([0; 4]));
            }
            _ => {}
        }
        offset += nlmsg_align(attr.rta_len as usize);
    }

    Ok(Some(RuleInfo {
        family,
        from,
        from_prefix_len: message.src_len,
        to,
        to_prefix_len: message.dst_len,
        iif,
        oif,
        fwmark,
        priority,
        table,
    }))
}

#[cfg(target_os = "linux")]
fn live_rule_change(rule: &RuleInfo, request_type: u16) -> io::Result<()> {
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &FibRuleHdr {
            family: match rule.family {
                AddressFamily::Inet4 => libc::AF_INET as u8,
                AddressFamily::Inet6 => libc::AF_INET6 as u8,
            },
            dst_len: rule.to_prefix_len,
            src_len: rule.from_prefix_len,
            tos: 0,
            table: rule.table.min(u8::MAX as u32) as u8,
            res1: 0,
            res2: 0,
            action: FR_ACT_TO_TBL,
            flags: 0,
        },
    );
    if let Some(from) = &rule.from {
        append_rule_addr(&mut payload, rule.family, FRA_SRC, from)?;
    }
    if let Some(to) = &rule.to {
        append_rule_addr(&mut payload, rule.family, FRA_DST, to)?;
    }
    if let Some(iif) = &rule.iif {
        append_attr_c_string(&mut payload, FRA_IIFNAME, iif);
    }
    if let Some(oif) = &rule.oif {
        append_attr_c_string(&mut payload, FRA_OIFNAME, oif);
    }
    if let Some(fwmark) = rule.fwmark {
        append_rtattr(&mut payload, FRA_FWMARK, &fwmark.to_ne_bytes());
    }
    if let Some(priority) = rule.priority {
        append_rtattr(&mut payload, FRA_PRIORITY, &priority.to_ne_bytes());
    }
    if rule.table > u8::MAX as u32 {
        append_rtattr(&mut payload, FRA_TABLE, &rule.table.to_ne_bytes());
    }
    let flags = if request_type == libc::RTM_NEWRULE {
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK | libc::NLM_F_CREATE | libc::NLM_F_EXCL) as u16
    } else {
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK) as u16
    };
    rtnetlink_request(request_type, flags, &payload)
}

#[cfg(target_os = "linux")]
fn append_rule_addr(
    payload: &mut Vec<u8>,
    family: AddressFamily,
    attr: u16,
    value: &str,
) -> io::Result<()> {
    append_route_addr(payload, family, attr, value)
}

#[cfg(target_os = "linux")]
fn append_attr_c_string(payload: &mut Vec<u8>, attr: u16, value: &str) {
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    append_rtattr(payload, attr, &bytes);
}

#[cfg(target_os = "linux")]
fn parse_attr_c_string(data: &[u8]) -> Option<String> {
    let trimmed = data.split(|byte| *byte == 0).next().unwrap_or(data);
    (!trimmed.is_empty()).then(|| String::from_utf8_lossy(trimmed).into_owned())
}

#[cfg(target_os = "linux")]
fn hex_ipv4(value: &str) -> Option<Ipv4Addr> {
    let value = u32::from_str_radix(value, 16).ok()?;
    Some(Ipv4Addr::from(value.swap_bytes()))
}

#[cfg(target_os = "linux")]
fn parse_ipv6_hex(value: &str) -> Option<Ipv6Addr> {
    if value.len() != 32 {
        return None;
    }
    let mut bytes = [0_u8; 16];
    for index in 0..16 {
        bytes[index] = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(Ipv6Addr::from(bytes))
}

#[cfg(target_os = "linux")]
fn live_apply_link_change(name: &str, change: &LinkChange) -> io::Result<()> {
    if change.up.is_some()
        || change.arp.is_some()
        || change.multicast.is_some()
        || change.allmulti.is_some()
        || change.promisc.is_some()
        || change.dynamic.is_some()
        || change.trailers.is_some()
    {
        let mut flags = get_flags(name)?;
        if let Some(up) = change.up {
            if up {
                flags |= libc::IFF_UP as u32;
            } else {
                flags &= !(libc::IFF_UP as u32);
            }
        }
        apply_flag_change(
            &mut flags,
            libc::IFF_NOARP as u32,
            change.arp.map(|value| !value),
        );
        apply_flag_change(&mut flags, libc::IFF_MULTICAST as u32, change.multicast);
        apply_flag_change(&mut flags, libc::IFF_ALLMULTI as u32, change.allmulti);
        apply_flag_change(&mut flags, libc::IFF_PROMISC as u32, change.promisc);
        apply_flag_change(&mut flags, libc::IFF_DYNAMIC as u32, change.dynamic);
        apply_flag_change(
            &mut flags,
            libc::IFF_NOTRAILERS as u32,
            change.trailers.map(|value| !value),
        );
        set_flags(name, flags)?;
    }
    if let Some(mtu) = change.mtu {
        set_mtu(name, mtu)?;
    }
    if let Some(metric) = change.metric {
        set_metric(name, metric)?;
    }
    if let Some(tx_queue_len) = change.tx_queue_len {
        set_tx_queue_len(name, tx_queue_len)?;
    }
    if change.mem_start.is_some() || change.io_addr.is_some() || change.irq.is_some() {
        set_ifmap(name, change.mem_start, change.io_addr, change.irq)?;
    }
    if let Some(address) = &change.address {
        set_hwaddr(name, &parse_mac(address)?)?;
    }
    if let Some(new_name) = &change.new_name {
        rename_interface(name, new_name)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn live_set_ipv4(name: &str, change: &Ipv4Change) -> io::Result<()> {
    if let Some(address) = change.address {
        set_sockaddr(name, libc::SIOCSIFADDR, address)?;
    } else {
        set_sockaddr(name, libc::SIOCSIFADDR, Ipv4Addr::UNSPECIFIED)?;
    }
    if let Some(prefix_len) = change.prefix_len {
        set_sockaddr(name, libc::SIOCSIFNETMASK, prefix_to_netmask(prefix_len))?;
    }
    if let Some(peer) = change.peer {
        set_sockaddr(name, libc::SIOCSIFDSTADDR, peer)?;
    }
    if let Some(broadcast) = &change.broadcast {
        set_sockaddr(
            name,
            libc::SIOCSIFBRDADDR,
            broadcast.unwrap_or(Ipv4Addr::UNSPECIFIED),
        )?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn live_add_ipv4(name: &str, address: &InterfaceAddress) -> io::Result<()> {
    let ipv4 = address
        .address
        .parse::<Ipv4Addr>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid IPv4 address"))?;
    let peer = address
        .peer
        .as_deref()
        .map(|value| {
            value
                .split_once('/')
                .map_or(value, |(address, _)| address)
                .parse::<Ipv4Addr>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid peer address"))
        })
        .transpose()?;
    let broadcast = address
        .broadcast
        .as_deref()
        .map(|value| {
            value.parse::<Ipv4Addr>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid broadcast address")
            })
        })
        .transpose()?;
    let scope = parse_scope_value(address.scope.as_deref())?;
    let index = if_index(name)?;
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &IfAddrMsg {
            ifa_family: libc::AF_INET as u8,
            ifa_prefixlen: address.prefix_len,
            ifa_flags: 0,
            ifa_scope: scope,
            ifa_index: index,
        },
    );
    let local = peer.unwrap_or(ipv4);
    append_rtattr(&mut payload, libc::IFA_LOCAL, &local.octets());
    append_rtattr(&mut payload, libc::IFA_ADDRESS, &ipv4.octets());
    if let Some(broadcast) = broadcast {
        append_rtattr(&mut payload, libc::IFA_BROADCAST, &broadcast.octets());
    }
    if let Some(label) = &address.label {
        let mut bytes = label.as_bytes().to_vec();
        bytes.push(0);
        append_rtattr(&mut payload, libc::IFA_LABEL, &bytes);
    }
    rtnetlink_request(
        libc::RTM_NEWADDR,
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK | libc::NLM_F_CREATE | libc::NLM_F_EXCL) as u16,
        &payload,
    )
}

#[cfg(target_os = "linux")]
fn live_remove_ipv4(name: &str, address: &str) -> io::Result<()> {
    let ipv4 = address
        .parse::<Ipv4Addr>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid IPv4 address"))?;
    let index = if_index(name)?;
    let prefix_len = list_interfaces()?
        .into_iter()
        .find(|interface| interface.name == name)
        .and_then(|interface| {
            interface.addresses.into_iter().find_map(|existing| {
                (existing.family == AddressFamily::Inet4 && existing.address == address)
                    .then_some(existing.prefix_len)
            })
        })
        .unwrap_or(32);
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &IfAddrMsg {
            ifa_family: libc::AF_INET as u8,
            ifa_prefixlen: prefix_len,
            ifa_flags: 0,
            ifa_scope: 0,
            ifa_index: index,
        },
    );
    let octets = ipv4.octets();
    append_rtattr(&mut payload, libc::IFA_LOCAL, &octets);
    append_rtattr(&mut payload, libc::IFA_ADDRESS, &octets);
    rtnetlink_request(
        libc::RTM_DELADDR,
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK) as u16,
        &payload,
    )
}

#[cfg(target_os = "linux")]
fn live_route_change(route: &RouteInfo, request: libc::c_ulong) -> io::Result<()> {
    let request_type = match request {
        libc::SIOCADDRT => libc::RTM_NEWROUTE,
        libc::SIOCDELRT => libc::RTM_DELROUTE,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported route request",
            ));
        }
    };
    live_netlink_route_change(route, request_type)
}

#[cfg(target_os = "linux")]
fn live_add_ipv6(name: &str, address: &InterfaceAddress) -> io::Result<()> {
    let ipv6 = address
        .address
        .parse::<Ipv6Addr>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid IPv6 address"))?;
    let peer = address
        .peer
        .as_deref()
        .map(|value| {
            value
                .split_once('/')
                .map_or(value, |(address, _)| address)
                .parse::<Ipv6Addr>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid peer address"))
        })
        .transpose()?;
    let scope = parse_scope_value(address.scope.as_deref())?;
    let index = if_index(name)?;
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &IfAddrMsg {
            ifa_family: libc::AF_INET6 as u8,
            ifa_prefixlen: address.prefix_len,
            ifa_flags: 0,
            ifa_scope: scope,
            ifa_index: index,
        },
    );
    append_rtattr(&mut payload, libc::IFA_ADDRESS, &ipv6.octets());
    append_rtattr(
        &mut payload,
        libc::IFA_LOCAL,
        &peer.unwrap_or(ipv6).octets(),
    );
    if let Some(label) = &address.label {
        let mut bytes = label.as_bytes().to_vec();
        bytes.push(0);
        append_rtattr(&mut payload, libc::IFA_LABEL, &bytes);
    }
    rtnetlink_request(
        libc::RTM_NEWADDR,
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK | libc::NLM_F_CREATE | libc::NLM_F_EXCL) as u16,
        &payload,
    )
}

#[cfg(target_os = "linux")]
fn live_remove_ipv6(name: &str, address: &str) -> io::Result<()> {
    let ipv6 = address
        .parse::<Ipv6Addr>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid IPv6 address"))?;
    let index = if_index(name)?;
    let prefix_len = list_interfaces()?
        .into_iter()
        .find(|interface| interface.name == name)
        .and_then(|interface| {
            interface.addresses.into_iter().find_map(|existing| {
                (existing.family == AddressFamily::Inet6 && existing.address == address)
                    .then_some(existing.prefix_len)
            })
        })
        .unwrap_or(128);
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &IfAddrMsg {
            ifa_family: libc::AF_INET6 as u8,
            ifa_prefixlen: prefix_len,
            ifa_flags: 0,
            ifa_scope: 0,
            ifa_index: index,
        },
    );
    let octets = ipv6.octets();
    append_rtattr(&mut payload, libc::IFA_ADDRESS, &octets);
    append_rtattr(&mut payload, libc::IFA_LOCAL, &octets);
    rtnetlink_request(
        libc::RTM_DELADDR,
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK) as u16,
        &payload,
    )
}

#[cfg(target_os = "linux")]
fn live_netlink_route_change(route: &RouteInfo, request_type: u16) -> io::Result<()> {
    let index = if_index(&route.dev)?;
    let table = route.table.unwrap_or(libc::RT_TABLE_MAIN as u32);
    let mut payload = Vec::new();
    append_bytes(
        &mut payload,
        &RtMsg {
            rtm_family: match route.family {
                AddressFamily::Inet4 => libc::AF_INET as u8,
                AddressFamily::Inet6 => libc::AF_INET6 as u8,
            },
            rtm_dst_len: route.prefix_len,
            rtm_src_len: 0,
            rtm_tos: 0,
            rtm_table: table.min(u8::MAX as u32) as u8,
            rtm_protocol: route.protocol.unwrap_or(libc::RTPROT_BOOT),
            rtm_scope: if route.gateway.is_some() {
                libc::RT_SCOPE_UNIVERSE
            } else {
                libc::RT_SCOPE_LINK
            },
            rtm_type: libc::RTN_UNICAST,
            rtm_flags: 0,
        },
    );
    append_route_addr(
        &mut payload,
        route.family,
        libc::RTA_DST,
        &route.destination,
    )?;
    if let Some(gateway) = &route.gateway {
        append_route_addr(&mut payload, route.family, libc::RTA_GATEWAY, gateway)?;
    }
    if let Some(source) = &route.source {
        append_route_addr(&mut payload, route.family, libc::RTA_PREFSRC, source)?;
    }
    append_rtattr(&mut payload, libc::RTA_OIF, &index.to_ne_bytes());
    if let Some(metric) = route.metric {
        append_rtattr(&mut payload, libc::RTA_PRIORITY, &metric.to_ne_bytes());
    }
    if table > u8::MAX as u32 {
        append_rtattr(&mut payload, libc::RTA_TABLE, &table.to_ne_bytes());
    }

    let flags = if request_type == libc::RTM_NEWROUTE {
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK | libc::NLM_F_CREATE | libc::NLM_F_EXCL) as u16
    } else {
        (libc::NLM_F_REQUEST | libc::NLM_F_ACK) as u16
    };
    rtnetlink_request(request_type, flags, &payload)
}

#[cfg(target_os = "linux")]
fn append_route_addr(
    payload: &mut Vec<u8>,
    family: AddressFamily,
    attr: u16,
    value: &str,
) -> io::Result<()> {
    match family {
        AddressFamily::Inet4 => {
            let address = value.parse::<Ipv4Addr>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid IPv4 route address")
            })?;
            if address != Ipv4Addr::UNSPECIFIED {
                append_rtattr(payload, attr, &address.octets());
            }
        }
        AddressFamily::Inet6 => {
            let address = value.parse::<Ipv6Addr>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid IPv6 route address")
            })?;
            if address != Ipv6Addr::UNSPECIFIED {
                append_rtattr(payload, attr, &address.octets());
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn if_index(name: &str) -> io::Result<u32> {
    let name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "device contains NUL byte"))?;
    // SAFETY: `name` is a valid C string for if_nametoindex.
    let index = unsafe { libc::if_nametoindex(name.as_ptr()) };
    if index == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(index)
    }
}

#[cfg(target_os = "linux")]
fn rtnetlink_request(request_type: u16, flags: u16, payload: &[u8]) -> io::Result<()> {
    let fd = rtnetlink_socket_fd()?;
    let mut message = Vec::with_capacity(mem::size_of::<libc::nlmsghdr>() + payload.len());
    append_bytes(
        &mut message,
        &libc::nlmsghdr {
            nlmsg_len: (mem::size_of::<libc::nlmsghdr>() + payload.len()) as u32,
            nlmsg_type: request_type,
            nlmsg_flags: flags,
            nlmsg_seq: 1,
            nlmsg_pid: 0,
        },
    );
    message.extend_from_slice(payload);

    let mut kernel = unsafe { mem::zeroed::<libc::sockaddr_nl>() };
    kernel.nl_family = libc::AF_NETLINK as libc::sa_family_t;

    // SAFETY: buffer and sockaddr point to initialized memory for sendto.
    let rc = unsafe {
        libc::sendto(
            fd,
            message.as_ptr() as *const libc::c_void,
            message.len(),
            0,
            &kernel as *const libc::sockaddr_nl as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        let err = io::Error::last_os_error();
        close_fd(fd);
        return Err(err);
    }

    let mut buffer = vec![0_u8; 4096];
    // SAFETY: `buffer` is valid writable memory for recv.
    let len = unsafe {
        libc::recv(
            fd,
            buffer.as_mut_ptr() as *mut libc::c_void,
            buffer.len(),
            0,
        )
    };
    if len < 0 {
        let err = io::Error::last_os_error();
        close_fd(fd);
        return Err(err);
    }
    if len == 0 {
        close_fd(fd);
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "short netlink reply",
        ));
    }

    let result = parse_rtnetlink_reply(&buffer[..len as usize]);
    close_fd(fd);
    result
}

#[cfg(target_os = "linux")]
fn parse_rtnetlink_reply(buffer: &[u8]) -> io::Result<()> {
    let header_len = mem::size_of::<libc::nlmsghdr>();
    let error_len = mem::size_of::<libc::nlmsgerr>();
    let mut offset = 0;
    while offset + header_len <= buffer.len() {
        // SAFETY: bounds checked above and aligned access is fine for nlmsghdr.
        let header = unsafe { &*(buffer[offset..].as_ptr() as *const libc::nlmsghdr) };
        if header.nlmsg_len < header_len as u32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "malformed netlink reply",
            ));
        }
        let message_end = offset + header.nlmsg_len as usize;
        if message_end > buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "truncated netlink reply",
            ));
        }
        match header.nlmsg_type {
            value if value == libc::NLMSG_DONE as u16 => return Ok(()),
            value if value == libc::NLMSG_ERROR as u16 => {
                if header.nlmsg_len < (header_len + error_len) as u32 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "truncated netlink error",
                    ));
                }
                // SAFETY: bounds checked above for nlmsgerr payload.
                let error =
                    unsafe { &*(buffer[offset + header_len..].as_ptr() as *const libc::nlmsgerr) };
                if error.error == 0 {
                    return Ok(());
                }
                return Err(io::Error::from_raw_os_error(-error.error));
            }
            _ => {}
        }
        offset += nlmsg_align(header.nlmsg_len as usize);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "missing netlink ack",
    ))
}

#[cfg(target_os = "linux")]
fn rtnetlink_socket_fd() -> io::Result<libc::c_int> {
    // SAFETY: socket arguments are valid constants.
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::NETLINK_ROUTE,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    let mut local = unsafe { mem::zeroed::<libc::sockaddr_nl>() };
    local.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    // SAFETY: sockaddr points to initialized local address for bind.
    let rc = unsafe {
        libc::bind(
            fd,
            &local as *const libc::sockaddr_nl as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if rc == 0 {
        Ok(fd)
    } else {
        let err = io::Error::last_os_error();
        close_fd(fd);
        Err(err)
    }
}

#[cfg(target_os = "linux")]
fn append_rtattr(buffer: &mut Vec<u8>, rta_type: u16, data: &[u8]) {
    let attr_len = mem::size_of::<RtAttr>() + data.len();
    append_bytes(
        buffer,
        &RtAttr {
            rta_len: attr_len as u16,
            rta_type,
        },
    );
    buffer.extend_from_slice(data);
    let aligned = nlmsg_align(attr_len);
    if aligned > attr_len {
        buffer.resize(buffer.len() + (aligned - attr_len), 0);
    }
}

#[cfg(target_os = "linux")]
fn nlmsg_align(length: usize) -> usize {
    (length + 3) & !3
}

#[cfg(target_os = "linux")]
fn append_bytes<T>(buffer: &mut Vec<u8>, value: &T) {
    let len = mem::size_of::<T>();
    // SAFETY: `value` is a valid plain-old-data struct copied byte-for-byte into the message.
    let bytes = unsafe { std::slice::from_raw_parts(value as *const T as *const u8, len) };
    buffer.extend_from_slice(bytes);
}

#[cfg(target_os = "linux")]
fn get_flags(name: &str) -> io::Result<u32> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    // SAFETY: `ifreq` points to writable memory for the ioctl result.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCGIFFLAGS as _, &mut ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 {
        // SAFETY: `ifru_flags` is initialized by SIOCGIFFLAGS on success.
        Ok(unsafe { ifreq.ifru.flags } as u32)
    } else {
        Err(err)
    }
}

#[cfg(target_os = "linux")]
fn set_flags(name: &str, flags: u32) -> io::Result<()> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    ifreq.ifru.flags = flags as libc::c_short;
    // SAFETY: `ifreq` points to a valid ifreq with flags populated.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCSIFFLAGS as _, &ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn set_mtu(name: &str, mtu: u32) -> io::Result<()> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    ifreq.ifru.mtu = mtu as libc::c_int;
    // SAFETY: `ifreq` points to a valid ifreq with mtu populated.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCSIFMTU as _, &ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn get_metric(name: &str) -> io::Result<u32> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    // SAFETY: `ifreq` points to writable memory for the ioctl result.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCGIFMETRIC as _, &mut ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 {
        Ok(unsafe { ifreq.ifru.ivalue } as u32)
    } else {
        Err(err)
    }
}

#[cfg(target_os = "linux")]
fn set_metric(name: &str, metric: u32) -> io::Result<()> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    ifreq.ifru.ivalue = metric as libc::c_int;
    // SAFETY: `ifreq` points to a valid ifreq with metric populated.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCSIFMETRIC as _, &ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn set_tx_queue_len(name: &str, tx_queue_len: u32) -> io::Result<()> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    ifreq.ifru.ivalue = tx_queue_len as libc::c_int;
    // SAFETY: `ifreq` points to a valid ifreq with queue length populated.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCSIFTXQLEN as _, &ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn get_ifmap(name: &str) -> io::Result<(u64, u16, u8)> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    // SAFETY: `ifreq` points to writable memory for the ioctl result.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCGIFMAP as _, &mut ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 {
        let map = unsafe { ifreq.ifru.map };
        Ok((map.mem_start, map.base_addr, map.irq))
    } else {
        Err(err)
    }
}

#[cfg(target_os = "linux")]
fn set_ifmap(
    name: &str,
    mem_start: Option<u64>,
    io_addr: Option<u16>,
    irq: Option<u8>,
) -> io::Result<()> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    // SAFETY: `ifreq` points to writable memory for the ioctl result.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCGIFMAP as _, &mut ifreq) };
    let err = io::Error::last_os_error();
    if rc != 0 {
        close_fd(fd);
        return Err(err);
    }
    let mut map = unsafe { ifreq.ifru.map };
    if let Some(mem_start) = mem_start {
        map.mem_start = mem_start as libc::c_ulong;
    }
    if let Some(io_addr) = io_addr {
        map.base_addr = io_addr;
    }
    if let Some(irq) = irq {
        map.irq = irq;
    }
    ifreq.ifru.map = map;
    // SAFETY: `ifreq` contains a valid ifmap payload.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCSIFMAP as _, &ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn set_sockaddr(name: &str, request: libc::c_ulong, address: Ipv4Addr) -> io::Result<()> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    ifreq.ifru.addr = sockaddr_from_ipv4(address);
    // SAFETY: `ifreq` contains a valid AF_INET sockaddr for the request.
    let rc = unsafe { libc::ioctl(fd, request as _, &ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn set_hwaddr(name: &str, mac: &[u8; 6]) -> io::Result<()> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    ifreq.ifru.addr = sockaddr_from_hwaddr(mac);
    // SAFETY: `ifreq` contains a valid AF_PACKET hardware address.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCSIFHWADDR as _, &ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn rename_interface(name: &str, new_name: &str) -> io::Result<()> {
    let fd = socket_fd()?;
    let mut ifreq = Ifreq::new(name)?;
    let mut new_name_storage = [0; libc::IFNAMSIZ];
    copy_name_bytes(&mut new_name_storage, new_name)?;
    ifreq.ifru.name = new_name_storage;
    // SAFETY: `ifreq` contains valid old/new interface names.
    let rc = unsafe { libc::ioctl(fd, libc::SIOCSIFNAME as _, &ifreq) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
}

#[cfg(target_os = "linux")]
fn socket_fd() -> io::Result<libc::c_int> {
    // SAFETY: socket arguments are valid constants.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if fd >= 0 {
        Ok(fd)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn close_fd(fd: libc::c_int) {
    // SAFETY: `fd` is a socket descriptor from socket_fd.
    unsafe { libc::close(fd) };
}

#[cfg(target_os = "linux")]
fn sockaddr_from_ipv4(address: Ipv4Addr) -> libc::sockaddr {
    let mut storage = unsafe { mem::zeroed::<libc::sockaddr_in>() };
    storage.sin_family = libc::AF_INET as libc::sa_family_t;
    storage.sin_addr = libc::in_addr {
        s_addr: u32::from(address).to_be(),
    };
    // SAFETY: `sockaddr_in` and `sockaddr` are layout-compatible for the leading bytes used by ioctl.
    unsafe { mem::transmute(storage) }
}

#[cfg(target_os = "linux")]
fn sockaddr_from_hwaddr(mac: &[u8; 6]) -> libc::sockaddr {
    let mut storage = unsafe { mem::zeroed::<libc::sockaddr>() };
    storage.sa_family = libc::ARPHRD_ETHER as libc::sa_family_t;
    for (index, byte) in mac.iter().enumerate() {
        storage.sa_data[index] = *byte as libc::c_char;
    }
    storage
}

#[cfg(target_os = "linux")]
fn copy_name_bytes(target: &mut [libc::c_char; libc::IFNAMSIZ], name: &str) -> io::Result<()> {
    let bytes = name.as_bytes();
    if bytes.len() >= libc::IFNAMSIZ {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "interface name too long",
        ));
    }
    for byte in target.iter_mut() {
        *byte = 0;
    }
    for (index, byte) in bytes.iter().enumerate() {
        target[index] = *byte as libc::c_char;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn apply_flag_change(flags: &mut u32, mask: u32, enabled: Option<bool>) {
    match enabled {
        Some(true) => *flags |= mask,
        Some(false) => *flags &= !mask,
        None => {}
    }
}

#[cfg(target_os = "linux")]
#[repr(C)]
union IfreqUnion {
    addr: libc::sockaddr,
    flags: libc::c_short,
    mtu: libc::c_int,
    ivalue: libc::c_int,
    map: IfMap,
    name: [libc::c_char; libc::IFNAMSIZ],
}

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
struct IfMap {
    mem_start: libc::c_ulong,
    mem_end: libc::c_ulong,
    base_addr: libc::c_ushort,
    irq: u8,
    dma: u8,
    port: u8,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct IfAddrMsg {
    ifa_family: u8,
    ifa_prefixlen: u8,
    ifa_flags: u8,
    ifa_scope: u8,
    ifa_index: u32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct RtMsg {
    rtm_family: u8,
    rtm_dst_len: u8,
    rtm_src_len: u8,
    rtm_tos: u8,
    rtm_table: u8,
    rtm_protocol: u8,
    rtm_scope: u8,
    rtm_type: u8,
    rtm_flags: u32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct NdMsg {
    ndm_family: u8,
    ndm_pad1: u8,
    ndm_pad2: u16,
    ndm_ifindex: i32,
    ndm_state: u16,
    ndm_flags: u8,
    ndm_type: u8,
}

#[cfg(target_os = "linux")]
const NDA_DST: u16 = 1;
#[cfg(target_os = "linux")]
const NDA_LLADDR: u16 = 2;
#[cfg(target_os = "linux")]
const FRA_DST: u16 = 1;
#[cfg(target_os = "linux")]
const FRA_SRC: u16 = 2;
#[cfg(target_os = "linux")]
const FRA_IIFNAME: u16 = 3;
#[cfg(target_os = "linux")]
const FRA_PRIORITY: u16 = 6;
#[cfg(target_os = "linux")]
const FRA_FWMARK: u16 = 10;
#[cfg(target_os = "linux")]
const FRA_TABLE: u16 = 15;
#[cfg(target_os = "linux")]
const FRA_OIFNAME: u16 = 17;
#[cfg(target_os = "linux")]
const FR_ACT_TO_TBL: u8 = 1;

#[cfg(target_os = "linux")]
#[repr(C)]
struct FibRuleHdr {
    family: u8,
    dst_len: u8,
    src_len: u8,
    tos: u8,
    table: u8,
    res1: u8,
    res2: u8,
    action: u8,
    flags: u32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct RtAttr {
    rta_len: u16,
    rta_type: u16,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct Ifreq {
    ifrn_name: [libc::c_char; libc::IFNAMSIZ],
    ifru: IfreqUnion,
}

#[cfg(target_os = "linux")]
impl Ifreq {
    fn new(name: &str) -> io::Result<Self> {
        let mut ifreq = Self {
            ifrn_name: [0; libc::IFNAMSIZ],
            ifru: IfreqUnion { mtu: 0 },
        };
        copy_name_bytes(&mut ifreq.ifrn_name, name)?;
        Ok(ifreq)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use super::{
        AddressFamily, InterfaceAddress, InterfaceInfo, LinkChange, RouteInfo, add_address,
        add_route, broadcast_for, del_route, format_mac, interface_flag_names, parse_mac,
        parse_prefix, prefix_to_netmask, remove_address, set_ipv4, state,
    };
    #[cfg(target_os = "linux")]
    use crate::common::test_env;
    #[cfg(target_os = "linux")]
    use std::{fs, io};

    #[test]
    #[cfg(target_os = "linux")]
    fn parses_ipv4_prefix() {
        let (address, prefix) = parse_prefix("192.168.1.2/24").unwrap();
        assert_eq!(address.to_string(), "192.168.1.2");
        assert_eq!(prefix, 24);
        assert_eq!(prefix_to_netmask(24).to_string(), "255.255.255.0");
        assert_eq!(broadcast_for(address, prefix).to_string(), "192.168.1.255");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn parses_mac_addresses() {
        assert_eq!(
            format_mac(&parse_mac("02:00:00:00:00:01").unwrap()),
            "02:00:00:00:00:01"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn names_common_flags() {
        let names =
            interface_flag_names((libc::IFF_UP | libc::IFF_BROADCAST | libc::IFF_RUNNING) as u32);
        assert!(names.contains(&"UP"));
        assert!(names.contains(&"BROADCAST"));
        assert!(names.contains(&"RUNNING"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn mutates_state_backend() {
        let _guard = test_env::lock();
        let path = std::env::temp_dir().join(format!("seed-net-{}", std::process::id()));
        let mut state = state::State::default();
        state.interfaces.push(InterfaceInfo {
            index: 1,
            name: String::from("eth0"),
            flags: libc::IFF_UP as u32,
            mtu: 1500,
            metric: 0,
            tx_queue_len: 1000,
            mem_start: 0,
            io_addr: 0,
            irq: 0,
            mac: Some(String::from("02:00:00:00:00:01")),
            addresses: Vec::new(),
            stats: super::InterfaceStats::default(),
        });
        state::write_state(&path, &state).unwrap();
        // SAFETY: the test holds the global env lock for the full mutation window.
        unsafe { std::env::set_var("SEED_NET_STATE", &path) };
        set_ipv4(
            "eth0",
            &super::Ipv4Change {
                address: Some("10.0.0.2".parse().unwrap()),
                prefix_len: Some(24),
                broadcast: Some(Some("10.0.0.255".parse().unwrap())),
                peer: None,
            },
        )
        .unwrap();
        super::apply_link_change(
            "eth0",
            &LinkChange {
                up: Some(false),
                mtu: Some(1400),
                metric: Some(2),
                tx_queue_len: Some(500),
                address: None,
                new_name: None,
                arp: Some(false),
                multicast: Some(true),
                allmulti: Some(true),
                promisc: Some(true),
                dynamic: Some(true),
                trailers: Some(false),
                mem_start: Some(0xfe000000),
                io_addr: Some(0x300),
                irq: Some(5),
            },
        )
        .unwrap();
        add_route(&RouteInfo {
            family: AddressFamily::Inet4,
            destination: String::from("0.0.0.0"),
            prefix_len: 0,
            gateway: Some(String::from("10.0.0.1")),
            dev: String::from("eth0"),
            source: Some(String::from("10.0.0.2")),
            metric: Some(10),
            table: Some(100),
            protocol: Some(libc::RTPROT_STATIC),
        })
        .unwrap();
        del_route(&RouteInfo {
            family: AddressFamily::Inet4,
            destination: String::from("10.1.0.0"),
            prefix_len: 16,
            gateway: None,
            dev: String::from("eth0"),
            source: None,
            metric: None,
            table: None,
            protocol: None,
        })
        .unwrap();
        add_address(
            "eth0",
            &InterfaceAddress {
                family: AddressFamily::Inet6,
                address: String::from("2001:db8::2"),
                prefix_len: 64,
                peer: None,
                broadcast: None,
                label: None,
                scope: None,
            },
        )
        .unwrap();
        remove_address("eth0", AddressFamily::Inet6, "2001:db8::2").unwrap();
        let state = state::read_state(&path).unwrap();
        // SAFETY: the test holds the global env lock for the full mutation window.
        unsafe { std::env::remove_var("SEED_NET_STATE") };
        assert_eq!(state.interfaces[0].mtu, 1400);
        assert_eq!(state.interfaces[0].metric, 2);
        assert_eq!(state.interfaces[0].tx_queue_len, 500);
        assert_eq!(state.interfaces[0].mem_start, 0xfe000000);
        assert_eq!(state.interfaces[0].io_addr, 0x300);
        assert_eq!(state.interfaces[0].irq, 5);
        assert_eq!(state.interfaces[0].addresses[0].address, "10.0.0.2");
        assert_eq!(state.routes.len(), 1);
        assert_eq!(state.routes[0].source.as_deref(), Some("10.0.0.2"));
        assert_eq!(state.routes[0].metric, Some(10));
        assert_eq!(state.routes[0].table, Some(100));
        assert_eq!(state.routes[0].protocol, Some(libc::RTPROT_STATIC));
        let _ = fs::remove_file(path);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn rejects_malformed_state_backend_records() {
        let path = std::env::temp_dir().join(format!("seed-net-bad-{}", std::process::id()));
        fs::write(&path, "iface\tbad\teth0\t1\t1500\t\t0\t0\t0\t0\n").unwrap();

        let err = state::read_state(&path).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(
            err.to_string()
                .contains("malformed network state at line 1")
        );

        let _ = fs::remove_file(path);
    }
}
