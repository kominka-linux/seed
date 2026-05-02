use std::collections::{BTreeSet, HashMap};
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
use md5::Digest as _;

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
    keyfile: Option<PathBuf>,
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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ConfigFile {
    peers: Vec<PeerSpec>,
    keyfile: Option<PathBuf>,
    trusted_keys: BTreeSet<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PeerSpec {
    address: String,
    key_id: Option<u32>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AuthConfig {
    keys: HashMap<u32, AuthKey>,
    trusted_keys: BTreeSet<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AuthKey {
    id: u32,
    algorithm: AuthAlgorithm,
    secret: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthAlgorithm {
    Md5,
    Sha1,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish_code(run(args))
}

fn run(args: &[std::ffi::OsString]) -> AppletCodeResult {
    let options = parse_args(args)?;
    let (peers, auth) = load_runtime_config(&options)?;
    if peers.is_empty() && !options.listen {
        return Err(vec![AppletError::new(APPLET, "no peers specified")]);
    }
    run_with_options(options, peers, auth)
}

fn run_with_options(options: Options, peers: Vec<PeerSpec>, auth: AuthConfig) -> AppletCodeResult {
    let mut last_sync = if peers.is_empty() {
        None
    } else {
        Some(sync_once(&options, &peers, &auth)?)
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
        if !peers.is_empty() {
            match sync_once(&options, &peers, &auth) {
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

        serve_once(&server_socket, last_sync.as_ref(), options.verbose, &auth)?;
        thread::sleep(Duration::from_secs(DEFAULT_POLL_SECS));
    }
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<Options, Vec<AppletError>> {
    let mut cursor = ArgCursor::new(args);
    let mut options = Options {
        verbose: 0,
        quit_after_sync: false,
        query_only: false,
        script: None,
        peers: Vec::new(),
        listen: false,
        interface: None,
        keyfile: None,
    };

    while let Some(arg) = cursor.next_arg(APPLET)? {
        match arg {
            ArgToken::Operand(value) => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unexpected operand '{value}'"),
                )]);
            }
            ArgToken::LongOption("debug", None) => {
                options.verbose = options.verbose.saturating_add(1);
            }
            ArgToken::LongOption("quit", None) => options.quit_after_sync = true,
            ArgToken::LongOption("wait", None) => {
                options.query_only = true;
                options.quit_after_sync = true;
            }
            ArgToken::LongOption("listen", None) => options.listen = true,
            ArgToken::LongOption("script", attached) => {
                options.script = Some(
                    cursor
                        .next_value_or_maybe_attached(attached, APPLET, "S")?
                        .to_string(),
                );
            }
            ArgToken::LongOption("peer", attached) => {
                options.peers.push(
                    cursor
                        .next_value_or_maybe_attached(attached, APPLET, "p")?
                        .to_string(),
                );
            }
            ArgToken::LongOption("interface", attached) => {
                options.interface = Some(
                    cursor
                        .next_value_or_maybe_attached(attached, APPLET, "I")?
                        .to_string(),
                );
            }
            ArgToken::LongOption("keyfile", attached) => {
                options.keyfile = Some(PathBuf::from(
                    cursor.next_value_or_maybe_attached(attached, APPLET, "k")?,
                ));
            }
            ArgToken::LongOption(name, _) => {
                return Err(vec![AppletError::unrecognized_option(
                    APPLET,
                    &format!("--{name}"),
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
                            let attached = &flags[offset + flag.len_utf8()..];
                            options.keyfile = Some(PathBuf::from(
                                cursor.next_value_or_attached(attached, APPLET, "k")?,
                            ));
                            break;
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

fn load_runtime_config(options: &Options) -> Result<(Vec<PeerSpec>, AuthConfig), Vec<AppletError>> {
    let file_config = if options.peers.is_empty() && !options.listen {
        read_configured_state()?
    } else {
        ConfigFile::default()
    };
    let peers = if options.peers.is_empty() && !options.listen {
        file_config.peers.clone()
    } else {
        options
            .peers
            .iter()
            .map(|peer| parse_peer_spec(peer))
            .collect::<Result<Vec<_>, _>>()?
    };
    let auth = load_auth_config(options.keyfile.as_deref(), &file_config)?;
    Ok((peers, auth))
}

fn read_configured_state() -> Result<ConfigFile, Vec<AppletError>> {
    let path = env::var_os("SEED_NTP_CONF")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONF));
    read_configured_state_from_path(&path)
}

fn read_configured_state_from_path(path: &Path) -> Result<ConfigFile, Vec<AppletError>> {
    let path_text = path.to_string_lossy().into_owned();
    let text = fs::read_to_string(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&path_text), err)])?;
    let mut config = ConfigFile::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let parts = line.split_whitespace().collect::<Vec<_>>();
        match parts.first().copied() {
            Some("server") | Some("peer") => {
                if parts.len() < 2 {
                    continue;
                }
                let mut peer = PeerSpec {
                    address: parts[1].to_string(),
                    key_id: None,
                };
                let mut index = 2;
                while index < parts.len() {
                    if parts[index] == "key" {
                        if let Some(value) = parts.get(index + 1) {
                            peer.key_id = Some(parse_key_id(value)?);
                        }
                        break;
                    }
                    index += 1;
                }
                config.peers.push(peer);
            }
            Some("keys") => {
                if let Some(value) = parts.get(1) {
                    config.keyfile = Some(PathBuf::from(value));
                }
            }
            Some("trustedkey") => {
                for value in &parts[1..] {
                    config.trusted_keys.insert(parse_key_id(value)?);
                }
            }
            _ => {}
        }
    }
    Ok(config)
}

fn load_auth_config(
    cli_keyfile: Option<&Path>,
    file_config: &ConfigFile,
) -> Result<AuthConfig, Vec<AppletError>> {
    let path = cli_keyfile.or(file_config.keyfile.as_deref());
    let keys = match path {
        Some(path) => read_keys_from_path(path)?,
        None => HashMap::new(),
    };
    Ok(AuthConfig {
        keys,
        trusted_keys: file_config.trusted_keys.clone(),
    })
}

fn sync_once(
    options: &Options,
    peers: &[PeerSpec],
    auth: &AuthConfig,
) -> Result<SyncResult, Vec<AppletError>> {
    let mut errors = Vec::new();
    for peer in peers {
        match query_peer(peer, options.interface.as_deref(), auth) {
            Ok(result) => return Ok(result),
            Err(err) => errors.extend(err),
        }
    }
    Err(errors)
}

fn query_peer(
    peer: &PeerSpec,
    interface: Option<&str>,
    auth: &AuthConfig,
) -> Result<SyncResult, Vec<AppletError>> {
    let peer_addr = resolve_peer(&peer.address)?;
    let auth_key = peer_auth_key(peer, auth)?;
    let socket = UdpSocket::bind(("0.0.0.0", 0))
        .map_err(|err| vec![AppletError::from_io(APPLET, "binding", None, err)])?;
    maybe_bind_to_device(&socket, interface)?;
    socket
        .set_read_timeout(Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS)))
        .map_err(|err| vec![AppletError::from_io(APPLET, "configuring", None, err)])?;

    let transmit_time = now_ntp_timestamp()?;
    let request = encode_packet(
        &NtpPacket::client_request(transmit_time),
        auth_key.map(|key| (key.id, auth_digest(key, &NtpPacket::client_request(transmit_time).encode()))),
    );
    socket
        .send_to(&request, peer_addr)
        .map_err(|err| vec![AppletError::from_io(APPLET, "sending", None, err)])?;

    let mut buffer = [0_u8; 512];
    let (read, from) = socket
        .recv_from(&mut buffer)
        .map_err(|err| vec![AppletError::from_io(APPLET, "receiving", None, err)])?;
    let destination_time = now_ntp_timestamp()?;
    let packet = NtpPacket::decode(&buffer[..read])?;
    validate_response(&packet, &buffer[..read], transmit_time, auth_key)?;
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
    raw_packet: &[u8],
    originate_timestamp: NtpTimestamp,
    auth_key: Option<&AuthKey>,
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
    if let Some(key) = auth_key {
        validate_packet_auth(raw_packet, key)?;
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
    auth: &AuthConfig,
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
    let auth_key = if read > 48 {
        Some(validate_incoming_auth(&buffer[..read], auth)?)
    } else {
        None
    };

    let receive_time = now_ntp_timestamp()?;
    let transmit_time = now_ntp_timestamp()?;
    let stratum = last_sync
        .map(|sync| sync.stratum.saturating_add(1).min(16))
        .unwrap_or(1);
    let response_packet = NtpPacket::server_response(
        request.transmit_timestamp,
        receive_time,
        transmit_time,
        last_sync
            .map(|sync| sync.transmit_timestamp)
            .unwrap_or(transmit_time),
        stratum,
    );
    let response = encode_packet(
        &response_packet,
        auth_key.map(|key| (key.id, auth_digest(key, &response_packet.encode()))),
    );
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

fn parse_peer_spec(value: &str) -> Result<PeerSpec, Vec<AppletError>> {
    if let Some(rest) = value.strip_prefix("keyno:") {
        let Some((key_id, address)) = rest.split_once(':') else {
            return Err(vec![AppletError::new(APPLET, "expected 'keyno:ID:HOST'")]);
        };
        return Ok(PeerSpec {
            address: address.to_string(),
            key_id: Some(parse_key_id(key_id)?),
        });
    }
    Ok(PeerSpec {
        address: value.to_string(),
        key_id: None,
    })
}

fn parse_key_id(value: &str) -> Result<u32, Vec<AppletError>> {
    value
        .parse::<u32>()
        .ok()
        .filter(|key| (1..=65535).contains(key))
        .ok_or_else(|| vec![AppletError::new(APPLET, format!("invalid key id '{value}'"))])
}

fn read_keys_from_path(path: &Path) -> Result<HashMap<u32, AuthKey>, Vec<AppletError>> {
    let path_text = path.to_string_lossy().into_owned();
    let text = fs::read_to_string(path)
        .map_err(|err| vec![AppletError::from_io(APPLET, "reading", Some(&path_text), err)])?;
    let mut keys = HashMap::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 3 {
            return Err(vec![AppletError::new(
                APPLET,
                format!("malformed key at line {}", line_number + 1),
            )]);
        }
        let id = parse_key_id(parts[0])?;
        let algorithm = parse_key_algorithm(parts[1], line_number + 1)?;
        let secret = parse_key_secret(algorithm, parts[2], line_number + 1)?;
        keys.insert(
            id,
            AuthKey {
                id,
                algorithm,
                secret,
            },
        );
    }
    Ok(keys)
}

fn parse_key_algorithm(value: &str, line_number: usize) -> Result<AuthAlgorithm, Vec<AppletError>> {
    if value.eq_ignore_ascii_case("m") || value.eq_ignore_ascii_case("md5") {
        Ok(AuthAlgorithm::Md5)
    } else if value.eq_ignore_ascii_case("sha") || value.eq_ignore_ascii_case("sha1") {
        Ok(AuthAlgorithm::Sha1)
    } else {
        Err(vec![AppletError::new(
            APPLET,
            format!("unsupported key type '{}' at line {}", value, line_number),
        )])
    }
}

fn parse_key_secret(
    algorithm: AuthAlgorithm,
    value: &str,
    line_number: usize,
) -> Result<Vec<u8>, Vec<AppletError>> {
    match algorithm {
        AuthAlgorithm::Md5 => {
            if value.is_empty() || value.len() > 16 {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("malformed key at line {}", line_number),
                )]);
            }
            Ok(value.as_bytes().to_vec())
        }
        AuthAlgorithm::Sha1 => decode_hex(value).ok_or_else(|| {
            vec![AppletError::new(
                APPLET,
                format!("malformed key at line {}", line_number),
            )]
        }),
    }
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let chars = value.as_bytes().chunks(2);
    for chunk in chars {
        let text = std::str::from_utf8(chunk).ok()?;
        bytes.push(u8::from_str_radix(text, 16).ok()?);
    }
    Some(bytes)
}

