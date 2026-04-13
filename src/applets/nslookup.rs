use std::collections::BTreeSet;
use std::io::Write;
use std::net::ToSocketAddrs;

use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "nslookup";

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(()) => 0,
        Err(errors) => {
            for error in errors {
                error.print();
            }
            1
        }
    }
}

fn run(args: &[String]) -> Result<(), Vec<AppletError>> {
    let host = parse_args(args)?;
    let addresses = lookup_host(&host)?;
    let mut out = stdout();
    writeln!(out, "Name:\t{host}")
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    for (index, address) in addresses.iter().enumerate() {
        writeln!(out, "Address {}:\t{}", index + 1, address)
            .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    }
    out.flush()
        .map_err(|err| vec![AppletError::from_io(APPLET, "writing stdout", None, err)])?;
    Ok(())
}

fn parse_args(args: &[String]) -> Result<String, Vec<AppletError>> {
    match args {
        [] => Err(vec![AppletError::new(APPLET, "missing operand")]),
        [host] => Ok(host.clone()),
        _ => Err(vec![AppletError::new(APPLET, "extra operand")]),
    }
}

fn lookup_host(host: &str) -> Result<Vec<String>, Vec<AppletError>> {
    let mut addresses = BTreeSet::new();
    for address in (host, 0).to_socket_addrs().map_err(|err| {
        vec![AppletError::new(
            APPLET,
            format!("lookup failed for '{host}': {err}"),
        )]
    })? {
        addresses.insert(address.ip().to_string());
    }

    if addresses.is_empty() {
        Err(vec![AppletError::new(
            APPLET,
            format!("lookup failed for '{host}'"),
        )])
    } else {
        Ok(addresses.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{lookup_host, parse_args};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_host() {
        assert_eq!(
            parse_args(&args(&["localhost"])).expect("parse nslookup"),
            "localhost"
        );
    }

    #[test]
    fn lookup_localhost_resolves() {
        let addresses = lookup_host("localhost").expect("lookup localhost");
        assert!(
            addresses
                .iter()
                .any(|address| address == "127.0.0.1" || address == "::1")
        );
    }
}
