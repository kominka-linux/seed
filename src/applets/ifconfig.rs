#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::net::Ipv4Addr;

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::net::{parse_mac, parse_prefix};
#[cfg(target_os = "linux")]
use crate::common::net::{
    AddressFamily, Ipv4Change, LinkChange, apply_link_change, broadcast_for, format_mac,
    interface_flag_names, list_interfaces, prefix_to_netmask, set_ipv4,
};

const APPLET: &str = "ifconfig";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    all: bool,
    interface: Option<String>,
    address: Option<Ipv4Addr>,
    prefix_len: Option<u8>,
    netmask: Option<Ipv4Addr>,
    broadcast: Option<Option<Ipv4Addr>>,
    pointopoint: Option<Ipv4Addr>,
    mtu: Option<u32>,
    hwaddr: Option<String>,
    up: Option<bool>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;

    #[cfg(target_os = "linux")]
    {
        return run_linux(&options);
    }

    #[cfg(not(target_os = "linux"))]
    let _ = options;

    #[allow(unreachable_code)]
    Err(vec![AppletError::new(
        APPLET,
        "unsupported on this platform",
    )])
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options::default();
    let mut index = 0;

    if args.first().is_some_and(|arg| arg == "-a") {
        options.all = true;
        index += 1;
    }

    if let Some(interface) = args.get(index) {
        options.interface = Some(interface.clone());
        index += 1;
    }

    while index < args.len() {
        let arg = &args[index];
        index += 1;
        match arg.as_str() {
            "netmask" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "netmask")]);
                };
                index += 1;
                options.netmask = Some(parse_ipv4(value)?);
            }
            "broadcast" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "broadcast")]);
                };
                index += 1;
                options.broadcast = Some(if value == "+" {
                    None
                } else {
                    Some(parse_ipv4(value)?)
                });
            }
            "pointopoint" | "dstaddr" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "pointopoint")]);
                };
                index += 1;
                options.pointopoint = Some(parse_ipv4(value)?);
            }
            "mtu" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "mtu")]);
                };
                index += 1;
                options.mtu = Some(parse_u32("mtu", value)?);
            }
            "hw" => {
                let Some(kind) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "hw")]);
                };
                if kind != "ether" {
                    return Err(vec![AppletError::new(APPLET, "only 'hw ether' is supported")]);
                }
                let Some(value) = args.get(index + 1) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "hw")]);
                };
                parse_mac(value).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
                options.hwaddr = Some(value.clone());
                index += 2;
            }
            "up" => options.up = Some(true),
            "down" => options.up = Some(false),
            _ if arg.starts_with('-') => {
                return Err(vec![AppletError::invalid_option(
                    APPLET,
                    arg.chars().nth(1).unwrap_or('-'),
                )]);
            }
            _ => {
                let (address, prefix_len) =
                    parse_prefix(arg).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
                options.address = Some(address);
                options.prefix_len = Some(prefix_len);
            }
        }
    }

    Ok(options)
}

