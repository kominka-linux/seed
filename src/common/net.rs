#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

#[cfg(target_os = "linux")]
use std::ffi::{CStr, CString};
#[cfg(target_os = "linux")]
use std::fs;
use std::io;
#[cfg(target_os = "linux")]
use std::mem;
use std::net::Ipv4Addr;
#[cfg(target_os = "linux")]
use std::net::Ipv6Addr;
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

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
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct LinkChange {
    pub(crate) up: Option<bool>,
    pub(crate) mtu: Option<u32>,
    pub(crate) address: Option<String>,
    pub(crate) new_name: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Ipv4Change {
    pub(crate) address: Option<Ipv4Addr>,
    pub(crate) prefix_len: Option<u8>,
    pub(crate) broadcast: Option<Option<Ipv4Addr>>,
    pub(crate) peer: Option<Ipv4Addr>,
}

#[cfg(target_os = "linux")]
pub(crate) fn list_interfaces() -> io::Result<Vec<InterfaceInfo>> {
    if let Some(path) = state_path() {
        let state = read_state(&path)?;
        let mut interfaces = state.interfaces;
        interfaces.sort_by(|left, right| {
            left.index
                .cmp(&right.index)
                .then(left.name.cmp(&right.name))
        });
        return Ok(interfaces);
    }
    live_list_interfaces()
}

#[cfg(target_os = "linux")]
pub(crate) fn list_routes() -> io::Result<Vec<RouteInfo>> {
    if let Some(path) = state_path() {
        let mut routes = read_state(&path)?.routes;
        routes.sort_by(|left, right| {
            left.dev
                .cmp(&right.dev)
                .then(left.destination.cmp(&right.destination))
        });
        return Ok(routes);
    }
    live_list_routes()
}

#[cfg(target_os = "linux")]
pub(crate) fn apply_link_change(name: &str, change: &LinkChange) -> io::Result<()> {
    if let Some(path) = state_path() {
        let mut state = read_state(&path)?;
        let interface = state
            .interfaces
            .iter_mut()
            .find(|interface| interface.name == name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "interface not found"))?;
        if let Some(up) = change.up {
            if up {
                interface.flags |= libc::IFF_UP as u32;
            } else {
                interface.flags &= !(libc::IFF_UP as u32);
            }
        }
        if let Some(mtu) = change.mtu {
            interface.mtu = mtu;
        }
        if let Some(address) = &change.address {
            interface.mac = Some(address.clone());
        }
        if let Some(new_name) = &change.new_name {
            interface.name = new_name.clone();
            for route in &mut state.routes {
                if route.dev == name {
                    route.dev = new_name.clone();
                }
            }
        }
        return write_state(&path, &state);
    }
    live_apply_link_change(name, change)
}

#[cfg(target_os = "linux")]
pub(crate) fn set_ipv4(name: &str, change: &Ipv4Change) -> io::Result<()> {
    if let Some(path) = state_path() {
        let mut state = read_state(&path)?;
        let interface = state
            .interfaces
            .iter_mut()
            .find(|interface| interface.name == name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "interface not found"))?;
        interface
            .addresses
            .retain(|address| address.family != AddressFamily::Inet4);
        if let Some(address) = change.address {
            interface.addresses.push(InterfaceAddress {
                family: AddressFamily::Inet4,
                address: address.to_string(),
                prefix_len: change.prefix_len.unwrap_or(32),
                peer: change.peer.map(|peer| peer.to_string()),
                broadcast: match change.broadcast {
                    Some(Some(value)) => Some(value.to_string()),
                    Some(None) => None,
                    None => None,
                },
            });
        }
        interface.addresses.sort_by(|left, right| {
            left.family
                .cmp(&right.family)
                .then(left.address.cmp(&right.address))
        });
        return write_state(&path, &state);
    }
    live_set_ipv4(name, change)
}

