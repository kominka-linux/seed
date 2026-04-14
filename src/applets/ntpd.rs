use std::env;
use std::fs;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::common::applet::{AppletCodeResult, finish_code};
use crate::common::args::{ArgCursor, ArgToken};
use crate::common::error::AppletError;

const APPLET: &str = "ntpd";
const DEFAULT_CONF: &str = "/etc/ntp.conf";
const DEFAULT_PORT: u16 = 123;
const DEFAULT_TIMEOUT_SECS: u64 = 3;
const DEFAULT_POLL_SECS: u64 = 64;
const NTP_UNIX_EPOCH_DELTA: u64 = 2_208_988_800;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Options {
    verbose: u8,
    quit_after_sync: bool,
    query_only: bool,
    script: Option<String>,
    peers: Vec<String>,
    listen: bool,
    interface: Option<String>,
}

#[derive(Clone, Debug)]
struct SyncResult {
    peer: SocketAddr,
    offset_seconds: f64,
    delay_seconds: f64,
    stratum: u8,
    transmit_timestamp: NtpTimestamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NtpTimestamp(u64);

#[derive(Clone, Copy, Debug)]
struct NtpPacket {
    li_vn_mode: u8,
    stratum: u8,
    poll: i8,
    precision: i8,
    root_delay: u32,
    root_dispersion: u32,
    reference_id: u32,
    reference_timestamp: NtpTimestamp,
    originate_timestamp: NtpTimestamp,
    receive_timestamp: NtpTimestamp,
    transmit_timestamp: NtpTimestamp,
}

pub fn main(args: &[String]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[String]) -> AppletCodeResult {
    let options = parse_args(args)?;
    if options.peers.is_empty() && !options.listen {
        let peers = read_configured_peers()?;
        if peers.is_empty() {
            return Err(vec![AppletError::new(APPLET, "no peers specified")]);
        }
        return run_with_options(Options { peers, ..options });
    }
    run_with_options(options)
}

fn run_with_options(options: Options) -> AppletCodeResult {
    let mut last_sync = if options.peers.is_empty() {
        None
    } else {
        Some(sync_once(&options)?)
    };

    if let Some(sync) = &last_sync {
        if !options.query_only {
            step_time(sync.offset_seconds)?;
        }
        run_script(&options, sync)?;
    }

    if options.quit_after_sync || !options.listen {
        return Ok(0);
    }

    let server_socket = bind_server_socket(options.interface.as_deref())?;
    loop {
        if !options.peers.is_empty() {
            match sync_once(&options) {
                Ok(sync) => {
                    if !options.query_only {
                        step_time(sync.offset_seconds)?;
                    }
                    run_script(&options, &sync)?;
                    last_sync = Some(sync);
                }
                Err(errors) if options.verbose > 0 => {
                    for error in errors {
                        error.print();
                    }
                }
                Err(_) => {}
            }
        }

        serve_once(&server_socket, last_sync.as_ref(), options.verbose)?;
        thread::sleep(Duration::from_secs(DEFAULT_POLL_SECS));
    }
}

fn parse_args(args: &[String]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut options = Options {
        verbose: 0,
        quit_after_sync: false,
        query_only: false,
        script: None,
        peers: Vec::new(),
        listen: false,
        interface: None,
    };

    while let Some(arg) = cursor.next_arg() {
        match arg {
            ArgToken::Operand(value) => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unexpected operand '{value}'"),
                )]);
            }
            ArgToken::ShortFlags(flags) => {
                let mut offset = 0;
                for flag in flags.chars() {
                    match flag {
                        'd' => options.verbose = options.verbose.saturating_add(1),
                        'n' | 'N' => {}
                        'q' => options.quit_after_sync = true,
                        'w' => {
                            options.query_only = true;
                            options.quit_after_sync = true;
                        }
                        'l' => options.listen = true,
                        'S' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.script = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "S")?
                                    .to_string(),
                            );
                            break;
                        }
                        'p' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.peers.push(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "p")?
                                    .to_string(),
                            );
                            break;
                        }
                        'I' => {
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.interface = Some(
                                cursor
                                    .next_value_or_attached(attached, APPLET, "I")?
                                    .to_string(),
                            );
                            break;
                        }
                        'k' => {
                            return Err(vec![AppletError::new(
                                APPLET,
                                "keyfile authentication is not supported",
                            )]);
                        }
                        other => return Err(vec![AppletError::invalid_option(APPLET, other)]),
                    }
                    offset += flag.len_utf8();
                }
            }
        }
    }

    Ok(options)
}

