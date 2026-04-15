
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::common::applet::finish;
use crate::common::error::AppletError;
use crate::common::net::{parse_mac, parse_prefix};
use crate::common::net::{
    AddressFamily, InterfaceAddress, Ipv4Change, LinkChange, add_address, apply_link_change,
    broadcast_for, format_mac, interface_flag_names, list_interfaces, prefix_to_netmask,
    remove_address, set_ipv4,
};

const APPLET: &str = "ifconfig";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Options {
    all: bool,
    interface: Option<String>,
    address: Option<Ipv4Addr>,
    prefix_len: Option<u8>,
    add_address: Option<(AddressFamily, String, u8)>,
    del_address: Option<(AddressFamily, String)>,
    netmask: Option<Ipv4Addr>,
    broadcast: Option<Option<Ipv4Addr>>,
    pointopoint: Option<Ipv4Addr>,
    mtu: Option<u32>,
    metric: Option<u32>,
    tx_queue_len: Option<u32>,
    hwaddr: Option<String>,
    up: Option<bool>,
    arp: Option<bool>,
    multicast: Option<bool>,
    allmulti: Option<bool>,
    promisc: Option<bool>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let options = parse_args(args)?;
    run_linux(&options)
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
                if let Some(value) = args.get(index) {
                    if !is_flag_like(value) {
                        index += 1;
                        options.broadcast = Some(if value == "+" {
                            None
                        } else {
                            Some(parse_ipv4(value)?)
                        });
                    } else {
                        options.broadcast = Some(None);
                    }
                } else {
                    options.broadcast = Some(None);
                }
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
            "metric" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "metric")]);
                };
                index += 1;
                options.metric = Some(parse_u32("metric", value)?);
            }
            "txqueuelen" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "txqueuelen")]);
                };
                index += 1;
                options.tx_queue_len = Some(parse_u32("txqueuelen", value)?);
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
            "arp" => options.arp = Some(true),
            "-arp" => options.arp = Some(false),
            "multicast" => options.multicast = Some(true),
            "-multicast" => options.multicast = Some(false),
            "allmulti" => options.allmulti = Some(true),
            "-allmulti" => options.allmulti = Some(false),
            "promisc" => options.promisc = Some(true),
            "-promisc" => options.promisc = Some(false),
            "add" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "add")]);
                };
                index += 1;
                let (family, address, prefix_len) = parse_any_prefix(value)?;
                options.add_address = Some((family, address, prefix_len));
            }
            "del" => {
                let Some(value) = args.get(index) else {
                    return Err(vec![AppletError::option_requires_arg(APPLET, "del")]);
                };
                index += 1;
                let parsed = parse_ip_addr(value)?;
                options.del_address = Some((
                    match parsed {
                        IpAddr::V4(_) => AddressFamily::Inet4,
                        IpAddr::V6(_) => AddressFamily::Inet6,
                    },
                    parsed.to_string(),
                ));
            }
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

fn run_linux(options: &Options) -> Result<(), Vec<AppletError>> {
    if options.interface.is_none() {
        return print_interfaces(options.all);
    }

    let interface = options.interface.as_deref().unwrap_or("");
    if !has_changes(options) {
        return print_single_interface(interface);
    }

    if let Some((family, address, prefix_len)) = &options.add_address {
        add_address(
            interface,
            &InterfaceAddress {
                family: *family,
                address: address.clone(),
                prefix_len: *prefix_len,
                peer: None,
                broadcast: None,
            },
        )
        .map_err(io_error("adding address", Some(interface.to_string())))?;
    }

    if let Some((family, address)) = &options.del_address {
        remove_address(interface, *family, address)
            .map_err(io_error("removing address", Some(interface.to_string())))?;
    }

    if options.address.is_none()
        && options.up.is_none()
        && options.mtu.is_none()
        && options.metric.is_none()
        && options.tx_queue_len.is_none()
        && options.hwaddr.is_none()
        && options.arp.is_none()
        && options.multicast.is_none()
        && options.allmulti.is_none()
        && options.promisc.is_none()
    {
        return Ok(());
    }

    if options.address.is_some()
        || options.netmask.is_some()
        || options.broadcast.is_some()
        || options.pointopoint.is_some()
    {
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
            metric: options.metric,
            tx_queue_len: options.tx_queue_len,
            address: options.hwaddr.clone(),
            new_name: None,
            arp: options.arp,
            multicast: options.multicast,
            allmulti: options.allmulti,
            promisc: options.promisc,
        },
    )
    .map_err(io_error("updating link", Some(interface.to_string())))?;

    Ok(())
}

