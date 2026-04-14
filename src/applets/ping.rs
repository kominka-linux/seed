use std::io;
use std::mem;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET_PING: &str = "ping";
const APPLET_PING6: &str = "ping6";
const DEFAULT_PAYLOAD_SIZE: usize = 56;
const DEFAULT_TIMEOUT_SECS: u64 = 1;
const ICMP_ECHO_REQUEST: u8 = 8;
const ICMP_ECHO_REPLY: u8 = 0;
const ICMP6_ECHO_REQUEST: u8 = 128;
const ICMP6_ECHO_REPLY: u8 = 129;

static RUNNING: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Family {
    Inet4,
    Inet6,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    family: Family,
    count: Option<u32>,
    timeout: Duration,
    payload_size: usize,
    quiet: bool,
    host: String,
}

#[derive(Clone, Copy, Debug)]
struct Reply {
    bytes: usize,
    sequence: u16,
    elapsed: Duration,
}

pub fn main(args: &[String]) -> i32 {
    finish_code(run(APPLET_PING, Family::Inet4, args))
}

pub fn main_ping6(args: &[String]) -> i32 {
    finish_code(run(APPLET_PING6, Family::Inet6, args))
}

fn run(applet: &'static str, family: Family, args: &[String]) -> AppletCodeResult {
    let options = parse_args(applet, family, args)?;
    let _guard = SigintGuard::install();
    ping(&options)
}

fn parse_args(applet: &'static str, family: Family, args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut options = Options {
        family,
        count: None,
        timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        payload_size: DEFAULT_PAYLOAD_SIZE,
        quiet: false,
        host: String::new(),
    };
    let mut cursor = ArgCursor::new(args);

    while let Some(token) = cursor.next_arg() {
        match token {
            ArgToken::ShortFlags(flags) => {
                for (index, flag) in flags.char_indices() {
                    match flag {
                        '4' => options.family = Family::Inet4,
                        '6' => options.family = Family::Inet6,
                        'q' => options.quiet = true,
                        'c' => {
                            let attached = &flags[index + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, applet, "c")?;
                            options.count = Some(parse_u32(applet, "count", value)?);
                            break;
                        }
                        'W' => {
                            let attached = &flags[index + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, applet, "W")?;
                            options.timeout =
                                Duration::from_secs(parse_u32(applet, "timeout", value)? as u64);
                            break;
                        }
                        's' => {
                            let attached = &flags[index + flag.len_utf8()..];
                            let value = cursor.next_value_or_attached(attached, applet, "s")?;
                            options.payload_size = parse_u32(applet, "size", value)? as usize;
                            break;
                        }
                        _ => return Err(vec![AppletError::invalid_option(applet, flag)]),
                    }
                }
            }
            ArgToken::Operand(value) => {
                if options.host.is_empty() {
                    options.host = value.to_string();
                } else {
                    return Err(vec![AppletError::new(applet, "extra operand")]);
                }
            }
        }
    }

    if options.host.is_empty() {
        return Err(vec![AppletError::new(applet, "missing operand")]);
    }

    Ok(options)
}

fn parse_u32(applet: &'static str, what: &str, value: &str) -> Result<u32, Vec<AppletError>> {
    value
        .parse::<u32>()
        .map_err(|_| vec![AppletError::new(applet, format!("invalid {what} '{value}'"))])
}