#[cfg(target_os = "linux")]
pub(crate) fn clear_ipv4(name: &str) -> io::Result<()> {
    set_ipv4(
        name,
        &Ipv4Change {
            address: None,
            prefix_len: Some(0),
            broadcast: Some(None),
            peer: None,
        },
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn add_route(route: &RouteInfo) -> io::Result<()> {
    if let Some(path) = state_path() {
        let mut state = read_state(&path)?;
        state.routes.retain(|existing| existing != route);
        state.routes.push(route.clone());
        state.routes.sort_by(|left, right| {
            left.dev
                .cmp(&right.dev)
                .then(left.destination.cmp(&right.destination))
        });
        return write_state(&path, &state);
    }
    live_route_change(route, libc::SIOCADDRT)
}

#[cfg(target_os = "linux")]
pub(crate) fn del_route(route: &RouteInfo) -> io::Result<()> {
    if let Some(path) = state_path() {
        let mut state = read_state(&path)?;
        state.routes.retain(|existing| existing != route);
        return write_state(&path, &state);
    }
    live_route_change(route, libc::SIOCDELRT)
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
    if flags & libc::IFF_PROMISC as u32 != 0 {
        names.push("PROMISC");
    }
    if flags & libc::IFF_POINTOPOINT as u32 != 0 {
        names.push("POINTOPOINT");
    }
    names
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Default)]
struct State {
    interfaces: Vec<InterfaceInfo>,
    routes: Vec<RouteInfo>,
}

#[cfg(target_os = "linux")]
fn state_path() -> Option<PathBuf> {
    std::env::var_os("SEED_NET_STATE").map(PathBuf::from)
}

#[cfg(target_os = "linux")]
fn read_state(path: &Path) -> io::Result<State> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(State::default()),
        Err(err) => return Err(err),
    };
    let mut state = State::default();
    for line in text.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.as_slice() {
            [
                "iface",
                index,
                name,
                flags,
                mtu,
                mac,
                rx_bytes,
                rx_packets,
                tx_bytes,
                tx_packets,
            ] => {
                state.interfaces.push(InterfaceInfo {
                    index: index.parse().unwrap_or(0),
                    name: (*name).to_string(),
                    flags: flags.parse().unwrap_or(0),
                    mtu: mtu.parse().unwrap_or(0),
                    mac: (!mac.is_empty()).then(|| (*mac).to_string()),
                    addresses: Vec::new(),
                    stats: InterfaceStats {
                        rx_bytes: rx_bytes.parse().unwrap_or(0),
                        rx_packets: rx_packets.parse().unwrap_or(0),
                        tx_bytes: tx_bytes.parse().unwrap_or(0),
                        tx_packets: tx_packets.parse().unwrap_or(0),
                    },
                });
            }
            ["addr", name, family, address, prefix_len, peer, broadcast] => {
                if let Some(interface) = state
                    .interfaces
                    .iter_mut()
                    .find(|interface| interface.name == *name)
                {
                    interface.addresses.push(InterfaceAddress {
                        family: if *family == "inet6" {
                            AddressFamily::Inet6
                        } else {
                            AddressFamily::Inet4
                        },
                        address: (*address).to_string(),
                        prefix_len: prefix_len.parse().unwrap_or(0),
                        peer: (!peer.is_empty()).then(|| (*peer).to_string()),
                        broadcast: (!broadcast.is_empty()).then(|| (*broadcast).to_string()),
                    });
                }
            }
            ["route", family, destination, prefix_len, gateway, dev] => {
                state.routes.push(RouteInfo {
                    family: if *family == "inet6" {
                        AddressFamily::Inet6
                    } else {
                        AddressFamily::Inet4
                    },
                    destination: (*destination).to_string(),
                    prefix_len: prefix_len.parse().unwrap_or(0),
                    gateway: (!gateway.is_empty()).then(|| (*gateway).to_string()),
                    dev: (*dev).to_string(),
                });
            }
            _ => {}
        }
    }
    for interface in &mut state.interfaces {
        interface.addresses.sort_by(|left, right| {
            left.family
                .cmp(&right.family)
                .then(left.address.cmp(&right.address))
        });
    }
    Ok(state)
}

