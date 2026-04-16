
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::common::args::ArgCursor;
use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::net::{
    AddressFamily, InterfaceAddress, LinkChange, NeighborInfo, RouteInfo, RuleInfo, add_address,
    add_route, add_rule, apply_link_change, broadcast_for, del_neighbor, del_route, del_rule,
    format_mac, interface_flag_names, list_interfaces, list_neighbors, list_routes, list_rules,
    parse_mac, parse_prefix, remove_address,
};

const APPLET: &str = "ip";
const RTPROT_RA: u8 = 9;
const NUD_INCOMPLETE: u16 = 0x01;
const NUD_REACHABLE: u16 = 0x02;
const NUD_STALE: u16 = 0x04;
const NUD_DELAY: u16 = 0x08;
const NUD_PROBE: u16 = 0x10;
const NUD_FAILED: u16 = 0x20;
const NUD_NOARP: u16 = 0x40;
const NUD_PERMANENT: u16 = 0x80;

#[derive(Clone, Copy)]
struct GlobalOptions {
    family: Option<AddressFamily>,
    link_family: bool,
    oneline: bool,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

fn run(args: &[std::ffi::OsString]) -> Result<(), Vec<AppletError>> {
    let (options, command_args) = parse_global_options(args)?;
    run_linux(options, &command_args)
}

fn run_linux(options: GlobalOptions, args: &[String]) -> Result<(), Vec<AppletError>> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(vec![AppletError::new(APPLET, "missing command")]);
    };
    if options.link_family && command != "link" {
        return Err(vec![AppletError::new(
            APPLET,
            "family 'link' is only valid with 'ip link'",
        )]);
    }
    match command {
        "addr" | "address" => run_addr(options.family, options.oneline, &args[1..]),
        "link" => run_link(options.oneline, &args[1..]),
        "neigh" => run_neigh(options.family, &args[1..]),
        "rule" => run_rule(options.family, &args[1..]),
        "route" => run_route(options.family, &args[1..]),
        _ => Err(vec![AppletError::new(
            APPLET,
            format!("unsupported command '{command}'"),
        )]),
    }
}

fn parse_global_options(args: &[std::ffi::OsString]) -> Result<(GlobalOptions, Vec<String>), Vec<AppletError>> {
    let mut options = GlobalOptions {
        family: None,
        link_family: false,
        oneline: false,
    };
    let mut cursor = ArgCursor::new(args);
    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "-4" => options.family = Some(AddressFamily::Inet4),
            "-6" => options.family = Some(AddressFamily::Inet6),
            "-o" | "-oneline" => options.oneline = true,
            "-f" | "-family" => {
                let value = cursor.next_value(APPLET, "f")?;
                match value {
                    "inet" => options.family = Some(AddressFamily::Inet4),
                    "inet6" => options.family = Some(AddressFamily::Inet6),
                    "link" => options.link_family = true,
                    _ => {
                        return Err(vec![AppletError::new(
                            APPLET,
                            format!("invalid family '{value}'"),
                        )])
                    }
                }
            }
            _ => {
                let mut command_args = vec![arg.to_string()];
                for remaining in cursor.remaining() {
                    command_args
                        .push(crate::common::args::os_to_string(APPLET, remaining.as_os_str())?);
                }
                return Ok((options, command_args));
            }
        }
    }
    Ok((options, Vec::new()))
}

fn next_string(cursor: &mut ArgCursor<'_, String>, option: &str) -> Result<String, Vec<AppletError>> {
    Ok(cursor.next_value(APPLET, option)?.to_string())
}

fn run_addr(
    family: Option<AddressFamily>,
    oneline: bool,
    args: &[String],
) -> Result<(), Vec<AppletError>> {
    match args.first().map(String::as_str).unwrap_or("show") {
        "show" | "list" => {
            let filter = parse_addr_filter(&args[1..])?;
            let interfaces = list_interfaces().map_err(io_error("listing interfaces", None))?;
            for interface in interfaces {
                if filter.dev.as_deref().is_some_and(|dev| dev != interface.name) {
                    continue;
                }
                if has_visible_addresses(&interface, family, &filter) {
                    print_addr_interface(&interface, family, &filter, oneline);
                }
            }
            Ok(())
        }
        "flush" => {
            let filter = parse_addr_filter(&args[1..])?;
            for interface in list_interfaces().map_err(io_error("listing interfaces", None))? {
                if filter.dev.as_deref().is_some_and(|dev| dev != interface.name) {
                    continue;
                }
                for address in interface.addresses {
                    if family.is_some_and(|expected| expected != address.family)
                        || !matches_addr_filter(&address, &filter.prefix)
                        || !matches_label_scope(&address, &filter.label, &filter.scope)
                    {
                        continue;
                    }
                    remove_address(&interface.name, address.family, &address.address)
                        .map_err(io_error("clearing address", Some("interface".to_string())))?;
                }
            }
            Ok(())
        }
        "add" | "del" => {
            let delete = args[0] == "del";
            let change = parse_addr_change(&args[1..], family)?;
            match &change.prefix {
                ParsedPrefix::Inet4(address, prefix_len) => {
                    if delete {
                        remove_address(&change.dev, AddressFamily::Inet4, &address.to_string()).map_err(io_error(
                            "clearing IPv4 address",
                            Some("interface".to_string()),
                        ))?;
                    } else {
                        add_address(
                            &change.dev,
                            &InterfaceAddress {
                                family: AddressFamily::Inet4,
                                address: address.to_string(),
                                prefix_len: *prefix_len,
                                peer: change.peer.clone(),
                                broadcast: match &change.broadcast {
                                    Some(Some(value)) => Some(value.clone()),
                                    Some(None) => Some(broadcast_for(*address, *prefix_len).to_string()),
                                    None => Some(broadcast_for(*address, *prefix_len).to_string()),
                                },
                                label: change.label.clone(),
                                scope: change.scope.clone(),
                            },
                        )
                        .map_err(io_error(
                            "setting IPv4 address",
                            Some("interface".to_string()),
                        ))?;
                    }
                }
                ParsedPrefix::Inet6(address, prefix_len) => {
                    if delete {
                        remove_address(&change.dev, AddressFamily::Inet6, address).map_err(io_error(
                            "clearing IPv6 address",
                            Some("interface".to_string()),
                        ))?;
                    } else {
                        add_address(
                            &change.dev,
                            &InterfaceAddress {
                                family: AddressFamily::Inet6,
                                address: address.clone(),
                                prefix_len: *prefix_len,
                                peer: change.peer.clone(),
                                broadcast: None,
                                label: change.label.clone(),
                                scope: change.scope.clone(),
                            },
                        )
                        .map_err(io_error(
                            "setting IPv6 address",
                            Some("interface".to_string()),
                        ))?;
                    }
                }
            }
            Ok(())
        }
        other => Err(vec![AppletError::new(
            APPLET,
            format!("unsupported 'ip addr' command '{other}'"),
        )]),
    }
}

