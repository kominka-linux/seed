use std::fs;
use std::io;
use std::path::Path;

use super::{
    AddressFamily, InterfaceAddress, InterfaceInfo, InterfaceStats, Ipv4Change, LinkChange,
    NeighborInfo, RouteInfo, RuleInfo, apply_flag_change,
};

#[derive(Clone, Debug, Default)]
pub(super) struct State {
    pub(super) interfaces: Vec<InterfaceInfo>,
    pub(super) routes: Vec<RouteInfo>,
    pub(super) neighbors: Vec<NeighborInfo>,
    pub(super) rules: Vec<RuleInfo>,
}

pub(super) fn add_address(path: &Path, name: &str, address: &InterfaceAddress) -> io::Result<()> {
    let mut state = read_state(path)?;
    let interface = interface_mut(&mut state, name)?;
    interface.addresses.retain(|existing| {
        !(existing.family == address.family && existing.address == address.address)
    });
    interface.addresses.push(address.clone());
    sort_interface_addresses(interface);
    write_state(path, &state)
}

pub(super) fn remove_address(
    path: &Path,
    name: &str,
    family: AddressFamily,
    address: &str,
) -> io::Result<()> {
    let mut state = read_state(path)?;
    let interface = interface_mut(&mut state, name)?;
    interface
        .addresses
        .retain(|existing| !(existing.family == family && existing.address == address));
    write_state(path, &state)
}

pub(super) fn list_interfaces(path: &Path) -> io::Result<Vec<InterfaceInfo>> {
    let mut interfaces = read_state(path)?.interfaces;
    interfaces.sort_by(|left, right| {
        left.index
            .cmp(&right.index)
            .then(left.name.cmp(&right.name))
    });
    Ok(interfaces)
}

pub(super) fn list_routes(path: &Path) -> io::Result<Vec<RouteInfo>> {
    let mut routes = read_state(path)?.routes;
    sort_routes(&mut routes);
    Ok(routes)
}

pub(super) fn list_neighbors(path: &Path) -> io::Result<Vec<NeighborInfo>> {
    let mut neighbors = read_state(path)?.neighbors;
    sort_neighbors(&mut neighbors);
    Ok(neighbors)
}

pub(super) fn list_rules(path: &Path) -> io::Result<Vec<RuleInfo>> {
    let mut rules = read_state(path)?.rules;
    sort_rules(&mut rules);
    Ok(rules)
}

pub(super) fn del_neighbor(path: &Path, neighbor: &NeighborInfo) -> io::Result<()> {
    let mut state = read_state(path)?;
    state.neighbors.retain(|existing| existing != neighbor);
    write_state(path, &state)
}

pub(super) fn add_rule(path: &Path, rule: &RuleInfo) -> io::Result<()> {
    let mut state = read_state(path)?;
    state.rules.retain(|existing| existing != rule);
    state.rules.push(rule.clone());
    sort_rules(&mut state.rules);
    write_state(path, &state)
}

pub(super) fn del_rule(path: &Path, rule: &RuleInfo) -> io::Result<()> {
    let mut state = read_state(path)?;
    state.rules.retain(|existing| existing != rule);
    write_state(path, &state)
}

pub(super) fn apply_link_change(path: &Path, name: &str, change: &LinkChange) -> io::Result<()> {
    let mut state = read_state(path)?;
    let interface = interface_mut(&mut state, name)?;
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
    if let Some(metric) = change.metric {
        interface.metric = metric;
    }
    if let Some(tx_queue_len) = change.tx_queue_len {
        interface.tx_queue_len = tx_queue_len;
    }
    if let Some(mem_start) = change.mem_start {
        interface.mem_start = mem_start;
    }
    if let Some(io_addr) = change.io_addr {
        interface.io_addr = io_addr;
    }
    if let Some(irq) = change.irq {
        interface.irq = irq;
    }
    if let Some(address) = &change.address {
        interface.mac = Some(address.clone());
    }
    apply_flag_change(
        &mut interface.flags,
        libc::IFF_NOARP as u32,
        change.arp.map(|value| !value),
    );
    apply_flag_change(
        &mut interface.flags,
        libc::IFF_MULTICAST as u32,
        change.multicast,
    );
    apply_flag_change(
        &mut interface.flags,
        libc::IFF_ALLMULTI as u32,
        change.allmulti,
    );
    apply_flag_change(
        &mut interface.flags,
        libc::IFF_PROMISC as u32,
        change.promisc,
    );
    apply_flag_change(
        &mut interface.flags,
        libc::IFF_DYNAMIC as u32,
        change.dynamic,
    );
    apply_flag_change(
        &mut interface.flags,
        libc::IFF_NOTRAILERS as u32,
        change.trailers.map(|value| !value),
    );
    if let Some(new_name) = &change.new_name {
        interface.name = new_name.clone();
        for route in &mut state.routes {
            if route.dev == name {
                route.dev = new_name.clone();
            }
        }
    }
    write_state(path, &state)
}