fn read_configured_peers() -> Result<Vec<String>, Vec<AppletError>> {
    let path = env::var_os("SEED_NTP_CONF")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONF));
    read_configured_peers_from_path(&path)
}

fn read_configured_peers_from_path(path: &Path) -> Result<Vec<String>, Vec<AppletError>> {
    let path_text = path.to_string_lossy().into_owned();
    let text = fs::read_to_string(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&path_text), err)])?;
    let mut peers = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        if matches!(parts.next(), Some("server")) && let Some(peer) = parts.next() {
            peers.push(peer.to_string());
        }
    }
    Ok(peers)
}

fn sync_once(options: &Options) -> Result<SyncResult, Vec<AppletError>> {
    let mut errors = Vec::new();
    for peer in &options.peers {
        match query_peer(peer, options.interface.as_deref()) {
            Ok(result) => return Ok(result),
            Err(err) => errors.extend(err),
        }
    }
    Err(errors)
}

fn query_peer(peer: &str, interface: Option<&str>) -> Result<SyncResult, Vec<AppletError>> {
    let peer_addr = resolve_peer(peer)?;
    let socket = UdpSocket::bind(("0.0.0.0", 0))
        .map_err(|err| vec![AppletError::from_io(APPLET, "binding", None, err)])?;
    maybe_bind_to_device(&socket, interface)?;
    socket
        .set_read_timeout(Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS)))
        .map_err(|err| vec![AppletError::from_io(APPLET, "configuring", None, err)])?;

    let transmit_time = now_ntp_timestamp()?;
    let request = NtpPacket::client_request(transmit_time).encode();
    socket
        .send_to(&request, peer_addr)
        .map_err(|err| vec![AppletError::from_io(APPLET, "sending", None, err)])?;

    let mut buffer = [0_u8; 512];
    let (read, from) = socket
        .recv_from(&mut buffer)
        .map_err(|err| vec![AppletError::from_io(APPLET, "receiving", None, err)])?;
    let destination_time = now_ntp_timestamp()?;
    let packet = NtpPacket::decode(&buffer[..read])?;
    validate_response(&packet, transmit_time)?;
    let (offset_seconds, delay_seconds) = compute_offset_delay(
        transmit_time,
        packet.receive_timestamp,
        packet.transmit_timestamp,
        destination_time,
    );

    Ok(SyncResult {
        peer: from,
        offset_seconds,
        delay_seconds,
        stratum: packet.stratum,
        transmit_timestamp: packet.transmit_timestamp,
    })
}

fn validate_response(
    packet: &NtpPacket,
    originate_timestamp: NtpTimestamp,
) -> Result<(), Vec<AppletError>> {
    let mode = packet.li_vn_mode & 0x7;
    if mode != 4 && mode != 5 {
        return Err(vec![AppletError::new(APPLET, "invalid NTP reply mode")]);
    }
    if packet.stratum == 0 || packet.stratum > 16 {
        return Err(vec![AppletError::new(APPLET, "invalid NTP stratum")]);
    }
    if packet.originate_timestamp != originate_timestamp {
        return Err(vec![AppletError::new(
            APPLET,
            "invalid NTP originate timestamp",
        )]);
    }
    Ok(())
}