fn has_changes(options: &Options) -> bool {
    options.address.is_some()
        || options.netmask.is_some()
        || options.broadcast.is_some()
        || options.pointopoint.is_some()
        || options.mtu.is_some()
        || options.metric.is_some()
        || options.tx_queue_len.is_some()
        || options.hwaddr.is_some()
        || options.up.is_some()
        || options.arp.is_some()
        || options.multicast.is_some()
        || options.allmulti.is_some()
        || options.promisc.is_some()
        || options.add_address.is_some()
        || options.del_address.is_some()
}

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

fn print_single_interface(name: &str) -> Result<(), Vec<AppletError>> {
    let interface = list_interfaces()
        .map_err(io_error("listing interfaces", None))?
        .into_iter()
        .find(|interface| interface.name == name)
        .ok_or_else(|| vec![AppletError::new(APPLET, format!("interface '{name}' not found"))])?;
    print_interface(&interface);
    Ok(())
}

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
        "          {}  MTU:{}  Metric:{}  TXqueuelen:{}",
        interface_flag_names(interface.flags).join(" "),
        interface.mtu,
        interface.metric,
        interface.tx_queue_len,
    );
    println!(
        "          RX packets:{} bytes:{}  TX packets:{} bytes:{}",
        interface.stats.rx_packets,
        interface.stats.rx_bytes,
        interface.stats.tx_packets,
        interface.stats.tx_bytes
    );
}

fn is_flag_like(value: &str) -> bool {
    value.starts_with('-')
        || matches!(
            value,
            "up"
                | "down"
                | "arp"
                | "-arp"
                | "multicast"
                | "-multicast"
                | "allmulti"
                | "-allmulti"
                | "promisc"
                | "-promisc"
                | "netmask"
                | "broadcast"
                | "pointopoint"
                | "dstaddr"
                | "metric"
                | "mtu"
                | "txqueuelen"
                | "hw"
                | "add"
                | "del"
        )
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

fn parse_ip_addr(value: &str) -> Result<IpAddr, Vec<AppletError>> {
    value.parse::<IpAddr>().map_err(|_| {
        vec![AppletError::new(
            APPLET,
            format!("invalid IP address '{value}'"),
        )]
    })
}

fn parse_any_prefix(value: &str) -> Result<(AddressFamily, String, u8), Vec<AppletError>> {
    if value.contains(':') {
        let (address, prefix) = value.split_once('/').unwrap_or((value, "128"));
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
        Ok((AddressFamily::Inet6, address.to_string(), prefix))
    } else {
        let (address, prefix) =
            parse_prefix(value).map_err(|err| vec![AppletError::new(APPLET, err.to_string())])?;
        Ok((AddressFamily::Inet4, address.to_string(), prefix))
    }
}

fn io_error(
    action: &'static str,
    path: Option<String>,
) -> impl FnOnce(std::io::Error) -> Vec<AppletError> {
    move |err| vec![AppletError::from_io(APPLET, action, path.as_deref(), err)]
}

#[cfg(test)]
mod tests {
    use super::{AddressFamily, Options, parse_args};
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
                add_address: None,
                del_address: None,
                netmask: None,
                broadcast: None,
                pointopoint: None,
                mtu: Some(1400),
                metric: None,
                tx_queue_len: None,
                hwaddr: None,
                up: Some(true),
                arp: None,
                multicast: None,
                allmulti: None,
                promisc: None,
            }
        );
    }

    #[test]
    fn parses_link_flags_and_queue_length() {
        let options = parse_args(&args(&[
            "eth0",
            "metric",
            "2",
            "txqueuelen",
            "200",
            "-arp",
            "promisc",
        ]))
        .unwrap();
        assert_eq!(
            options,
            Options {
                all: false,
                interface: Some(String::from("eth0")),
                address: None,
                prefix_len: None,
                add_address: None,
                del_address: None,
                netmask: None,
                broadcast: None,
                pointopoint: None,
                mtu: None,
                metric: Some(2),
                tx_queue_len: Some(200),
                hwaddr: None,
                up: None,
                arp: Some(false),
                multicast: None,
                allmulti: None,
                promisc: Some(true),
            }
        );
    }

    #[test]
    fn parses_add_and_del_addresses() {
        let options = parse_args(&args(&["eth0", "add", "2001:db8::2/64"])).unwrap();
        assert_eq!(
            options.add_address,
            Some((AddressFamily::Inet6, String::from("2001:db8::2"), 64))
        );

        let options = parse_args(&args(&["eth0", "del", "10.0.0.2"])).unwrap();
        assert_eq!(
            options.del_address,
            Some((AddressFamily::Inet4, String::from("10.0.0.2")))
        );
    }
}