pub(super) fn set_ipv4(path: &Path, name: &str, change: &Ipv4Change) -> io::Result<()> {
    let mut state = read_state(path)?;
    let interface = interface_mut(&mut state, name)?;
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
            label: None,
            scope: None,
        });
    }
    sort_interface_addresses(interface);
    write_state(path, &state)
}

pub(super) fn add_route(path: &Path, route: &RouteInfo) -> io::Result<()> {
    let mut state = read_state(path)?;
    state.routes.retain(|existing| existing != route);
    state.routes.push(route.clone());
    sort_routes(&mut state.routes);
    write_state(path, &state)
}

pub(super) fn del_route(path: &Path, route: &RouteInfo) -> io::Result<()> {
    let mut state = read_state(path)?;
    state.routes.retain(|existing| existing != route);
    write_state(path, &state)
}

pub(super) fn read_state(path: &Path) -> io::Result<State> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(State::default()),
        Err(err) => return Err(err),
    };

    let mut state = State::default();
    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        match fields.first().copied() {
            Some("iface") => parse_interface_line(&mut state, line_no, &fields)?,
            Some("addr") => parse_address_line(&mut state, line_no, &fields)?,
            Some("route") => parse_route_line(&mut state, line_no, &fields)?,
            Some("neigh") => parse_neighbor_line(&mut state, line_no, &fields)?,
            Some("rule") => parse_rule_line(&mut state, line_no, &fields)?,
            Some(kind) => {
                return Err(invalid_state(
                    line_no,
                    format!("unknown record type '{kind}'"),
                ));
            }
            None => {}
        }
    }

    for interface in &mut state.interfaces {
        sort_interface_addresses(interface);
    }
    sort_neighbors(&mut state.neighbors);
    sort_rules(&mut state.rules);
    Ok(state)
}

pub(super) fn write_state(path: &Path, state: &State) -> io::Result<()> {
    let mut text = String::new();
    for interface in &state.interfaces {
        text.push_str(&format!(
            "iface\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            interface.index,
            interface.name,
            interface.flags,
            interface.mtu,
            interface.metric,
            interface.tx_queue_len,
            interface.mem_start,
            interface.io_addr,
            interface.irq,
            interface.mac.as_deref().unwrap_or(""),
            interface.stats.rx_bytes,
            interface.stats.rx_packets,
            interface.stats.tx_bytes,
            interface.stats.tx_packets
        ));
        for address in &interface.addresses {
            text.push_str(&format!(
                "addr\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                interface.name,
                match address.family {
                    AddressFamily::Inet4 => "inet",
                    AddressFamily::Inet6 => "inet6",
                },
                address.address,
                address.prefix_len,
                address.peer.as_deref().unwrap_or(""),
                address.broadcast.as_deref().unwrap_or(""),
                address.label.as_deref().unwrap_or(""),
                address.scope.as_deref().unwrap_or("")
            ));
        }
    }
    for route in &state.routes {
        text.push_str(&format!(
            "route\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            match route.family {
                AddressFamily::Inet4 => "inet",
                AddressFamily::Inet6 => "inet6",
            },
            route.destination,
            route.prefix_len,
            route.gateway.as_deref().unwrap_or(""),
            route.dev,
            route.source.as_deref().unwrap_or(""),
            route
                .metric
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or(""),
            route
                .table
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or(""),
            route
                .protocol
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or("")
        ));
    }
    for neighbor in &state.neighbors {
        text.push_str(&format!(
            "neigh\t{}\t{}\t{}\t{}\t{}\n",
            match neighbor.family {
                AddressFamily::Inet4 => "inet",
                AddressFamily::Inet6 => "inet6",
            },
            neighbor.address,
            neighbor.dev,
            neighbor.lladdr.as_deref().unwrap_or(""),
            neighbor.state
        ));
    }
    for rule in &state.rules {
        text.push_str(&format!(
            "rule\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            match rule.family {
                AddressFamily::Inet4 => "inet",
                AddressFamily::Inet6 => "inet6",
            },
            rule.from.as_deref().unwrap_or(""),
            rule.from_prefix_len,
            rule.to.as_deref().unwrap_or(""),
            rule.to_prefix_len,
            rule.iif.as_deref().unwrap_or(""),
            rule.oif.as_deref().unwrap_or(""),
            rule.fwmark
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or(""),
            rule.priority
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or(""),
            rule.table
        ));
    }
    fs::write(path, text)
}

fn interface_mut<'a>(state: &'a mut State, name: &str) -> io::Result<&'a mut InterfaceInfo> {
    state
        .interfaces
        .iter_mut()
        .find(|interface| interface.name == name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "interface not found"))
}