fn peer_auth_key<'a>(peer: &PeerSpec, auth: &'a AuthConfig) -> Result<Option<&'a AuthKey>, Vec<AppletError>> {
    let Some(key_id) = peer.key_id else {
        return Ok(None);
    };
    if !auth.trusted_keys.is_empty() && !auth.trusted_keys.contains(&key_id) {
        return Err(vec![AppletError::new(
            APPLET,
            format!("key {key_id} is not trusted"),
        )]);
    }
    auth.keys.get(&key_id).map(Some).ok_or_else(|| {
        vec![AppletError::new(
            APPLET,
            format!("key {key_id} is not defined"),
        )]
    })
}

fn encode_packet(packet: &NtpPacket, auth: Option<(u32, Vec<u8>)>) -> Vec<u8> {
    let mut bytes = packet.encode().to_vec();
    if let Some((key_id, digest)) = auth {
        bytes.extend_from_slice(&key_id.to_be_bytes());
        bytes.extend_from_slice(&digest);
    }
    bytes
}

fn auth_digest(key: &AuthKey, packet: &[u8; 48]) -> Vec<u8> {
    match key.algorithm {
        AuthAlgorithm::Md5 => {
            let mut digest = md5::Md5::new();
            digest.update(&key.secret);
            digest.update(packet);
            digest.finalize().to_vec()
        }
        AuthAlgorithm::Sha1 => {
            let mut digest = sha1::Sha1::new();
            digest.update(&key.secret);
            digest.update(packet);
            digest.finalize().to_vec()
        }
    }
}