fn run_link(oneline: bool, args: &[String]) -> Result<(), Vec<AppletError>> {
    match args.first().map(String::as_str).unwrap_or("show") {
        "show" | "list" => {
            let dev = parse_optional_dev(&args[1..])?;
            let interfaces = list_interfaces().map_err(io_error("listing interfaces", None))?;
            for interface in interfaces {
                if dev.as_deref().is_some_and(|dev| dev != interface.name) {
                    continue;
                }
                print_link_interface(&interface, oneline);
            }
            Ok(())
        }
        "set" => {
            let (name, change) = parse_link_change(&args[1..])?;
            apply_link_change(&name, &change)
                .map_err(io_error("updating link", Some("interface".to_string())))?;
            Ok(())
        }
        other => Err(vec![AppletError::new(
            APPLET,
            format!("unsupported 'ip link' command '{other}'"),
        )]),
    }
}

fn run_route(family: Option<AddressFamily>, args: &[String]) -> Result<(), Vec<AppletError>> {
    match args.first().map(String::as_str).unwrap_or("show") {
        "show" | "list" => {
            for route in list_routes().map_err(io_error("listing routes", None))? {
                if family.is_none_or(|expected| expected == route.family) {
                    print_route(&route);
                }
            }
            Ok(())
        }
        "flush" => {
            let dev = parse_route_dev_filter(&args[1..])?;
            for route in list_routes().map_err(io_error("listing routes", None))? {
                if family.is_some_and(|expected| expected != route.family) {
                    continue;
                }
                if dev.as_deref().is_some_and(|expected| expected != route.dev) {
                    continue;
                }
                del_route(&route).map_err(io_error("deleting route", None))?;
            }
            Ok(())
        }
        "add" | "append" | "del" | "change" | "replace" => {
            let delete = args[0] == "del";
            let replace = matches!(args[0].as_str(), "change" | "replace");
            let route = parse_route_change(family, &args[1..])?;
            if delete {
                del_route(&route).map_err(io_error("deleting route", None))?;
            } else {
                if replace {
                    for existing in list_routes().map_err(io_error("listing routes", None))? {
                        if existing.family == route.family
                            && existing.destination == route.destination
                            && existing.prefix_len == route.prefix_len
                            && existing.dev == route.dev
                        {
                            del_route(&existing).map_err(io_error("deleting route", None))?;
                        }
                    }
                }
                add_route(&route).map_err(io_error("adding route", None))?;
            }
            Ok(())
        }
        other => Err(vec![AppletError::new(
            APPLET,
            format!("unsupported 'ip route' command '{other}'"),
        )]),
    }
}

#[derive(Default)]
struct NeighFilter {
    dev: Option<String>,
    prefix: Option<ParsedPrefix>,
    nud: Option<u16>,
}

fn run_neigh(family: Option<AddressFamily>, args: &[String]) -> Result<(), Vec<AppletError>> {
    match args.first().map(String::as_str).unwrap_or("show") {
        "show" | "list" => {
            let filter = parse_neigh_filter(&args[1..], family)?;
            for neighbor in list_neighbors().map_err(io_error("listing neighbors", None))? {
                if matches_neigh_filter(&neighbor, family, &filter) {
                    print_neigh(&neighbor);
                }
            }
            Ok(())
        }
        "flush" => {
            let filter = parse_neigh_filter(&args[1..], family)?;
            for neighbor in list_neighbors().map_err(io_error("listing neighbors", None))? {
                if matches_neigh_filter(&neighbor, family, &filter) {
                    del_neighbor(&neighbor).map_err(io_error("deleting neighbor", None))?;
                }
            }
            Ok(())
        }
        other => Err(vec![AppletError::new(
            APPLET,
            format!("unsupported 'ip neigh' command '{other}'"),
        )]),
    }
}

fn run_rule(family: Option<AddressFamily>, args: &[String]) -> Result<(), Vec<AppletError>> {
    match args.first().map(String::as_str).unwrap_or("list") {
        "show" | "list" => {
            for rule in list_rules().map_err(io_error("listing rules", None))? {
                if family.is_none_or(|expected| expected == rule.family) {
                    print_rule(&rule);
                }
            }
            Ok(())
        }
        "add" | "del" => {
            let delete = args[0] == "del";
            let rule = parse_rule_change(family, &args[1..])?;
            if delete {
                del_rule(&rule).map_err(io_error("deleting rule", None))?;
            } else {
                add_rule(&rule).map_err(io_error("adding rule", None))?;
            }
            Ok(())
        }
        other => Err(vec![AppletError::new(
            APPLET,
            format!("unsupported 'ip rule' command '{other}'"),
        )]),
    }
}