fn ping(options: &Options) -> AppletCodeResult {
    let target = resolve_target(options)?;
    let fd = open_socket(options.family)?;
    let _fd = FdGuard(fd);

    connect_socket(fd, options.family, &target.addr)?;
    set_receive_timeout(fd, options.family, options.timeout)?;

    let destination = sockaddr_to_string(&target.addr)?;
    println!(
        "PING {} ({}): {} data bytes",
        options.host, destination, options.payload_size
    );

    let limit = options.count.unwrap_or(u32::MAX);
    let mut transmitted = 0_u32;
    let mut received = 0_u32;
    let mut total = Duration::ZERO;
    let mut min = None::<Duration>;
    let mut max = None::<Duration>;

    for sequence in 0..limit {
        if !RUNNING.load(Ordering::Relaxed) {
            break;
        }
        transmitted += 1;
        let reply = send_and_receive(fd, options.family, sequence as u16, options.payload_size)?;
        if let Some(reply) = reply {
            received += 1;
            total += reply.elapsed;
            min = Some(min.map_or(reply.elapsed, |current| current.min(reply.elapsed)));
            max = Some(max.map_or(reply.elapsed, |current| current.max(reply.elapsed)));
            if !options.quiet {
                println!(
                    "{} bytes from {}: icmp_seq={} time={:.3} ms",
                    reply.bytes,
                    destination,
                    reply.sequence,
                    duration_ms(reply.elapsed)
                );
            }
        }

        if sequence + 1 < limit {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    println!();
    println!("--- {} ping statistics ---", options.host);
    let loss = if transmitted == 0 {
        0
    } else {
        ((transmitted - received) * 100) / transmitted
    };
    println!(
        "{} packets transmitted, {} packets received, {}% packet loss",
        transmitted, received, loss
    );
    if received > 0 {
        let avg = total.as_secs_f64() / f64::from(received);
        println!(
            "round-trip min/avg/max = {:.3}/{:.3}/{:.3} ms",
            duration_ms(min.unwrap_or(Duration::ZERO)),
            avg * 1000.0,
            duration_ms(max.unwrap_or(Duration::ZERO))
        );
        Ok(0)
    } else {
        Ok(1)
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn send_and_receive(
    fd: libc::c_int,
    family: Family,
    sequence: u16,
    payload_size: usize,
) -> Result<Option<Reply>, Vec<AppletError>> {
    let request = build_request(family, sequence, payload_size);
    let started = Instant::now();
    let sent = unsafe { libc::send(fd, request.as_ptr().cast(), request.len(), 0) };
    if sent < 0 {
        return Err(vec![AppletError::from_io(
            applet_name(family),
            "sending",
            None,
            io::Error::last_os_error(),
        )]);
    }

    let mut buffer = [0_u8; 2048];
    loop {
        let read = unsafe { libc::recv(fd, buffer.as_mut_ptr().cast(), buffer.len(), 0) };
        if read < 0 {
            let err = io::Error::last_os_error();
            return match err.kind() {
                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Ok(None),
                _ => Err(vec![AppletError::from_io(applet_name(family), "receiving", None, err)]),
            };
        }
        let read = read as usize;
        if let Some(reply_sequence) = parse_reply(family, &buffer[..read]) {
            return Ok(Some(Reply {
                bytes: read,
                sequence: reply_sequence,
                elapsed: started.elapsed(),
            }));
        }
    }
}

fn build_request(family: Family, sequence: u16, payload_size: usize) -> Vec<u8> {
    let mut packet = Vec::with_capacity(8 + payload_size);
    packet.push(match family {
        Family::Inet4 => ICMP_ECHO_REQUEST,
        Family::Inet6 => ICMP6_ECHO_REQUEST,
    });
    packet.push(0);
    packet.extend_from_slice(&[0, 0]);
    packet.extend_from_slice(&0_u16.to_be_bytes());
    packet.extend_from_slice(&sequence.to_be_bytes());
    for index in 0..payload_size {
        packet.push(index as u8);
    }
    let checksum = checksum(&packet);
    packet[2] = (checksum >> 8) as u8;
    packet[3] = checksum as u8;
    packet
}

fn parse_reply(family: Family, packet: &[u8]) -> Option<u16> {
    if packet.len() < 8 {
        return None;
    }
    let expected_type = match family {
        Family::Inet4 => ICMP_ECHO_REPLY,
        Family::Inet6 => ICMP6_ECHO_REPLY,
    };
    if packet[0] != expected_type || packet[1] != 0 {
        return None;
    }
    Some(u16::from_be_bytes([packet[6], packet[7]]))
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum = 0_u32;
    for chunk in bytes.chunks(2) {
        let word = match chunk {
            [left, right] => u16::from_be_bytes([*left, *right]) as u32,
            [left] => u16::from_be_bytes([*left, 0]) as u32,
            _ => 0,
        };
        sum += word;
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn open_socket(family: Family) -> Result<libc::c_int, Vec<AppletError>> {
    let (domain, protocol) = match family {
        Family::Inet4 => (libc::AF_INET, libc::IPPROTO_ICMP),
        Family::Inet6 => (libc::AF_INET6, libc::IPPROTO_ICMPV6),
    };
    let fd = unsafe { libc::socket(domain, libc::SOCK_DGRAM, protocol) };
    if fd >= 0 {
        Ok(fd)
    } else {
        Err(vec![AppletError::from_io(
            applet_name(family),
            "creating socket",
            None,
            io::Error::last_os_error(),
        )])
    }
}

fn set_receive_timeout(
    fd: libc::c_int,
    family: Family,
    timeout: Duration,
) -> Result<(), Vec<AppletError>> {
    let tv = libc::timeval {
        tv_sec: timeout.as_secs().try_into().unwrap_or_default(),
        tv_usec: timeout.subsec_micros().into(),
    };
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&tv as *const libc::timeval).cast(),
            mem::size_of::<libc::timeval>() as libc::socklen_t,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::from_io(
            applet_name(family),
            "configuring socket",
            None,
            io::Error::last_os_error(),
        )])
    }
}

fn connect_socket(fd: libc::c_int, family: Family, addr: &SockAddr) -> Result<(), Vec<AppletError>> {
    let rc = unsafe { libc::connect(fd, addr.as_ptr(), addr.len()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(vec![AppletError::from_io(
            applet_name(family),
            "connecting",
            None,
            io::Error::last_os_error(),
        )])
    }
}

fn resolve_target(options: &Options) -> Result<ResolvedTarget, Vec<AppletError>> {
    let family = match options.family {
        Family::Inet4 => libc::AF_INET,
        Family::Inet6 => libc::AF_INET6,
    };
    let host = std::ffi::CString::new(options.host.as_str())
        .map_err(|_| vec![AppletError::new(applet_name(options.family), "host contains NUL byte")])?;
    let hints = libc::addrinfo {
        ai_flags: 0,
        ai_family: family,
        ai_socktype: 0,
        ai_protocol: 0,
        ai_addrlen: 0,
        ai_addr: std::ptr::null_mut(),
        ai_canonname: std::ptr::null_mut(),
        ai_next: std::ptr::null_mut(),
    };
    let mut result = std::ptr::null_mut::<libc::addrinfo>();
    let rc = unsafe { libc::getaddrinfo(host.as_ptr(), std::ptr::null(), &hints, &mut result) };
    if rc != 0 {
        let message = unsafe { std::ffi::CStr::from_ptr(libc::gai_strerror(rc)) }
            .to_string_lossy()
            .into_owned();
        return Err(vec![AppletError::new(
            applet_name(options.family),
            format!("bad address '{}': {message}", options.host),
        )]);
    }
    let _guard = AddrInfoGuard(result);
    let addr = unsafe { SockAddr::from_ptr((*result).ai_addr, (*result).ai_addrlen) }?;
    Ok(ResolvedTarget { addr })
}

struct ResolvedTarget {
    addr: SockAddr,
}

#[derive(Clone)]
struct SockAddr {
    storage: libc::sockaddr_storage,
    len: libc::socklen_t,
}

impl SockAddr {
    unsafe fn from_ptr(addr: *const libc::sockaddr, len: libc::socklen_t) -> Result<Self, Vec<AppletError>> {
        if addr.is_null() {
            return Err(vec![AppletError::new(APPLET_PING, "name resolution returned no address")]);
        }
        if (len as usize) > mem::size_of::<libc::sockaddr_storage>() {
            return Err(vec![AppletError::new(APPLET_PING, "resolved address is too large")]);
        }
        let mut storage = unsafe { mem::zeroed::<libc::sockaddr_storage>() };
        unsafe {
            std::ptr::copy_nonoverlapping(
                addr.cast::<u8>(),
                (&mut storage as *mut libc::sockaddr_storage).cast::<u8>(),
                len as usize,
            );
        }
        Ok(Self { storage, len })
    }

    fn as_ptr(&self) -> *const libc::sockaddr {
        (&self.storage as *const libc::sockaddr_storage).cast::<libc::sockaddr>()
    }

    fn len(&self) -> libc::socklen_t {
        self.len
    }
}

fn sockaddr_to_string(addr: &SockAddr) -> Result<String, Vec<AppletError>> {
    unsafe {
        match (*addr.as_ptr()).sa_family as libc::c_int {
            libc::AF_INET => {
                let sin = &*(addr.as_ptr() as *const libc::sockaddr_in);
                Ok(Ipv4Addr::from(sin.sin_addr.s_addr.to_ne_bytes()).to_string())
            }
            libc::AF_INET6 => {
                let sin6 = &*(addr.as_ptr() as *const libc::sockaddr_in6);
                Ok(Ipv6Addr::from(sin6.sin6_addr.s6_addr).to_string())
            }
            _ => Err(vec![AppletError::new(APPLET_PING, "unsupported address family")]),
        }
    }
}

fn applet_name(family: Family) -> &'static str {
    match family {
        Family::Inet4 => APPLET_PING,
        Family::Inet6 => APPLET_PING6,
    }
}

struct FdGuard(libc::c_int);

impl Drop for FdGuard {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

struct AddrInfoGuard(*mut libc::addrinfo);

impl Drop for AddrInfoGuard {
    fn drop(&mut self) {
        unsafe {
            libc::freeaddrinfo(self.0);
        }
    }
}

extern "C" fn handle_sigint(_: libc::c_int) {
    RUNNING.store(false, Ordering::Relaxed);
}

struct SigintGuard(Option<libc::sighandler_t>);

impl SigintGuard {
    fn install() -> Self {
        RUNNING.store(true, Ordering::Relaxed);
        let previous =
            unsafe { libc::signal(libc::SIGINT, handle_sigint as *const () as libc::sighandler_t) };
        if previous == libc::SIG_ERR {
            Self(None)
        } else {
            Self(Some(previous))
        }
    }
}

impl Drop for SigintGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.0 {
            unsafe {
                libc::signal(libc::SIGINT, previous);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Family, checksum, parse_args, parse_reply};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_count_timeout_and_size() {
        let options = parse_args("ping", Family::Inet4, &args(&["-c", "2", "-W1", "-s", "8", "127.0.0.1"]))
            .unwrap();
        assert_eq!(options.count, Some(2));
        assert_eq!(options.payload_size, 8);
        assert_eq!(options.host, "127.0.0.1");
    }

    #[test]
    fn checksum_matches_known_value() {
        let packet = [8_u8, 0, 0, 0, 0, 0, 0, 1];
        assert_eq!(checksum(&packet), 0xf7fe);
    }

    #[test]
    fn parses_echo_reply_sequence() {
        let packet = [0_u8, 0, 0, 0, 0, 0, 0x12, 0x34, 1, 2, 3];
        assert_eq!(parse_reply(Family::Inet4, &packet), Some(0x1234));
    }
}
