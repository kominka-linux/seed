
use std::net::Ipv4Addr;

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::net::{
    AddressFamily, Ipv4Change, LinkChange, RouteInfo, add_route, apply_link_change, broadcast_for,
    clear_ipv4, del_route, format_mac, interface_flag_names, list_interfaces, list_routes, parse_mac,
    parse_prefix, set_ipv4,
};

const APPLET: &str = "ip";

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    run_linux(args)
}

fn run_linux(args: &[String]) -> Result<(), Vec<AppletError>> {
    let Some(command) = args.first().map(String::as_str) else {
        return Err(vec![AppletError::new(APPLET, "missing command")]);
    };
    match command {
        "addr" | "address" => run_addr(&args[1..]),
        "link" => run_link(&args[1..]),
        "route" => run_route(&args[1..]),
        _ => Err(vec![AppletError::new(
            APPLET,
            format!("unsupported command '{command}'"),
        )]),
    }
}

fn run_addr(args: &[String]) -> Result<(), Vec<AppletError>> {
    match args.first().map(String::as_str).unwrap_or("show") {
        "show" | "list" => {
            let dev = parse_dev_filter(&args[1..])?;
            let interfaces = list_interfaces().map_err(io_error("listing interfaces", None))?;
            for interface in interfaces {
                if dev.as_deref().is_some_and(|dev| dev != interface.name) {
                    continue;
                }
                print_addr_interface(&interface);
            }
            Ok(())
        }
        "add" | "del" => {
            let delete = args[0] == "del";
            let (spec, dev) = parse_addr_change(&args[1..])?;
            if delete {
                clear_ipv4(&dev)
                    .map_err(io_error("clearing IPv4 address", Some("interface".to_string())))?;
            } else {
                let (address, prefix_len) =
                    parse_prefix(&spec).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
                set_ipv4(
                    &dev,
                    &Ipv4Change {
                        address: Some(address),
                        prefix_len: Some(prefix_len),
                        broadcast: Some(Some(broadcast_for(address, prefix_len))),
                        peer: None,
                    },
                )
                .map_err(io_error("setting IPv4 address", Some("interface".to_string())))?;
            }
            Ok(())
        }
        other => Err(vec![AppletError::new(
            APPLET,
            format!("unsupported 'ip addr' command '{other}'"),
        )]),
    }
}

fn run_link(args: &[String]) -> Result<(), Vec<AppletError>> {
    match args.first().map(String::as_str).unwrap_or("show") {
        "show" | "list" => {
            let dev = parse_optional_dev(&args[1..])?;
            let interfaces = list_interfaces().map_err(io_error("listing interfaces", None))?;
            for interface in interfaces {
                if dev.as_deref().is_some_and(|dev| dev != interface.name) {
                    continue;
                }
                print_link_interface(&interface);
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

fn run_route(args: &[String]) -> Result<(), Vec<AppletError>> {
    match args.first().map(String::as_str).unwrap_or("show") {
        "show" | "list" => {
            for route in list_routes().map_err(io_error("listing routes", None))? {
                print_route(&route);
            }
            Ok(())
        }
        "add" | "del" => {
            let delete = args[0] == "del";
            let route = parse_route_change(&args[1..])?;
            if delete {
                del_route(&route).map_err(io_error("deleting route", None))?;
            } else {
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

fn parse_dev_filter(args: &[String]) -> Result<Option<String>, Vec<AppletError>> {
    match args {
        [] => Ok(None),
        [label, dev] if label == "dev" => Ok(Some(dev.clone())),
        _ => Err(vec![AppletError::new(APPLET, "expected 'dev IFACE'")]),
    }
}

fn parse_optional_dev(args: &[String]) -> Result<Option<String>, Vec<AppletError>> {
    match args {
        [] => Ok(None),
        [name] => Ok(Some(name.clone())),
        [label, dev] if label == "dev" => Ok(Some(dev.clone())),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn parse_addr_change(args: &[String]) -> Result<(String, String), Vec<AppletError>> {
    match args {
        [spec, label, dev] if label == "dev" => Ok((spec.clone(), dev.clone())),
        _ => Err(vec![AppletError::new(APPLET, "expected 'ADDR/PREFIX dev IFACE'")]),
    }
}

fn parse_link_change(args: &[String]) -> Result<(String, LinkChange), Vec<AppletError>> {
    let (name, mut index) = match args {
        [label, dev, ..] if label == "dev" => (dev.clone(), 2),
        [dev, ..] => (dev.clone(), 1),
        [] => return Err(vec![AppletError::new(APPLET, "missing operand")]),
    };
    let mut change = LinkChange::default();
    while index < args.len() {
        match args[index].as_str() {
            "up" => {
                change.up = Some(true);
                index += 1;
            }
            "down" => {
                change.up = Some(false);
                index += 1;
            }
            "mtu" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "mtu")]);
                };
                change.mtu = Some(value.parse().map_err(|_| {
                    vec![AppletError::new(APPLET, format!("invalid mtu '{value}'"))]
                })?);
                index += 2;
            }
            "address" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "address")]);
                };
                parse_mac(value).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
                change.address = Some(value.clone());
                index += 2;
            }
            "name" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "name")]);
                };
                change.new_name = Some(value.clone());
                index += 2;
            }
            _ => return Err(vec![AppletError::new(APPLET, format!("unsupported link option '{}'", args[index]))]),
        }
    }
    Ok((name, change))
}