fn validate_packet_auth(raw_packet: &[u8], key: &AuthKey) -> Result<(), Vec<AppletError>> {
    let digest_len = match key.algorithm {
        AuthAlgorithm::Md5 => 16,
        AuthAlgorithm::Sha1 => 20,
    };
    if raw_packet.len() != 48 + 4 + digest_len {
        return Err(vec![AppletError::new(APPLET, "invalid authenticated NTP packet")]);
    }
    let key_id = u32::from_be_bytes(raw_packet[48..52].try_into().expect("length checked"));
    if key_id != key.id {
        return Err(vec![AppletError::new(APPLET, "unexpected NTP key id")]);
    }
    let expected = auth_digest(key, &raw_packet[..48].try_into().expect("length checked"));
    if expected.as_slice() != &raw_packet[52..] {
        return Err(vec![AppletError::new(APPLET, "invalid NTP authentication digest")]);
    }
    Ok(())
}

fn validate_incoming_auth<'a>(
    raw_packet: &[u8],
    auth: &'a AuthConfig,
) -> Result<&'a AuthKey, Vec<AppletError>> {
    if raw_packet.len() < 52 {
        return Err(vec![AppletError::new(APPLET, "invalid authenticated NTP packet")]);
    }
    let key_id = u32::from_be_bytes(raw_packet[48..52].try_into().expect("length checked"));
    if !auth.trusted_keys.is_empty() && !auth.trusted_keys.contains(&key_id) {
        return Err(vec![AppletError::new(
            APPLET,
            format!("key {key_id} is not trusted"),
        )]);
    }
    let key = auth.keys.get(&key_id).ok_or_else(|| {
        vec![AppletError::new(
            APPLET,
            format!("key {key_id} is not defined"),
        )]
    })?;
    validate_packet_auth(raw_packet, key)?;
    Ok(key)
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
    use std::path::Path;

    use super::{
        AuthAlgorithm, AuthKey, NtpPacket, NtpTimestamp, auth_digest, compute_offset_delay,
        parse_args, parse_peer_spec, read_configured_state_from_path, read_keys_from_path,
        validate_packet_auth,
    };
    use crate::common::unix::temp_dir;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn parses_flags_and_peers() {
        let options =
            parse_args(&args(&["-dwq", "-k", "/etc/ntp.keys", "-S", "/bin/true", "-p", "127.0.0.1"]))
                .unwrap();
        assert_eq!(options.verbose, 1);
        assert!(options.query_only);
        assert!(options.quit_after_sync);
        assert_eq!(options.keyfile.as_deref(), Some(Path::new("/etc/ntp.keys")));
        assert_eq!(options.script.as_deref(), Some("/bin/true"));
        assert_eq!(options.peers, vec![String::from("127.0.0.1")]);
    }

    #[test]
    fn reads_peer_and_key_lines_from_config() {
        let dir = temp_dir("ntpd");
        let conf = dir.join("ntp.conf");
        std::fs::write(
            &conf,
            "keys /etc/ntp.keys\ntrustedkey 4 7\nserver 127.0.0.1 key 4 iburst\npeer pool.ntp.org prefer\n",
        )
        .unwrap();
        let config = read_configured_state_from_path(&conf).unwrap();
        assert_eq!(config.keyfile.as_deref(), Some(Path::new("/etc/ntp.keys")));
        assert!(config.trusted_keys.contains(&4));
        assert!(config.trusted_keys.contains(&7));
        assert_eq!(
            config.peers,
            vec![
                super::PeerSpec {
                    address: String::from("127.0.0.1"),
                    key_id: Some(4),
                },
                super::PeerSpec {
                    address: String::from("pool.ntp.org"),
                    key_id: None,
                },
            ]
        );
    }

    #[test]
    fn parses_keyed_peer_spec() {
        assert_eq!(
            parse_peer_spec("keyno:5:time.example.com").unwrap(),
            super::PeerSpec {
                address: String::from("time.example.com"),
                key_id: Some(5),
            }
        );
    }

    #[test]
    fn reads_md5_and_sha1_keys() {
        let dir = temp_dir("ntpd");
        let path = dir.join("ntp.keys");
        std::fs::write(&path, "1 MD5 secret\n2 SHA1 00112233445566778899aabbccddeeff00112233\n")
            .unwrap();
        let keys = read_keys_from_path(&path).unwrap();
        assert_eq!(keys[&1].algorithm, AuthAlgorithm::Md5);
        assert_eq!(keys[&1].secret, b"secret");
        assert_eq!(keys[&2].algorithm, AuthAlgorithm::Sha1);
        assert_eq!(keys[&2].secret.len(), 20);
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

    #[test]
    fn validates_authenticated_packet() {
        let packet = NtpPacket::client_request(NtpTimestamp::from_parts(10, 11));
        let key = AuthKey {
            id: 3,
            algorithm: AuthAlgorithm::Md5,
            secret: b"secret".to_vec(),
        };
        let encoded = super::encode_packet(&packet, Some((key.id, auth_digest(&key, &packet.encode()))));
        validate_packet_auth(&encoded, &key).unwrap();
    }
}