#[derive(Default)]
struct AddrFilter {
    dev: Option<String>,
    prefix: Option<ParsedPrefix>,
    label: Option<String>,
    scope: Option<String>,
}

fn parse_addr_filter(args: &[String]) -> Result<AddrFilter, Vec<AppletError>> {
    let mut filter = AddrFilter::default();
    let mut cursor = ArgCursor::new(args);
    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "dev" => filter.dev = Some(next_string(&mut cursor, "dev")?),
            "to" => filter.prefix = Some(parse_ip_prefix(cursor.next_value(APPLET, "to")?, None)?),
            "label" => filter.label = Some(next_string(&mut cursor, "label")?),
            "scope" => filter.scope = Some(next_string(&mut cursor, "scope")?),
            _ => {
                return Err(vec![AppletError::new(
                    APPLET,
                    "expected 'dev IFACE', 'to PREFIX', 'label PATTERN', or 'scope SCOPE'",
                )])
            }
        }
    }
    Ok(filter)
}

fn parse_optional_dev(args: &[String]) -> Result<Option<String>, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let Some(first) = cursor.next_token(APPLET)? else {
        return Ok(None);
    };
    if first == "dev" {
        let dev = next_string(&mut cursor, "dev")?;
        if cursor.next_token(APPLET)?.is_some() {
            return Err(vec![AppletError::new(APPLET, "extra operand")]);
        }
        return Ok(Some(dev));
    }
    if cursor.next_token(APPLET)?.is_some() {
        return Err(vec![AppletError::new(APPLET, "extra operand")]);
    }
    Ok(Some(first.to_string()))
}

fn parse_route_dev_filter(args: &[String]) -> Result<Option<String>, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let Some(first) = cursor.next_token(APPLET)? else {
        return Ok(None);
    };
    if first != "dev" {
        return Err(vec![AppletError::new(APPLET, "expected 'dev IFACE'")]);
    }
    let dev = next_string(&mut cursor, "dev")?;
    if cursor.next_token(APPLET)?.is_some() {
        return Err(vec![AppletError::new(APPLET, "expected 'dev IFACE'")]);
    }
    Ok(Some(dev))
}

fn parse_neigh_filter(
    args: &[String],
    family_hint: Option<AddressFamily>,
) -> Result<NeighFilter, Vec<AppletError>> {
    let mut filter = NeighFilter::default();
    let mut cursor = ArgCursor::new(args);
    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "dev" => filter.dev = Some(next_string(&mut cursor, "dev")?),
            "to" => {
                filter.prefix = Some(parse_ip_prefix(cursor.next_value(APPLET, "to")?, family_hint)?)
            }
            "nud" => filter.nud = parse_neigh_state(cursor.next_value(APPLET, "nud")?)?,
            _ => {
                return Err(vec![AppletError::new(
                    APPLET,
                    "expected 'dev IFACE', 'to PREFIX', or 'nud STATE'",
                )])
            }
        }
    }
    Ok(filter)
}

struct AddrChange {
    prefix: ParsedPrefix,
    dev: String,
    peer: Option<String>,
    broadcast: Option<Option<String>>,
    label: Option<String>,
    scope: Option<String>,
}

fn parse_addr_change(
    args: &[String],
    family_hint: Option<AddressFamily>,
) -> Result<AddrChange, Vec<AppletError>> {
    let mut prefix = None;
    let mut dev = None;
    let mut peer = None;
    let mut broadcast = None;
    let mut label = None;
    let mut scope = None;
    let mut cursor = ArgCursor::new(args);
    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "dev" => dev = Some(next_string(&mut cursor, "dev")?),
            "peer" => {
                let peer_prefix = parse_ip_prefix(cursor.next_value(APPLET, "peer")?, family_hint)?;
                peer = Some(match peer_prefix {
                    ParsedPrefix::Inet4(address, prefix_len) => format!("{address}/{prefix_len}"),
                    ParsedPrefix::Inet6(address, prefix_len) => format!("{address}/{prefix_len}"),
                });
            }
            "broadcast" => {
                let value = cursor.next_value(APPLET, "broadcast")?;
                broadcast = Some(match value {
                    "+" => None,
                    "-" => return Err(vec![AppletError::new(APPLET, "broadcast '-' is unsupported")]),
                    value => Some(parse_ip_addr(value)?.to_string()),
                });
            }
            "label" => label = Some(next_string(&mut cursor, "label")?),
            "scope" => scope = Some(next_string(&mut cursor, "scope")?),
            value => {
                if prefix.is_some() {
                    return Err(vec![AppletError::new(APPLET, "extra operand")]);
                }
                prefix = Some(parse_ip_prefix(value, family_hint)?);
            }
        }
    }
    Ok(AddrChange {
        prefix: prefix.ok_or_else(|| vec![AppletError::new(APPLET, "missing address")])?,
        dev: dev.ok_or_else(|| vec![AppletError::new(APPLET, "missing device")])?,
        peer,
        broadcast,
        label,
        scope,
    })
}