#[cfg(target_os = "linux")]
fn run_linux(options: &Options) -> Result<(), Vec<AppletError>> {
    if options.interface.is_none() {
        return print_interfaces(options.all);
    }

    let interface = options.interface.as_deref().unwrap_or("");
    if !has_changes(options) {
        return print_single_interface(interface);
    }

    if options.address.is_none() && options.up.is_none() && options.mtu.is_none() && options.hwaddr.is_none() {
        return Ok(());
    }

    if options.address.is_some() || options.netmask.is_some() || options.broadcast.is_some() || options.pointopoint.is_some() {
        let prefix_len = options
            .prefix_len
            .or_else(|| options.netmask.map(|mask| u32::from(mask).count_ones() as u8))
            .unwrap_or(32);
        let broadcast = match options.broadcast {
            Some(Some(value)) => Some(Some(value)),
            Some(None) => options.address.map(|address| Some(broadcast_for(address, prefix_len))),
            None => None,
        };
        set_ipv4(
            interface,
            &Ipv4Change {
                address: options.address,
                prefix_len: Some(prefix_len),
                broadcast,
                peer: options.pointopoint,
            },
        )
        .map_err(io_error(
            "updating IPv4 address",
            Some(interface.to_string()),
        ))?;
    }

    apply_link_change(
        interface,
        &LinkChange {
            up: options.up,
            mtu: options.mtu,
            address: options.hwaddr.clone(),
            new_name: None,
        },
    )
    .map_err(io_error("updating link", Some(interface.to_string())))?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn has_changes(options: &Options) -> bool {
    options.address.is_some()
        || options.netmask.is_some()
        || options.broadcast.is_some()
        || options.pointopoint.is_some()
        || options.mtu.is_some()
        || options.hwaddr.is_some()
        || options.up.is_some()
}

#[cfg(target_os = "linux")]
fn print_interfaces(all: bool) -> Result<(), Vec<AppletError>> {
    let interfaces = list_interfaces().map_err(io_error("listing interfaces", None))?;
    for interface in interfaces {
        if !all && interface.flags & libc::IFF_UP as u32 == 0 {
            continue;
        }
        print_interface(&interface);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn print_single_interface(name: &str) -> Result<(), Vec<AppletError>> {
    let interface = list_interfaces()
        .map_err(io_error("listing interfaces", None))?
        .into_iter()
        .find(|interface| interface.name == name)
        .ok_or_else(|| vec![AppletError::new(APPLET, format!("interface '{name}' not found"))])?;
    print_interface(&interface);
    Ok(())
}

#[cfg(target_os = "linux")]
fn print_interface(interface: &crate::common::net::InterfaceInfo) {
    let mac = interface
        .mac
        .as_deref()
        .and_then(|value| parse_mac(value).ok())
        .map(|bytes| format_mac(&bytes))
        .unwrap_or_else(|| String::from("00:00:00:00:00:00"));
    println!("{}  Link encap:Ethernet  HWaddr {}", interface.name, mac);
    for address in &interface.addresses {
        match address.family {
            AddressFamily::Inet4 => {
                let mask = prefix_to_netmask(address.prefix_len);
                print!("          inet addr:{}  Mask:{}", address.address, mask);
                if let Some(broadcast) = &address.broadcast {
                    print!("  Bcast:{broadcast}");
                }
                if let Some(peer) = &address.peer {
                    print!("  P-t-P:{peer}");
                }
                println!();
            }
            AddressFamily::Inet6 => {
                println!("          inet6 addr: {}/{}", address.address, address.prefix_len);
            }
        }
    }
    println!(
        "          {}  MTU:{}",
        interface_flag_names(interface.flags).join(" "),
        interface.mtu
    );
    println!(
        "          RX packets:{} bytes:{}  TX packets:{} bytes:{}",
        interface.stats.rx_packets,
        interface.stats.rx_bytes,
        interface.stats.tx_packets,
        interface.stats.tx_bytes
    );
}

fn parse_ipv4(value: &str) -> Result<Ipv4Addr, Vec<AppletError>> {
    value
        .parse::<Ipv4Addr>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid IPv4 address '{value}'"))])
}

fn parse_u32(kind: &str, value: &str) -> Result<u32, Vec<AppletError>> {
    value
        .parse::<u32>()
        .map_err(|_| vec![AppletError::new(APPLET, format!("invalid {kind} '{value}'"))])
}

#[cfg(target_os = "linux")]
fn io_error(
    action: &'static str,
    path: Option<String>,
) -> impl FnOnce(std::io::Error) -> Vec<AppletError> {
    move |err| vec![AppletError::from_io(APPLET, action, path.as_deref(), err)]
}

#[cfg(test)]
mod tests {
    use super::{Options, parse_args};
    use std::net::Ipv4Addr;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_interface_change() {
        assert_eq!(
            parse_args(&args(&["eth0", "192.168.1.2/24", "mtu", "1400", "up"])).unwrap(),
            Options {
                all: false,
                interface: Some(String::from("eth0")),
                address: Some(Ipv4Addr::new(192, 168, 1, 2)),
                prefix_len: Some(24),
                netmask: None,
                broadcast: None,
                pointopoint: None,
                mtu: Some(1400),
                hwaddr: None,
                up: Some(true),
            }
        );
    }
}