fn parse_route_change(args: &[String]) -> Result<RouteInfo, Vec<AppletError>> {
    let mut destination = None;
    let mut prefix_len = None;
    let mut gateway = None;
    let mut dev = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "default" => {
                destination = Some(String::from("0.0.0.0"));
                prefix_len = Some(0);
                index += 1;
            }
            "via" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "via")]);
                };
                gateway = Some(parse_ipv4(value)?.to_string());
                index += 2;
            }
            "dev" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "dev")]);
                };
                dev = Some(value.clone());
                index += 2;
            }
            value => {
                let (address, prefix) =
                    parse_prefix(value).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
                destination = Some(address.to_string());
                prefix_len = Some(prefix);
                index += 1;
            }
        }
    }

    Ok(RouteInfo {
        family: AddressFamily::Inet4,
        destination: destination.ok_or_else(|| vec![AppletError::new(APPLET, "missing route destination")])?,
        prefix_len: prefix_len.unwrap_or(32),
        gateway,
        dev: dev.ok_or_else(|| vec![AppletError::new(APPLET, "missing route device")])?,
    })
}

fn print_addr_interface(interface: &crate::common::net::InterfaceInfo) {
    print_link_interface(interface);
    for address in &interface.addresses {
        match address.family {
            AddressFamily::Inet4 => {
                println!("    inet {}/{}", address.address, address.prefix_len);
            }
            AddressFamily::Inet6 => {
                println!("    inet6 {}/{}", address.address, address.prefix_len);
            }
        }
    }
}

fn print_link_interface(interface: &crate::common::net::InterfaceInfo) {
    println!(
        "{}: {}: <{}> mtu {}",
        interface.index,
        interface.name,
        interface_flag_names(interface.flags).join(","),
        interface.mtu
    );
    println!(
        "    link/ether {}",
        interface
            .mac
            .as_deref()
            .and_then(|value| parse_mac(value).ok())
            .map(|bytes| format_mac(&bytes))
            .unwrap_or_else(|| String::from("00:00:00:00:00:00"))
    );
}

fn print_route(route: &RouteInfo) {
    if route.family != AddressFamily::Inet4 {
        return;
    }
    if route.prefix_len == 0 && route.destination == "0.0.0.0" {
        match &route.gateway {
            Some(gateway) => println!("default via {} dev {}", gateway, route.dev),
            None => println!("default dev {}", route.dev),
        }
    } else {
        match &route.gateway {
            Some(gateway) => println!("{}/{} via {} dev {}", route.destination, route.prefix_len, gateway, route.dev),
            None => println!("{}/{} dev {}", route.destination, route.prefix_len, route.dev),
        }
    }
}

fn parse_ipv4(value: &str) -> Result<Ipv4Addr, Vec<AppletError>> {
    value
        .parse::<Ipv4Addr>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid IPv4 address '{value}'"))])
}

fn io_error(
    action: &'static str,
    path: Option<String>,
) -> impl FnOnce(std::io::Error) -> Vec<AppletError> {
    move |err| vec![AppletError::from_io(APPLET, action, path.as_deref(), err)]
}

#[cfg(test)]
mod tests {
    use super::{parse_addr_change, parse_link_change, parse_route_change};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_addr_change() {
        assert_eq!(
            parse_addr_change(&args(&["10.0.0.2/24", "dev", "eth0"])).unwrap(),
            (String::from("10.0.0.2/24"), String::from("eth0"))
        );
    }

    #[test]
    fn parses_link_change() {
        let (name, change) = parse_link_change(&args(&["dev", "eth0", "mtu", "1400", "up"])).unwrap();
        assert_eq!(name, "eth0");
        assert_eq!(change.mtu, Some(1400));
        assert_eq!(change.up, Some(true));
    }

    #[test]
    fn parses_route_change() {
        let route = parse_route_change(&args(&["default", "via", "10.0.0.1", "dev", "eth0"])).unwrap();
        assert_eq!(route.destination, "0.0.0.0");
        assert_eq!(route.prefix_len, 0);
        assert_eq!(route.gateway.as_deref(), Some("10.0.0.1"));
    }
}