fn parse_rule_change(
    family_hint: Option<AddressFamily>,
    args: &[String],
) -> Result<RuleInfo, Vec<AppletError>> {
    let mut family = family_hint;
    let mut from = None;
    let mut from_prefix_len = 0;
    let mut to = None;
    let mut to_prefix_len = 0;
    let mut iif = None;
    let mut oif = None;
    let mut fwmark = None;
    let mut priority = None;
    let mut table = None;
    let mut cursor = ArgCursor::new(args);
    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "from" => {
                let value = cursor.next_value(APPLET, "from")?;
                if value != "all" {
                    match parse_ip_prefix(value, family_hint)? {
                        ParsedPrefix::Inet4(address, prefix) => {
                            family = Some(AddressFamily::Inet4);
                            from = Some(address.to_string());
                            from_prefix_len = prefix;
                        }
                        ParsedPrefix::Inet6(address, prefix) => {
                            family = Some(AddressFamily::Inet6);
                            from = Some(address);
                            from_prefix_len = prefix;
                        }
                    }
                }
            }
            "to" => {
                let value = cursor.next_value(APPLET, "to")?;
                if value != "all" {
                    match parse_ip_prefix(value, family_hint)? {
                        ParsedPrefix::Inet4(address, prefix) => {
                            family = Some(AddressFamily::Inet4);
                            to = Some(address.to_string());
                            to_prefix_len = prefix;
                        }
                        ParsedPrefix::Inet6(address, prefix) => {
                            family = Some(AddressFamily::Inet6);
                            to = Some(address);
                            to_prefix_len = prefix;
                        }
                    }
                }
            }
            "iif" => iif = Some(next_string(&mut cursor, "iif")?),
            "oif" => oif = Some(next_string(&mut cursor, "oif")?),
            "fwmark" => fwmark = Some(parse_u32_auto_base("fwmark", cursor.next_value(APPLET, "fwmark")?)?),
            "priority" | "pref" => {
                priority = Some(parse_u32_auto_base("priority", cursor.next_value(APPLET, "priority")?)?)
            }
            "lookup" | "table" => table = Some(parse_route_table(cursor.next_value(APPLET, "lookup")?)?),
            _ => return Err(vec![AppletError::new(APPLET, "unsupported rule selector/action")]),
        }
    }
    let family = family.unwrap_or(AddressFamily::Inet4);
    Ok(RuleInfo {
        family,
        from,
        from_prefix_len,
        to,
        to_prefix_len,
        iif,
        oif,
        fwmark,
        priority,
        table: table.unwrap_or(libc::RT_TABLE_MAIN as u32),
    })
}

fn parse_link_change(args: &[String]) -> Result<(String, LinkChange), Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let Some(first) = cursor.next_token(APPLET)? else {
        return Err(vec![AppletError::new(APPLET, "missing operand")]);
    };
    let name = if first == "dev" {
        next_string(&mut cursor, "dev")?
    } else {
        first.to_string()
    };
    let mut change = LinkChange::default();
    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "up" => {
                change.up = Some(true);
            }
            "down" => {
                change.up = Some(false);
            }
            "arp" => change.arp = Some(parse_on_off(APPLET, "arp", cursor.next_value(APPLET, "arp")?)?),
            "multicast" => {
                change.multicast = Some(parse_on_off(APPLET, "multicast", cursor.next_value(APPLET, "multicast")?)?)
            }
            "allmulticast" | "allmulti" => {
                change.allmulti =
                    Some(parse_on_off(APPLET, "allmulticast", cursor.next_value(APPLET, "allmulticast")?)?)
            }
            "promisc" => change.promisc = Some(parse_on_off(APPLET, "promisc", cursor.next_value(APPLET, "promisc")?)?),
            "mtu" => {
                let value = cursor.next_value(APPLET, "mtu")?;
                change.mtu = Some(value.parse().map_err(|_| {
                    vec![AppletError::new(APPLET, format!("invalid mtu '{value}'"))]
                })?);
            }
            "qlen" | "txqueuelen" => {
                let value = cursor.next_value(APPLET, "qlen")?;
                change.tx_queue_len = Some(value.parse().map_err(|_| {
                    vec![AppletError::new(APPLET, format!("invalid qlen '{value}'"))]
                })?);
            }
            "address" => {
                let value = cursor.next_value(APPLET, "address")?;
                parse_mac(value).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
                change.address = Some(value.to_string());
            }
            "name" => change.new_name = Some(next_string(&mut cursor, "name")?),
            _ => return Err(vec![AppletError::new(APPLET, format!("unsupported link option '{arg}'"))]),
        }
    }
    Ok((name, change))
}