fn maybe_bind_to_device(
    _socket: &UdpSocket,
    _interface: Option<&str>,
) -> Result<(), Vec<AppletError>> {
    #[cfg(not(target_os = "macos"))]
    if let Some(interface) = _interface {
        use std::ffi::CString;
        use std::os::fd::AsRawFd;

        let value = CString::new(interface)
            .map_err(|_| vec![AppletError::new(APPLET, "interface contains NUL byte")])?;
        let rc = unsafe {
            libc::setsockopt(
                _socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_BINDTODEVICE,
                value.as_ptr().cast(),
                value.as_bytes_with_nul().len() as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(vec![AppletError::from_io(
                APPLET,
                "binding to interface",
                Some(interface),
                io::Error::last_os_error(),
            )]);
        }
    }

    Ok(())
}

fn bind_server_socket(interface: Option<&str>) -> Result<UdpSocket, Vec<AppletError>> {
    let socket = UdpSocket::bind(("0.0.0.0", configured_port()))
        .map_err(|err| vec![AppletError::from_io(APPLET, "binding", None, err)])?;
    maybe_bind_to_device(&socket, interface)?;
    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|err| vec![AppletError::from_io(APPLET, "configuring", None, err)])?;
    Ok(socket)
}

fn serve_once(
    socket: &UdpSocket,
    last_sync: Option<&SyncResult>,
    verbose: u8,
) -> Result<(), Vec<AppletError>> {
    let mut buffer = [0_u8; 512];
    let (read, from) = match socket.recv_from(&mut buffer) {
        Ok(value) => value,
        Err(err)
            if matches!(err.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
        {
            return Ok(());
        }
        Err(err) => return Err(vec![AppletError::from_io(APPLET, "receiving", None, err)]),
    };

    let request = NtpPacket::decode(&buffer[..read])?;
    if request.li_vn_mode & 0x7 != 3 {
        return Ok(());
    }

    let receive_time = now_ntp_timestamp()?;
    let transmit_time = now_ntp_timestamp()?;
    let stratum = last_sync
        .map(|sync| sync.stratum.saturating_add(1).min(16))
        .unwrap_or(1);
    let response = NtpPacket::server_response(
        request.transmit_timestamp,
        receive_time,
        transmit_time,
        last_sync
            .map(|sync| sync.transmit_timestamp)
            .unwrap_or(transmit_time),
        stratum,
    )
    .encode();
    socket
        .send_to(&response, from)
        .map_err(|err| vec![AppletError::from_io(APPLET, "sending", None, err)])?;
    if verbose > 0 {
        eprintln!("{APPLET}: served {}", from);
    }
    Ok(())
}

fn run_script(options: &Options, sync: &SyncResult) -> Result<(), Vec<AppletError>> {
    let Some(script) = &options.script else {
        return Ok(());
    };

    let status = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .env("offset", sync.offset_seconds.to_string())
        .env("delay", sync.delay_seconds.to_string())
        .env("stratum", sync.stratum.to_string())
        .env("server", sync.peer.to_string())
        .status()
        .map_err(|err| vec![AppletError::from_io(APPLET, "executing", Some(script), err)])?;
    if status.success() {
        Ok(())
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("script exited with status {}", status.code().unwrap_or(1)),
        )])
    }
}

fn step_time(offset_seconds: f64) -> Result<(), Vec<AppletError>> {
    #[cfg(not(target_os = "macos"))]
    {
        let now = now_unix_timeval()?;
        let base = now.tv_sec as f64 + (now.tv_usec as f64 / 1_000_000.0);
        let adjusted = base + offset_seconds;
        let mut microseconds = ((adjusted.fract()) * 1_000_000.0).round() as _;
        let mut seconds = adjusted.floor() as _;
        if microseconds >= 1_000_000 {
            microseconds -= 1_000_000;
            seconds += 1;
        }
        let tv = libc::timeval {
            tv_sec: seconds,
            tv_usec: microseconds,
        };
        let rc = unsafe { libc::settimeofday(&tv, std::ptr::null()) };
        if rc == 0 {
            return Ok(());
        }
        Err(vec![AppletError::from_io(
            APPLET,
            "setting time",
            None,
            io::Error::last_os_error(),
        )])
    }

    #[cfg(target_os = "macos")]
    {
        let _ = offset_seconds;
        Err(vec![AppletError::new(
            APPLET,
            "setting time is unsupported on this platform",
        )])
    }
}