#[cfg(target_os = "linux")]
fn write_state(path: &Path, state: &State) -> io::Result<()> {
    let mut text = String::new();
    for interface in &state.interfaces {
        text.push_str(&format!(
            "iface\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            interface.index,
            interface.name,
            interface.flags,
            interface.mtu,
            interface.mac.as_deref().unwrap_or(""),
            interface.stats.rx_bytes,
            interface.stats.rx_packets,
            interface.stats.tx_bytes,
            interface.stats.tx_packets
        ));
        for address in &interface.addresses {
            text.push_str(&format!(
                "addr\t{}\t{}\t{}\t{}\t{}\t{}\n",
                interface.name,
                match address.family {
                    AddressFamily::Inet4 => "inet",
                    AddressFamily::Inet6 => "inet6",
                },
                address.address,
                address.prefix_len,
                address.peer.as_deref().unwrap_or(""),
                address.broadcast.as_deref().unwrap_or("")
            ));
        }
    }
    for route in &state.routes {
        text.push_str(&format!(
            "route\t{}\t{}\t{}\t{}\t{}\n",
            match route.family {
                AddressFamily::Inet4 => "inet",
                AddressFamily::Inet6 => "inet6",
            },
            route.destination,
            route.prefix_len,
            route.gateway.as_deref().unwrap_or(""),
            route.dev
        ));
    }
    fs::write(path, text)
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
    let mut routes = parse_ipv4_routes()?;
    routes.extend(parse_ipv6_routes()?);
    routes.sort_by(|left, right| {
        left.dev
            .cmp(&right.dev)
            .then(left.destination.cmp(&right.destination))
    });
    Ok(routes)
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
            })
        })
        .collect())
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
    if let Some(up) = change.up {
        let mut flags = get_flags(name)?;
        if up {
            flags |= libc::IFF_UP as u32;
        } else {
            flags &= !(libc::IFF_UP as u32);
        }
        set_flags(name, flags)?;
    }
    if let Some(mtu) = change.mtu {
        set_mtu(name, mtu)?;
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
fn live_route_change(route: &RouteInfo, request: libc::c_ulong) -> io::Result<()> {
    if route.family != AddressFamily::Inet4 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "IPv6 route changes are not supported",
        ));
    }
    let mut entry = unsafe { mem::zeroed::<libc::rtentry>() };
    entry.rt_flags = libc::RTF_UP
        | if route.gateway.is_some() {
            libc::RTF_GATEWAY
        } else {
            0
        };
    if route.prefix_len == 32 {
        entry.rt_flags |= libc::RTF_HOST;
    }
    entry.rt_dst = sockaddr_from_ipv4(route.destination.parse().unwrap_or(Ipv4Addr::UNSPECIFIED));
    entry.rt_genmask = sockaddr_from_ipv4(prefix_to_netmask(route.prefix_len));
    entry.rt_gateway = sockaddr_from_ipv4(
        route
            .gateway
            .as_deref()
            .unwrap_or("0.0.0.0")
            .parse()
            .unwrap_or(Ipv4Addr::UNSPECIFIED),
    );
    let dev = CString::new(route.dev.as_str())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "device contains NUL byte"))?;
    entry.rt_dev = dev.as_ptr() as *mut libc::c_char;
    let fd = socket_fd()?;
    // SAFETY: `entry` points to a valid rtentry for the duration of the ioctl.
    let rc = unsafe { libc::ioctl(fd, request as _, &entry) };
    let err = io::Error::last_os_error();
    close_fd(fd);
    if rc == 0 { Ok(()) } else { Err(err) }
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
#[repr(C)]
union IfreqUnion {
    addr: libc::sockaddr,
    flags: libc::c_short,
    mtu: libc::c_int,
    name: [libc::c_char; libc::IFNAMSIZ],
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
        AddressFamily, InterfaceInfo, LinkChange, RouteInfo, add_route, broadcast_for, del_route,
        format_mac, interface_flag_names, parse_mac, parse_prefix, prefix_to_netmask, read_state,
        set_ipv4, write_state,
    };
    #[cfg(target_os = "linux")]
    use std::fs;

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
        let path = std::env::temp_dir().join(format!("seed-net-{}", std::process::id()));
        let mut state = super::State::default();
        state.interfaces.push(InterfaceInfo {
            index: 1,
            name: String::from("eth0"),
            flags: libc::IFF_UP as u32,
            mtu: 1500,
            mac: Some(String::from("02:00:00:00:00:01")),
            addresses: Vec::new(),
            stats: super::InterfaceStats::default(),
        });
        write_state(&path, &state).unwrap();
        // SAFETY: test-only env mutation with no concurrent access.
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
                address: None,
                new_name: None,
            },
        )
        .unwrap();
        add_route(&RouteInfo {
            family: AddressFamily::Inet4,
            destination: String::from("0.0.0.0"),
            prefix_len: 0,
            gateway: Some(String::from("10.0.0.1")),
            dev: String::from("eth0"),
        })
        .unwrap();
        del_route(&RouteInfo {
            family: AddressFamily::Inet4,
            destination: String::from("10.1.0.0"),
            prefix_len: 16,
            gateway: None,
            dev: String::from("eth0"),
        })
        .unwrap();
        let state = read_state(&path).unwrap();
        // SAFETY: paired with set_var above in this isolated test.
        unsafe { std::env::remove_var("SEED_NET_STATE") };
        assert_eq!(state.interfaces[0].mtu, 1400);
        assert_eq!(state.interfaces[0].addresses[0].address, "10.0.0.2");
        assert_eq!(state.routes.len(), 1);
        let _ = fs::remove_file(path);
    }
}