fn parse_route_change(
    family_hint: Option<AddressFamily>,
    args: &[String],
) -> Result<RouteInfo, Vec<AppletError>> {
    let mut destination = None;
    let mut prefix_len = None;
    let mut gateway = None;
    let mut dev = None;
    let mut source = None;
    let mut metric = None;
    let mut table = None;
    let mut protocol = None;
    let mut family = family_hint;
    let mut cursor = ArgCursor::new(args);
    while let Some(arg) = cursor.next_token(APPLET)? {
        match arg {
            "default" => {
                destination = Some(match family_hint.unwrap_or(AddressFamily::Inet4) {
                    AddressFamily::Inet4 => String::from("0.0.0.0"),
                    AddressFamily::Inet6 => String::from("::"),
                });
                prefix_len = Some(0);
            }
            "via" => {
                let value = cursor.next_value(APPLET, "via")?;
                let parsed = parse_ip_addr(value)?;
                family = Some(match parsed {
                    IpAddr::V4(_) => AddressFamily::Inet4,
                    IpAddr::V6(_) => AddressFamily::Inet6,
                });
                gateway = Some(parsed.to_string());
            }
            "src" => source = Some(next_string(&mut cursor, "src")?),
            "metric" => {
                let value = cursor.next_value(APPLET, "metric")?;
                metric = Some(value.parse().map_err(|_| {
                    vec![AppletError::new(APPLET, format!("invalid metric '{value}'"))]
                })?);
            }
            "table" => table = Some(parse_route_table(cursor.next_value(APPLET, "table")?)?),
            "proto" => protocol = Some(parse_route_protocol(cursor.next_value(APPLET, "proto")?)?),
            "dev" => dev = Some(next_string(&mut cursor, "dev")?),
            value => {
                match parse_ip_prefix(value, family_hint)? {
                    ParsedPrefix::Inet4(address, prefix) => {
                        family = Some(AddressFamily::Inet4);
                        destination = Some(address.to_string());
                        prefix_len = Some(prefix);
                    }
                    ParsedPrefix::Inet6(address, prefix) => {
                        family = Some(AddressFamily::Inet6);
                        destination = Some(address);
                        prefix_len = Some(prefix);
                    }
                }
            }
        }
    }

    let family = family.unwrap_or(AddressFamily::Inet4);
    let source = source
        .map(|value| match (family, parse_ip_addr(&value)?) {
            (AddressFamily::Inet4, IpAddr::V4(address)) => Ok(address.to_string()),
            (AddressFamily::Inet6, IpAddr::V6(address)) => Ok(address.to_string()),
            (AddressFamily::Inet4, _) => {
                Err(vec![AppletError::new(APPLET, "IPv4 route requires IPv4 src")])
            }
            (AddressFamily::Inet6, _) => {
                Err(vec![AppletError::new(APPLET, "IPv6 route requires IPv6 src")])
            }
        })
        .transpose()?;

    Ok(RouteInfo {
        family,
        destination: destination.ok_or_else(|| vec![AppletError::new(APPLET, "missing route destination")])?,
        prefix_len: prefix_len.unwrap_or(32),
        gateway,
        dev: dev.ok_or_else(|| vec![AppletError::new(APPLET, "missing route device")])?,
        source,
        metric,
        table,
        protocol,
    })
}

fn has_visible_addresses(
    interface: &crate::common::net::InterfaceInfo,
    family: Option<AddressFamily>,
    filter: &AddrFilter,
) -> bool {
    interface
        .addresses
        .iter()
        .any(|address| {
            family.is_none_or(|expected| expected == address.family)
                && matches_addr_filter(address, &filter.prefix)
                && matches_label_scope(address, &filter.label, &filter.scope)
        })
}

fn print_addr_interface(
    interface: &crate::common::net::InterfaceInfo,
    family: Option<AddressFamily>,
    filter: &AddrFilter,
    oneline: bool,
) {
    for address in &interface.addresses {
        if family.is_some_and(|expected| expected != address.family)
            || !matches_addr_filter(address, &filter.prefix)
            || !matches_label_scope(address, &filter.label, &filter.scope)
        {
            continue;
        }
        if oneline {
            print!("{}: {}    ", interface.index, interface.name);
        } else {
            print_link_interface(interface, false);
        }
        match address.family {
            AddressFamily::Inet4 => {
                if oneline {
                    print!("inet {}/{}", address.address, address.prefix_len);
                } else {
                    print!("    inet {}/{}", address.address, address.prefix_len);
                }
                if let Some(peer) = &address.peer {
                    print!(" peer {peer}");
                }
                if let Some(broadcast) = &address.broadcast {
                    print!(" brd {broadcast}");
                }
                print!(
                    " scope {} {}",
                    address.scope.as_deref().unwrap_or("global"),
                    interface.name
                );
                if let Some(label) = &address.label {
                    print!(" label {label}");
                }
                if oneline {
                    print!(r"\       valid_lft forever preferred_lft forever");
                }
                println!();
            }
            AddressFamily::Inet6 => {
                if oneline {
                    print!("inet6 {}/{}", address.address, address.prefix_len);
                } else {
                    print!("    inet6 {}/{}", address.address, address.prefix_len);
                }
                if let Some(peer) = &address.peer {
                    print!(" peer {peer}");
                }
                print!(" scope {}", address.scope.as_deref().unwrap_or("global"));
                if let Some(label) = &address.label {
                    print!(" label {label}");
                }
                if oneline {
                    print!(r"\       valid_lft forever preferred_lft forever");
                }
                println!();
            }
        }
    }
}

fn matches_addr_filter(
    address: &crate::common::net::InterfaceAddress,
    prefix: &Option<ParsedPrefix>,
) -> bool {
    let Some(prefix) = prefix else {
        return true;
    };
    match (address.family, prefix) {
        (AddressFamily::Inet4, ParsedPrefix::Inet4(expected, prefix_len)) => address
            .address
            .parse::<Ipv4Addr>()
            .ok()
            .is_some_and(|candidate| prefix_matches_ipv4(candidate, *expected, *prefix_len)),
        (AddressFamily::Inet6, ParsedPrefix::Inet6(expected, prefix_len)) => address
            .address
            .parse::<Ipv6Addr>()
            .ok()
            .zip(expected.parse::<Ipv6Addr>().ok())
            .is_some_and(|(candidate, expected)| {
                prefix_matches_ipv6(candidate, expected, *prefix_len)
            }),
        _ => false,
    }
}

fn matches_label_scope(
    address: &crate::common::net::InterfaceAddress,
    label: &Option<String>,
    scope: &Option<String>,
) -> bool {
    label
        .as_deref()
        .is_none_or(|expected| address.label.as_deref().unwrap_or("") == expected)
        && scope
            .as_deref()
            .is_none_or(|expected| address.scope.as_deref().unwrap_or("global") == expected)
}