fn parse_interface_line(state: &mut State, line_no: usize, fields: &[&str]) -> io::Result<()> {
    match fields {
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
        ] => state.interfaces.push(InterfaceInfo {
            index: parse_required(line_no, "interface index", index)?,
            name: (*name).to_string(),
            flags: parse_required(line_no, "interface flags", flags)?,
            mtu: parse_required(line_no, "interface mtu", mtu)?,
            metric: 0,
            tx_queue_len: 0,
            mem_start: 0,
            io_addr: 0,
            irq: 0,
            mac: optional_text(mac),
            addresses: Vec::new(),
            stats: InterfaceStats {
                rx_bytes: parse_required(line_no, "rx_bytes", rx_bytes)?,
                rx_packets: parse_required(line_no, "rx_packets", rx_packets)?,
                tx_bytes: parse_required(line_no, "tx_bytes", tx_bytes)?,
                tx_packets: parse_required(line_no, "tx_packets", tx_packets)?,
            },
        }),
        [
            "iface",
            index,
            name,
            flags,
            mtu,
            metric,
            tx_queue_len,
            mac,
            rx_bytes,
            rx_packets,
            tx_bytes,
            tx_packets,
        ] => state.interfaces.push(InterfaceInfo {
            index: parse_required(line_no, "interface index", index)?,
            name: (*name).to_string(),
            flags: parse_required(line_no, "interface flags", flags)?,
            mtu: parse_required(line_no, "interface mtu", mtu)?,
            metric: parse_required(line_no, "interface metric", metric)?,
            tx_queue_len: parse_required(line_no, "interface tx_queue_len", tx_queue_len)?,
            mem_start: 0,
            io_addr: 0,
            irq: 0,
            mac: optional_text(mac),
            addresses: Vec::new(),
            stats: InterfaceStats {
                rx_bytes: parse_required(line_no, "rx_bytes", rx_bytes)?,
                rx_packets: parse_required(line_no, "rx_packets", rx_packets)?,
                tx_bytes: parse_required(line_no, "tx_bytes", tx_bytes)?,
                tx_packets: parse_required(line_no, "tx_packets", tx_packets)?,
            },
        }),
        [
            "iface",
            index,
            name,
            flags,
            mtu,
            metric,
            tx_queue_len,
            mem_start,
            io_addr,
            irq,
            mac,
            rx_bytes,
            rx_packets,
            tx_bytes,
            tx_packets,
        ] => state.interfaces.push(InterfaceInfo {
            index: parse_required(line_no, "interface index", index)?,
            name: (*name).to_string(),
            flags: parse_required(line_no, "interface flags", flags)?,
            mtu: parse_required(line_no, "interface mtu", mtu)?,
            metric: parse_required(line_no, "interface metric", metric)?,
            tx_queue_len: parse_required(line_no, "interface tx_queue_len", tx_queue_len)?,
            mem_start: parse_required(line_no, "interface mem_start", mem_start)?,
            io_addr: parse_required(line_no, "interface io_addr", io_addr)?,
            irq: parse_required(line_no, "interface irq", irq)?,
            mac: optional_text(mac),
            addresses: Vec::new(),
            stats: InterfaceStats {
                rx_bytes: parse_required(line_no, "rx_bytes", rx_bytes)?,
                rx_packets: parse_required(line_no, "rx_packets", rx_packets)?,
                tx_bytes: parse_required(line_no, "tx_bytes", tx_bytes)?,
                tx_packets: parse_required(line_no, "tx_packets", tx_packets)?,
            },
        }),
        _ => return Err(invalid_state(line_no, "invalid iface record")),
    }
    Ok(())
}

fn parse_address_line(state: &mut State, line_no: usize, fields: &[&str]) -> io::Result<()> {
    let (name, family, address, prefix_len, peer, broadcast, label, scope) = match fields {
        ["addr", name, family, address, prefix_len, peer, broadcast] => (
            *name,
            *family,
            *address,
            *prefix_len,
            *peer,
            *broadcast,
            "",
            "",
        ),
        [
            "addr",
            name,
            family,
            address,
            prefix_len,
            peer,
            broadcast,
            label,
            scope,
        ] => (
            *name,
            *family,
            *address,
            *prefix_len,
            *peer,
            *broadcast,
            *label,
            *scope,
        ),
        _ => return Err(invalid_state(line_no, "invalid addr record")),
    };
    let interface = interface_mut(state, name).map_err(|_| {
        invalid_state(
            line_no,
            format!("address record references unknown interface '{name}'"),
        )
    })?;
    interface.addresses.push(InterfaceAddress {
        family: parse_family(line_no, family)?,
        address: address.to_string(),
        prefix_len: parse_required(line_no, "address prefix length", prefix_len)?,
        peer: optional_text(peer),
        broadcast: optional_text(broadcast),
        label: optional_text(label),
        scope: optional_text(scope),
    });
    Ok(())
}