fn now_unix_timeval() -> Result<libc::timeval, Vec<AppletError>> {
    let mut tv = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let rc = unsafe { libc::gettimeofday(&mut tv, std::ptr::null_mut()) };
    if rc == 0 {
        Ok(tv)
    } else {
        Err(vec![AppletError::from_io(
            APPLET,
            "reading current time",
            None,
            io::Error::last_os_error(),
        )])
    }
}

fn now_ntp_timestamp() -> Result<NtpTimestamp, Vec<AppletError>> {
    let tv = now_unix_timeval()?;
    let seconds = (tv.tv_sec as u64).saturating_add(NTP_UNIX_EPOCH_DELTA);
    let fraction = ((tv.tv_usec as u64) << 32) / 1_000_000;
    Ok(NtpTimestamp((seconds << 32) | fraction))
}

fn configured_port() -> u16 {
    env::var("SEED_NTP_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT)
}

fn resolve_peer(peer: &str) -> Result<SocketAddr, Vec<AppletError>> {
    if let Ok(addr) = peer.parse::<SocketAddr>() {
        return Ok(addr);
    }

    let port = configured_port();
    (peer, port)
        .to_socket_addrs()
        .map_err(|err| vec![AppletError::new(APPLET, format!("resolving {peer}: {err}"))])?
        .next()
        .ok_or_else(|| vec![AppletError::new(APPLET, format!("resolving {peer}: no address"))])
}

fn compute_offset_delay(
    t1: NtpTimestamp,
    t2: NtpTimestamp,
    t3: NtpTimestamp,
    t4: NtpTimestamp,
) -> (f64, f64) {
    let t1 = t1.to_seconds();
    let t2 = t2.to_seconds();
    let t3 = t3.to_seconds();
    let t4 = t4.to_seconds();
    (((t2 - t1) + (t3 - t4)) / 2.0, (t4 - t1) - (t3 - t2))
}

impl NtpPacket {
    fn client_request(transmit_timestamp: NtpTimestamp) -> Self {
        Self {
            li_vn_mode: (4 << 3) | 3,
            stratum: 0,
            poll: 6,
            precision: -20,
            root_delay: 0,
            root_dispersion: 0,
            reference_id: 0,
            reference_timestamp: NtpTimestamp(0),
            originate_timestamp: NtpTimestamp(0),
            receive_timestamp: NtpTimestamp(0),
            transmit_timestamp,
        }
    }

    fn server_response(
        originate_timestamp: NtpTimestamp,
        receive_timestamp: NtpTimestamp,
        transmit_timestamp: NtpTimestamp,
        reference_timestamp: NtpTimestamp,
        stratum: u8,
    ) -> Self {
        Self {
            li_vn_mode: (4 << 3) | 4,
            stratum,
            poll: 6,
            precision: -20,
            root_delay: 0,
            root_dispersion: 0,
            reference_id: u32::from_be_bytes(*b"SEED"),
            reference_timestamp,
            originate_timestamp,
            receive_timestamp,
            transmit_timestamp,
        }
    }

    fn decode(bytes: &[u8]) -> Result<Self, Vec<AppletError>> {
        if bytes.len() < 48 {
            return Err(vec![AppletError::new(APPLET, "short NTP packet")]);
        }
        Ok(Self {
            li_vn_mode: bytes[0],
            stratum: bytes[1],
            poll: bytes[2] as i8,
            precision: bytes[3] as i8,
            root_delay: u32::from_be_bytes(bytes[4..8].try_into().expect("length checked")),
            root_dispersion: u32::from_be_bytes(
                bytes[8..12].try_into().expect("length checked"),
            ),
            reference_id: u32::from_be_bytes(bytes[12..16].try_into().expect("length checked")),
            reference_timestamp: NtpTimestamp(u64::from_be_bytes(
                bytes[16..24].try_into().expect("length checked"),
            )),
            originate_timestamp: NtpTimestamp(u64::from_be_bytes(
                bytes[24..32].try_into().expect("length checked"),
            )),
            receive_timestamp: NtpTimestamp(u64::from_be_bytes(
                bytes[32..40].try_into().expect("length checked"),
            )),
            transmit_timestamp: NtpTimestamp(u64::from_be_bytes(
                bytes[40..48].try_into().expect("length checked"),
            )),
        })
    }

    fn encode(self) -> [u8; 48] {
        let mut bytes = [0_u8; 48];
        bytes[0] = self.li_vn_mode;
        bytes[1] = self.stratum;
        bytes[2] = self.poll as u8;
        bytes[3] = self.precision as u8;
        bytes[4..8].copy_from_slice(&self.root_delay.to_be_bytes());
        bytes[8..12].copy_from_slice(&self.root_dispersion.to_be_bytes());
        bytes[12..16].copy_from_slice(&self.reference_id.to_be_bytes());
        bytes[16..24].copy_from_slice(&self.reference_timestamp.0.to_be_bytes());
        bytes[24..32].copy_from_slice(&self.originate_timestamp.0.to_be_bytes());
        bytes[32..40].copy_from_slice(&self.receive_timestamp.0.to_be_bytes());
        bytes[40..48].copy_from_slice(&self.transmit_timestamp.0.to_be_bytes());
        bytes
    }
}

impl NtpTimestamp {
    #[cfg(test)]
    fn from_parts(seconds: u32, fraction: u32) -> Self {
        Self((u64::from(seconds) << 32) | u64::from(fraction))
    }

    fn to_seconds(self) -> f64 {
        let seconds = (self.0 >> 32) as f64;
        let fraction = (self.0 & 0xffff_ffff) as f64 / 4_294_967_296.0;
        seconds + fraction
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NtpPacket, NtpTimestamp, compute_offset_delay, parse_args, read_configured_peers_from_path,
    };
    use crate::common::unix::temp_dir;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_peers() {
        let options = parse_args(&args(&["-dwq", "-S", "/bin/true", "-p", "127.0.0.1"])).unwrap();
        assert_eq!(options.verbose, 1);
        assert!(options.query_only);
        assert!(options.quit_after_sync);
        assert_eq!(options.script.as_deref(), Some("/bin/true"));
        assert_eq!(options.peers, vec![String::from("127.0.0.1")]);
    }

    #[test]
    fn reads_server_lines_from_config() {
        let dir = temp_dir("ntpd");
        let conf = dir.join("ntp.conf");
        std::fs::write(
            &conf,
            "server 127.0.0.1\n# comment\nserver pool.ntp.org iburst\n",
        )
        .unwrap();
        let peers = read_configured_peers_from_path(&conf).unwrap();
        assert_eq!(
            peers,
            vec![String::from("127.0.0.1"), String::from("pool.ntp.org")]
        );
    }

    #[test]
    fn encodes_and_decodes_ntp_packets() {
        let packet = NtpPacket::server_response(
            NtpTimestamp::from_parts(1, 2),
            NtpTimestamp::from_parts(3, 4),
            NtpTimestamp::from_parts(5, 6),
            NtpTimestamp::from_parts(7, 8),
            2,
        );
        let decoded = NtpPacket::decode(&packet.encode()).unwrap();
        assert_eq!(decoded.stratum, 2);
        assert_eq!(decoded.originate_timestamp, NtpTimestamp::from_parts(1, 2));
        assert_eq!(decoded.receive_timestamp, NtpTimestamp::from_parts(3, 4));
        assert_eq!(decoded.transmit_timestamp, NtpTimestamp::from_parts(5, 6));
    }

    #[test]
    fn computes_offset_and_delay() {
        let t1 = NtpTimestamp::from_parts(1000, 0);
        let t2 = NtpTimestamp::from_parts(1001, 0);
        let t3 = NtpTimestamp::from_parts(1002, 0);
        let t4 = NtpTimestamp::from_parts(1003, 0);
        let (offset, delay) = compute_offset_delay(t1, t2, t3, t4);
        assert!((offset - 0.0).abs() < 1e-9);
        assert!((delay - 2.0).abs() < 1e-9);
    }
}