fn matches_neigh_filter(
    neighbor: &NeighborInfo,
    family: Option<AddressFamily>,
    filter: &NeighFilter,
) -> bool {
    family.is_none_or(|expected| expected == neighbor.family)
        && filter
            .dev
            .as_deref()
            .is_none_or(|expected| expected == neighbor.dev)
        && filter
            .nud
            .is_none_or(|expected| expected == 0 || neighbor.state & expected != 0)
        && match &filter.prefix {
            None => true,
            Some(ParsedPrefix::Inet4(expected, prefix_len)) if neighbor.family == AddressFamily::Inet4 => {
                neighbor
                    .address
                    .parse::<Ipv4Addr>()
                    .ok()
                    .is_some_and(|candidate| prefix_matches_ipv4(candidate, *expected, *prefix_len))
            }
            Some(ParsedPrefix::Inet6(expected, prefix_len)) if neighbor.family == AddressFamily::Inet6 => {
                neighbor
                    .address
                    .parse::<Ipv6Addr>()
                    .ok()
                    .zip(expected.parse::<Ipv6Addr>().ok())
                    .is_some_and(|(candidate, expected)| {
                        prefix_matches_ipv6(candidate, expected, *prefix_len)
                    })
            }
            Some(_) => false,
        }
}

fn print_link_interface(interface: &crate::common::net::InterfaceInfo, oneline: bool) {
    let kind = if interface.flags & libc::IFF_LOOPBACK as u32 != 0 {
        "loopback"
    } else {
        "ether"
    };
    let mac = interface
        .mac
        .as_deref()
        .and_then(|value| parse_mac(value).ok())
        .map(|bytes| format_mac(&bytes))
        .unwrap_or_else(|| String::from("00:00:00:00:00:00"));
    if oneline {
        println!(
            "{}: {}: <{}> mtu {} qlen {}\\    link/{} {}",
            interface.index,
            interface.name,
            interface_flag_names(interface.flags).join(","),
            interface.mtu,
            interface.tx_queue_len,
            kind,
            mac
        );
    } else {
        println!(
            "{}: {}: <{}> mtu {} qlen {}",
            interface.index,
            interface.name,
            interface_flag_names(interface.flags).join(","),
            interface.mtu,
            interface.tx_queue_len,
        );
        println!("    link/{kind} {mac}");
    }
}

fn print_neigh(neighbor: &NeighborInfo) {
    let mut parts = vec![neighbor.address.clone(), format!("dev {}", neighbor.dev)];
    if let Some(lladdr) = &neighbor.lladdr {
        parts.push(format!("lladdr {lladdr}"));
    }
    parts.push(format_neigh_state(neighbor.state));
    println!("{}", parts.join(" "));
}

fn print_rule(rule: &RuleInfo) {
    let priority = rule.priority.unwrap_or(0);
    let mut parts = vec![format!("{priority}:\tfrom {}", format_rule_prefix(&rule.from, rule.from_prefix_len))];
    if rule.to.is_some() || rule.to_prefix_len != 0 {
        parts.push(format!("to {}", format_rule_prefix(&rule.to, rule.to_prefix_len)));
    }
    if let Some(iif) = &rule.iif {
        parts.push(format!("iif {iif}"));
    }
    if let Some(oif) = &rule.oif {
        parts.push(format!("oif {oif}"));
    }
    if let Some(fwmark) = rule.fwmark {
        parts.push(format!("fwmark 0x{fwmark:x}"));
    }
    parts.push(format!("lookup {}", format_route_table(rule.table)));
    println!("{} ", parts.join(" "));
}

fn parse_on_off(applet: &'static str, option: &str, value: &str) -> Result<bool, Vec<AppletError>> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(vec![AppletError::new(
            applet,
            format!("expected '{option} on|off'"),
        )]),
    }
}

fn print_route(route: &RouteInfo) {
    let mut parts = Vec::new();
    if route.prefix_len == 0
        && matches!(
            route.destination.as_str(),
            "0.0.0.0" | "::" | "::0" | "0:0:0:0:0:0:0:0"
        )
    {
        parts.push(String::from("default"));
    } else {
        parts.push(format!("{}/{}", route.destination, route.prefix_len));
    }
    if let Some(gateway) = &route.gateway {
        parts.push(format!("via {gateway}"));
    }
    parts.push(format!("dev {}", route.dev));
    if let Some(protocol) = route.protocol.filter(|protocol| *protocol != libc::RTPROT_BOOT) {
        parts.push(format!("proto {}", format_route_protocol(protocol)));
    }
    if let Some(source) = &route.source {
        parts.push(format!("src {source}"));
    }
    if let Some(metric) = route.metric {
        parts.push(format!("metric {metric}"));
    }
    if let Some(table) = route.table.filter(|table| *table != libc::RT_TABLE_MAIN as u32) {
        parts.push(format!("table {}", format_route_table(table)));
    }
    println!("{}", parts.join(" "));
}

fn parse_route_table(value: &str) -> Result<u32, Vec<AppletError>> {
    match value {
        "default" => Ok(libc::RT_TABLE_DEFAULT as u32),
        "main" => Ok(libc::RT_TABLE_MAIN as u32),
        "local" => Ok(libc::RT_TABLE_LOCAL as u32),
        _ => value.parse::<u32>().map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid table '{value}'"),
            )]
        }),
    }
}

fn format_route_table(value: u32) -> String {
    match value {
        value if value == libc::RT_TABLE_DEFAULT as u32 => String::from("default"),
        value if value == libc::RT_TABLE_MAIN as u32 => String::from("main"),
        value if value == libc::RT_TABLE_LOCAL as u32 => String::from("local"),
        _ => value.to_string(),
    }
}