fn parse_route_line(state: &mut State, line_no: usize, fields: &[&str]) -> io::Result<()> {
    let route = match fields {
        ["route", family, destination, prefix_len, gateway, dev] => RouteInfo {
            family: parse_family(line_no, family)?,
            destination: (*destination).to_string(),
            prefix_len: parse_required(line_no, "route prefix length", prefix_len)?,
            gateway: optional_text(gateway),
            dev: (*dev).to_string(),
            source: None,
            metric: None,
            table: None,
            protocol: None,
        },
        [
            "route",
            family,
            destination,
            prefix_len,
            gateway,
            dev,
            source,
            metric,
            table,
            protocol,
        ] => RouteInfo {
            family: parse_family(line_no, family)?,
            destination: (*destination).to_string(),
            prefix_len: parse_required(line_no, "route prefix length", prefix_len)?,
            gateway: optional_text(gateway),
            dev: (*dev).to_string(),
            source: optional_text(source),
            metric: optional_parsed(line_no, "route metric", metric)?,
            table: optional_parsed(line_no, "route table", table)?,
            protocol: optional_parsed(line_no, "route protocol", protocol)?,
        },
        _ => return Err(invalid_state(line_no, "invalid route record")),
    };
    state.routes.push(route);
    Ok(())
}

fn parse_neighbor_line(state: &mut State, line_no: usize, fields: &[&str]) -> io::Result<()> {
    let ["neigh", family, address, dev, lladdr, nud_state] = fields else {
        return Err(invalid_state(line_no, "invalid neigh record"));
    };
    state.neighbors.push(NeighborInfo {
        family: parse_family(line_no, family)?,
        address: (*address).to_string(),
        dev: (*dev).to_string(),
        lladdr: optional_text(lladdr),
        state: parse_required(line_no, "neighbor state", nud_state)?,
    });
    Ok(())
}

fn parse_rule_line(state: &mut State, line_no: usize, fields: &[&str]) -> io::Result<()> {
    let [
        "rule",
        family,
        from,
        from_prefix_len,
        to,
        to_prefix_len,
        iif,
        oif,
        fwmark,
        priority,
        table,
    ] = fields
    else {
        return Err(invalid_state(line_no, "invalid rule record"));
    };
    state.rules.push(RuleInfo {
        family: parse_family(line_no, family)?,
        from: optional_text(from),
        from_prefix_len: parse_required(line_no, "rule from prefix length", from_prefix_len)?,
        to: optional_text(to),
        to_prefix_len: parse_required(line_no, "rule to prefix length", to_prefix_len)?,
        iif: optional_text(iif),
        oif: optional_text(oif),
        fwmark: optional_parsed(line_no, "rule fwmark", fwmark)?,
        priority: optional_parsed(line_no, "rule priority", priority)?,
        table: parse_required(line_no, "rule table", table)?,
    });
    Ok(())
}

fn parse_family(line_no: usize, value: &str) -> io::Result<AddressFamily> {
    match value {
        "inet" => Ok(AddressFamily::Inet4),
        "inet6" => Ok(AddressFamily::Inet6),
        other => Err(invalid_state(
            line_no,
            format!("unknown address family '{other}'"),
        )),
    }
}

fn parse_required<T>(line_no: usize, field: &str, value: &str) -> io::Result<T>
where
    T: std::str::FromStr,
{
    value
        .parse()
        .map_err(|_| invalid_state(line_no, format!("invalid {field} value '{value}'")))
}

fn optional_parsed<T>(line_no: usize, field: &str, value: &str) -> io::Result<Option<T>>
where
    T: std::str::FromStr,
{
    if value.is_empty() {
        Ok(None)
    } else {
        parse_required(line_no, field, value).map(Some)
    }
}

fn optional_text(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn sort_interface_addresses(interface: &mut InterfaceInfo) {
    interface.addresses.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.address.cmp(&right.address))
    });
}

fn sort_routes(routes: &mut [RouteInfo]) {
    routes.sort_by(|left, right| {
        left.dev
            .cmp(&right.dev)
            .then(left.destination.cmp(&right.destination))
    });
}

fn sort_neighbors(neighbors: &mut [NeighborInfo]) {
    neighbors.sort_by(|left, right| {
        left.family
            .cmp(&right.family)
            .then(left.dev.cmp(&right.dev))
            .then(left.address.cmp(&right.address))
    });
}

fn sort_rules(rules: &mut [RuleInfo]) {
    rules.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then(left.family.cmp(&right.family))
            .then(left.table.cmp(&right.table))
    });
}

fn invalid_state(line_no: usize, message: impl Into<String>) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "malformed network state at line {line_no}: {}",
            message.into()
        ),
    )
}