fn parse_route_protocol(value: &str) -> Result<u8, Vec<AppletError>> {
    match value {
        "redirect" => Ok(libc::RTPROT_REDIRECT),
        "kernel" => Ok(libc::RTPROT_KERNEL),
        "boot" => Ok(libc::RTPROT_BOOT),
        "static" => Ok(libc::RTPROT_STATIC),
        "ra" => Ok(RTPROT_RA),
        _ => value.parse::<u8>().map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid proto '{value}'"),
            )]
        }),
    }
}

fn parse_neigh_state(value: &str) -> Result<Option<u16>, Vec<AppletError>> {
    let state = match value {
        "all" => return Ok(Some(0)),
        "incomplete" => NUD_INCOMPLETE,
        "reachable" => NUD_REACHABLE,
        "stale" => NUD_STALE,
        "delay" => NUD_DELAY,
        "probe" => NUD_PROBE,
        "failed" => NUD_FAILED,
        "noarp" => NUD_NOARP,
        "permanent" => NUD_PERMANENT,
        _ => {
            return Err(vec![AppletError::new(
                APPLET,
                format!("invalid nud state '{value}'"),
            )])
        }
    };
    Ok(Some(state))
}

fn format_neigh_state(state: u16) -> String {
    let mut names = Vec::new();
    if state & NUD_INCOMPLETE != 0 {
        names.push("INCOMPLETE");
    }
    if state & NUD_REACHABLE != 0 {
        names.push("REACHABLE");
    }
    if state & NUD_STALE != 0 {
        names.push("STALE");
    }
    if state & NUD_DELAY != 0 {
        names.push("DELAY");
    }
    if state & NUD_PROBE != 0 {
        names.push("PROBE");
    }
    if state & NUD_FAILED != 0 {
        names.push("FAILED");
    }
    if state & NUD_NOARP != 0 {
        names.push("NOARP");
    }
    if state & NUD_PERMANENT != 0 {
        names.push("PERMANENT");
    }
    if names.is_empty() {
        state.to_string()
    } else {
        names.join(",")
    }
}

fn format_route_protocol(value: u8) -> String {
    match value {
        value if value == libc::RTPROT_REDIRECT => String::from("redirect"),
        value if value == libc::RTPROT_KERNEL => String::from("kernel"),
        value if value == libc::RTPROT_BOOT => String::from("boot"),
        value if value == libc::RTPROT_STATIC => String::from("static"),
        value if value == RTPROT_RA => String::from("ra"),
        _ => value.to_string(),
    }
}

fn parse_u32_auto_base(kind: &str, value: &str) -> Result<u32, Vec<AppletError>> {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(
            || value.parse::<u32>(),
            |hex| u32::from_str_radix(hex, 16),
        )
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid {kind} '{value}'"))])
}

fn format_rule_prefix(address: &Option<String>, prefix_len: u8) -> String {
    match address {
        Some(address) => format!("{address}/{prefix_len}"),
        None => String::from("all"),
    }
}

fn parse_ip_addr(value: &str) -> Result<IpAddr, Vec<AppletError>> {
    value.parse::<IpAddr>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid IP address '{value}'"),
        )]
    })
}

enum ParsedPrefix {
    Inet4(Ipv4Addr, u8),
    Inet6(String, u8),
}

fn parse_ip_prefix(
    spec: &str,
    family_hint: Option<AddressFamily>,
) -> Result<ParsedPrefix, Vec<AppletError>> {
    if spec.contains(':') || family_hint == Some(AddressFamily::Inet6) {
        let (address, prefix) = spec.split_once('/').unwrap_or((spec, "128"));
        let address = address.parse::<Ipv6Addr>().map_err(|_| {
            vec![AppletError::new(
                APPLET,
                format!("invalid IPv6 address '{address}'"),
            )]
        })?;
        let prefix = prefix.parse::<u8>().ok().filter(|prefix| *prefix <= 128).ok_or_else(|| {
            vec![AppletError::new(
                APPLET,
                format!("invalid prefix '{prefix}'"),
            )]
        })?;
        Ok(ParsedPrefix::Inet6(address.to_string(), prefix))
    } else {
        let (address, prefix) =
            parse_prefix(spec).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
        Ok(ParsedPrefix::Inet4(address, prefix))
    }
}

fn prefix_matches_ipv4(address: Ipv4Addr, expected: Ipv4Addr, prefix_len: u8) -> bool {
    let mask = if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len)
    };
    (u32::from(address) & mask) == (u32::from(expected) & mask)
}

fn prefix_matches_ipv6(address: Ipv6Addr, expected: Ipv6Addr, prefix_len: u8) -> bool {
    let address = u128::from_be_bytes(address.octets());
    let expected = u128::from_be_bytes(expected.octets());
    let mask = if prefix_len == 0 {
        0
    } else {
        u128::MAX << (128 - prefix_len)
    };
    (address & mask) == (expected & mask)
}

fn io_error(
    action: &'static str,
    path: Option<String>,
) -> impl FnOnce(std::io::Error) -> Vec<AppletError> {
    move |err| vec![AppletError::from_io(APPLET, action, path.as_deref(), err)]
}

#[cfg(test)]
mod tests {
    use super::{
        AddressFamily, NUD_STALE, parse_addr_change, parse_global_options, parse_ip_prefix,
        parse_link_change, parse_neigh_filter, parse_route_change, parse_rule_change,
    };
    use std::net::Ipv4Addr;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn args_os(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_addr_change() {
        let change = parse_addr_change(&args(&["10.0.0.2/24", "dev", "eth0"]), None).unwrap();
        match change.prefix {
            super::ParsedPrefix::Inet4(address, prefix) => {
                assert_eq!(address.to_string(), "10.0.0.2");
                assert_eq!(prefix, 24);
            }
            super::ParsedPrefix::Inet6(_, _) => panic!("expected inet4"),
        }
        assert_eq!(change.dev, "eth0");
    }

    #[test]
    fn parses_addr_change_metadata() {
        let change = parse_addr_change(
            &args(&[
                "10.0.0.2/24",
                "peer",
                "10.0.0.1/24",
                "broadcast",
                "+",
                "label",
                "eth0:1",
                "scope",
                "link",
                "dev",
                "eth0",
            ]),
            None,
        )
        .unwrap();
        assert_eq!(change.peer.as_deref(), Some("10.0.0.1/24"));
        assert_eq!(change.broadcast, Some(None));
        assert_eq!(change.label.as_deref(), Some("eth0:1"));
        assert_eq!(change.scope.as_deref(), Some("link"));
    }

    #[test]
    fn parses_link_change() {
        let (name, change) = parse_link_change(&args(&[
            "dev",
            "eth0",
            "mtu",
            "1400",
            "up",
            "arp",
            "off",
            "qlen",
            "200",
        ]))
        .unwrap();
        assert_eq!(name, "eth0");
        assert_eq!(change.mtu, Some(1400));
        assert_eq!(change.up, Some(true));
        assert_eq!(change.arp, Some(false));
        assert_eq!(change.tx_queue_len, Some(200));
    }

    #[test]
    fn parses_route_change() {
        let route = parse_route_change(None, &args(&["default", "via", "10.0.0.1", "dev", "eth0"])).unwrap();
        assert_eq!(route.destination, "0.0.0.0");
        assert_eq!(route.prefix_len, 0);
        assert_eq!(route.gateway.as_deref(), Some("10.0.0.1"));
    }

    #[test]
    fn parses_global_family_flag() {
        let values = args_os(&["-6", "route", "show"]);
        let (options, rest) = parse_global_options(&values).unwrap();
        assert_eq!(options.family, Some(AddressFamily::Inet6));
        assert_eq!(rest, args(&["route", "show"]));
    }

    #[test]
    fn parses_ipv6_prefix() {
        match parse_ip_prefix("2001:db8::2/64", Some(AddressFamily::Inet6)).unwrap() {
            super::ParsedPrefix::Inet6(address, prefix) => {
                assert_eq!(address, "2001:db8::2");
                assert_eq!(prefix, 64);
            }
            super::ParsedPrefix::Inet4(_, _) => panic!("expected inet6"),
        }
    }

    #[test]
    fn parses_ipv6_route_change() {
        let route =
            parse_route_change(Some(AddressFamily::Inet6), &args(&["default", "via", "fe80::1", "dev", "eth0"]))
                .unwrap();
        assert_eq!(route.family, AddressFamily::Inet6);
        assert_eq!(route.destination, "::");
        assert_eq!(route.gateway.as_deref(), Some("fe80::1"));
    }

    #[test]
    fn parses_route_change_attributes() {
        let route = parse_route_change(
            None,
            &args(&[
                "10.0.1.0/24",
                "dev",
                "eth0",
                "src",
                "10.0.0.2",
                "metric",
                "10",
                "table",
                "100",
                "proto",
                "static",
            ]),
        )
        .unwrap();
        assert_eq!(route.source.as_deref(), Some("10.0.0.2"));
        assert_eq!(route.metric, Some(10));
        assert_eq!(route.table, Some(100));
        assert_eq!(route.protocol, Some(libc::RTPROT_STATIC));
    }

    #[test]
    fn parses_neigh_filter() {
        let filter = parse_neigh_filter(
            &args(&["to", "10.0.0.0/24", "dev", "eth0", "nud", "stale"]),
            None,
        )
        .unwrap();
        match filter.prefix.unwrap() {
            super::ParsedPrefix::Inet4(address, prefix) => {
                assert_eq!(address, Ipv4Addr::new(10, 0, 0, 0));
                assert_eq!(prefix, 24);
            }
            super::ParsedPrefix::Inet6(_, _) => panic!("expected inet4"),
        }
        assert_eq!(filter.dev.as_deref(), Some("eth0"));
        assert_eq!(filter.nud, Some(NUD_STALE));
    }

    #[test]
    fn parses_rule_change() {
        let rule = parse_rule_change(
            None,
            &args(&[
                "from",
                "10.0.0.0/24",
                "to",
                "10.1.0.0/24",
                "iif",
                "eth0",
                "oif",
                "eth1",
                "fwmark",
                "0x10",
                "priority",
                "100",
                "lookup",
                "200",
            ]),
        )
        .unwrap();
        assert_eq!(rule.from.as_deref(), Some("10.0.0.0"));
        assert_eq!(rule.from_prefix_len, 24);
        assert_eq!(rule.to.as_deref(), Some("10.1.0.0"));
        assert_eq!(rule.to_prefix_len, 24);
        assert_eq!(rule.iif.as_deref(), Some("eth0"));
        assert_eq!(rule.oif.as_deref(), Some("eth1"));
        assert_eq!(rule.fwmark, Some(0x10));
        assert_eq!(rule.priority, Some(100));
        assert_eq!(rule.table, 200);
    }

    #[test]
    fn parses_family_option() {
        let values = args_os(&["-f", "inet6", "route", "show"]);
        let (options, rest) = parse_global_options(&values).unwrap();
        assert_eq!(options.family, Some(AddressFamily::Inet6));
        assert_eq!(rest, args(&["route", "show"]));
    }

    #[test]
    fn parses_oneline_option() {
        let values = args_os(&["-o", "addr", "show"]);
        let (options, rest) = parse_global_options(&values).unwrap();
        assert!(options.oneline);
        assert_eq!(rest, args(&["addr", "show"]));
    }
}
