use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ureq::Agent;
use ureq::tls::{PemItem, RootCerts, TlsConfig, parse_pem};

use crate::common::applet::{AppletResult, finish};
use crate::common::args::ArgCursor;
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "wget";
const SHORT_HELP: &str = "\
wget - retrieve files over HTTP and HTTPS

usage: wget [OPTIONS] URL...

Try 'man wget' for details.
";

#[derive(Debug, Default)]
struct Options {
    quiet: bool,
    output_document: Option<String>,
    output_dir: Option<String>,
    no_check_certificate: bool,
    ca_certificate: Option<String>,
    headers: Vec<String>,
    post_data: Option<String>,
    post_file: Option<String>,
}

pub fn main(args: &[std::ffi::OsString]) -> i32 {
    finish(run(args))
}

pub(crate) fn short_help() -> &'static str {
    SHORT_HELP
}

fn run(args: &[std::ffi::OsString]) -> AppletResult {
    match parse_args(args)? {
        ParsedCommand::Help => {
            print!("{SHORT_HELP}");
            Ok(())
        }
        ParsedCommand::Fetch(options, urls) => {
            let agent = make_agent(&options).map_err(|e| vec![e])?;
            let mut errors = Vec::new();

            for url in &urls {
                if let Err(err) = fetch_url(&agent, url, &options) {
                    errors.push(err);
                }
            }

            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors)
            }
        }
    }
}

enum ParsedCommand {
    Help,
    Fetch(Options, Vec<String>),
}

fn parse_args(args: &[std::ffi::OsString]) -> Result<ParsedCommand, Vec<AppletError>> {
    let mut options = Options::default();
    let mut urls = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token(APPLET)? {
        if !cursor.parsing_flags() || !arg.starts_with('-') || arg.len() == 1 {
            urls.push(arg.to_owned());
            continue;
        }

        if let Some(val) = arg.strip_prefix("--ca-certificate=") {
            options.ca_certificate = Some(val.to_owned());
            continue;
        }
        if let Some(val) = arg.strip_prefix("--header=") {
            options.headers.push(val.to_owned());
            continue;
        }
        if let Some(val) = arg.strip_prefix("--post-data=") {
            options.post_data = Some(val.to_owned());
            options.post_file = None;
            continue;
        }
        if let Some(val) = arg.strip_prefix("--post-file=") {
            options.post_file = Some(val.to_owned());
            options.post_data = None;
            continue;
        }

        match arg {
            "--help" => return Ok(ParsedCommand::Help),
            "-O" => {
                options.output_document = Some(cursor.next_value(APPLET, "O")?.to_owned());
            }
            "-P" => {
                options.output_dir = Some(cursor.next_value(APPLET, "P")?.to_owned());
            }
            "--ca-certificate" => {
                options.ca_certificate =
                    Some(cursor.next_value(APPLET, "ca-certificate")?.to_owned());
            }
            "--header" => {
                options
                    .headers
                    .push(cursor.next_value(APPLET, "header")?.to_owned());
            }
            "--no-check-certificate" => {
                options.no_check_certificate = true;
            }
            "--post-data" => {
                options.post_data = Some(cursor.next_value(APPLET, "post-data")?.to_owned());
                options.post_file = None;
            }
            "--post-file" => {
                options.post_file = Some(cursor.next_value(APPLET, "post-file")?.to_owned());
                options.post_data = None;
            }
            _ if !arg.starts_with("--") => {
                for flag in arg[1..].chars() {
                    match flag {
                        'q' => options.quiet = true,
                        'k' => options.no_check_certificate = true,
                        _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                    }
                }
            }
            _ => {
                return Err(vec![AppletError::new(
                    APPLET,
                    format!("unrecognized option: '{arg}'"),
                )]);
            }
        }
    }

    if urls.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing URL")]);
    }

    Ok(ParsedCommand::Fetch(options, urls))
}

fn make_agent(options: &Options) -> Result<Agent, AppletError> {
    let ssl_cert_file = std::env::var("SSL_CERT_FILE").ok();
    let ca_file = options
        .ca_certificate
        .as_deref()
        .or(ssl_cert_file.as_deref());

    let tls_builder = TlsConfig::builder()
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()));

    let tls = if options.no_check_certificate {
        tls_builder.disable_verification(true)
    } else if let Some(path) = ca_file {
        let certs = load_ca_certs(path)?;
        tls_builder.root_certs(RootCerts::new_with_certs(&certs))
    } else {
        tls_builder.root_certs(RootCerts::PlatformVerifier)
    };

    Ok(Agent::config_builder()
        .tls_config(tls.build())
        .build()
        .new_agent())
}

fn load_ca_certs(path: &str) -> Result<Vec<ureq::tls::Certificate<'static>>, AppletError> {
    let pem =
        std::fs::read(path).map_err(|e| AppletError::from_io(APPLET, "reading", Some(path), e))?;
    let certs = parse_pem(&pem)
        .filter_map(|item| match item {
            Ok(PemItem::Certificate(c)) => Some(Ok(c)),
            Ok(_) => None,
            Err(e) => Some(Err(AppletError::new(
                APPLET,
                format!("parsing {path}: {e}"),
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        return Err(AppletError::new(
            APPLET,
            format!("no certificates found in {path}"),
        ));
    }
    Ok(certs)
}

fn fetch_url(agent: &Agent, url: &str, options: &Options) -> Result<(), AppletError> {
    let output_path = output_path(url, options)?;
    let mut request = ureq::http::Request::builder().uri(url);
    let request_body = request_body(options)?;
    request = if request_body.is_some() {
        request.method("POST")
    } else {
        request.method("GET")
    };
    for header in &options.headers {
        let (name, value) = split_header(header)?;
        request = request.header(name, value);
    }
    let mut response = if let Some(body) = request_body {
        agent.run(request.body(body).map_err(|err| {
            AppletError::new(APPLET, format!("building request for {url}: {err}"))
        })?)
    } else {
        agent.run(request.body(()).map_err(|err| {
            AppletError::new(APPLET, format!("building request for {url}: {err}"))
        })?)
    }
    .map_err(|err| AppletError::new(APPLET, format!("fetching {url}: {err}")))?;

    if output_path.as_os_str() == "-" {
        let mut out = stdout();
        copy_response_body(response.body_mut().as_reader(), &mut out, url)?;
        out.flush()
            .map_err(|err| AppletError::from_io(APPLET, "flushing", None, err))?;
        return Ok(());
    }

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| {
            AppletError::from_io(APPLET, "creating", Some(path_for_error(parent)), err)
        })?;
    }

    let mut file = File::create(&output_path).map_err(|err| {
        AppletError::from_io(APPLET, "creating", Some(path_for_error(&output_path)), err)
    })?;
    copy_response_body(response.body_mut().as_reader(), &mut file, url)
}

fn request_body(options: &Options) -> Result<Option<Vec<u8>>, AppletError> {
    if let Some(post_file) = options.post_file.as_deref() {
        return fs::read(post_file)
            .map(Some)
            .map_err(|err| AppletError::from_io(APPLET, "reading", Some(post_file), err));
    }

    Ok(options
        .post_data
        .as_ref()
        .map(|post_data| post_data.as_bytes().to_vec()))
}

fn copy_response_body(
    mut reader: impl Read,
    mut writer: impl Write,
    url: &str,
) -> Result<(), AppletError> {
    io::copy(&mut reader, &mut writer)
        .map_err(|err| AppletError::new(APPLET, format!("reading {url}: {err}")))?;
    Ok(())
}

fn output_path(url: &str, options: &Options) -> Result<PathBuf, AppletError> {
    if let Some(path) = &options.output_document {
        return Ok(PathBuf::from(path));
    }

    let uri = url
        .parse::<ureq::http::Uri>()
        .map_err(|err| AppletError::new(APPLET, format!("invalid URL '{url}': {err}")))?;
    let filename = default_filename(&uri);
    if let Some(dir) = &options.output_dir {
        Ok(Path::new(dir).join(filename))
    } else {
        Ok(PathBuf::from(filename))
    }
}

fn default_filename(uri: &ureq::http::Uri) -> &str {
    let path = uri.path();
    if path.is_empty() || path == "/" {
        "index.html"
    } else {
        path.rsplit('/')
            .find(|segment| !segment.is_empty())
            .unwrap_or("index.html")
    }
}

fn path_for_error(path: &Path) -> &str {
    path.to_str().unwrap_or("<non-utf8 path>")
}

fn split_header(header: &str) -> Result<(&str, &str), AppletError> {
    let Some((name, value)) = header.split_once(':') else {
        return Err(AppletError::new(
            APPLET,
            format!("invalid header '{header}': expected 'Name: Value'"),
        ));
    };
    let name = name.trim();
    if name.is_empty() {
        return Err(AppletError::new(
            APPLET,
            format!("invalid header '{header}': empty header name"),
        ));
    }
    Ok((name, value.trim()))
}

#[cfg(test)]
mod tests {
    use super::{Options, ParsedCommand, default_filename, output_path, parse_args};
    use crate::common::unix;
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn parse_supports_quiet_output_document_and_directory() {
        let ParsedCommand::Fetch(options, urls) = parse_args(&[
            "-q".into(),
            "-O".into(),
            "out".into(),
            "-P".into(),
            "dir".into(),
            "http://example.com".into(),
        ])
        .expect("parse") else {
            panic!("expected fetch command");
        };

        assert!(options.quiet);
        assert_eq!(options.output_document.as_deref(), Some("out"));
        assert_eq!(options.output_dir.as_deref(), Some("dir"));
        assert_eq!(urls, vec![String::from("http://example.com")]);
    }

    #[test]
    fn parse_supports_post_data_and_headers() {
        let ParsedCommand::Fetch(options, urls) = parse_args(&args(&[
            "-q",
            "--post-data={\"arch\":\"x86_64\"}",
            "--header",
            "Authorization: Bearer test-token",
            "--header=Content-Type: application/json",
            "http://example.com",
        ]))
        .expect("parse") else {
            panic!("expected fetch command");
        };

        assert!(options.quiet);
        assert_eq!(options.post_data.as_deref(), Some("{\"arch\":\"x86_64\"}"));
        assert_eq!(
            options.headers,
            vec![
                String::from("Authorization: Bearer test-token"),
                String::from("Content-Type: application/json"),
            ]
        );
        assert_eq!(urls, vec![String::from("http://example.com")]);
    }

    #[test]
    fn parse_supports_post_file() {
        let ParsedCommand::Fetch(options, urls) = parse_args(&args(&[
            "--post-file=/tmp/request.json",
            "http://example.com",
        ]))
        .expect("parse") else {
            panic!("expected fetch command");
        };

        assert_eq!(options.post_file.as_deref(), Some("/tmp/request.json"));
        assert_eq!(options.post_data, None);
        assert_eq!(urls, vec![String::from("http://example.com")]);
    }

    #[test]
    fn parse_no_check_certificate_short() {
        let ParsedCommand::Fetch(options, _) =
            parse_args(&args(&["-k", "http://example.com"])).expect("parse")
        else {
            panic!("expected fetch command");
        };
        assert!(options.no_check_certificate);
    }

    #[test]
    fn parse_no_check_certificate_long() {
        let ParsedCommand::Fetch(options, _) =
            parse_args(&args(&["--no-check-certificate", "http://example.com"])).expect("parse")
        else {
            panic!("expected fetch command");
        };
        assert!(options.no_check_certificate);
    }

    #[test]
    fn parse_ca_certificate_separate_arg() {
        let ParsedCommand::Fetch(options, _) = parse_args(&args(&[
            "--ca-certificate",
            "/etc/ssl/certs/ca-certificates.crt",
            "http://example.com",
        ]))
        .expect("parse") else {
            panic!("expected fetch command");
        };
        assert_eq!(
            options.ca_certificate.as_deref(),
            Some("/etc/ssl/certs/ca-certificates.crt")
        );
    }

    #[test]
    fn parse_ca_certificate_equals_form() {
        let ParsedCommand::Fetch(options, _) = parse_args(&args(&[
            "--ca-certificate=/etc/ssl/certs/ca-certificates.crt",
            "http://example.com",
        ]))
        .expect("parse") else {
            panic!("expected fetch command");
        };
        assert_eq!(
            options.ca_certificate.as_deref(),
            Some("/etc/ssl/certs/ca-certificates.crt")
        );
    }

    #[test]
    fn empty_url_path_defaults_to_index_html() {
        let uri = "http://example.com"
            .parse::<ureq::http::Uri>()
            .expect("parse uri");
        assert_eq!(default_filename(&uri), "index.html");
    }

    #[test]
    fn output_document_overrides_output_directory() {
        let path = output_path(
            "http://example.com/",
            &Options {
                output_document: Some(String::from("index.html")),
                output_dir: Some(String::from("ignored")),
                ..Options::default()
            },
        )
        .expect("output path");

        assert_eq!(path, std::path::PathBuf::from("index.html"));
    }

    #[test]
    fn output_directory_uses_default_filename() {
        let path = output_path(
            "http://example.com/",
            &Options {
                output_dir: Some(String::from("foo")),
                ..Options::default()
            },
        )
        .expect("output path");

        assert_eq!(path, std::path::PathBuf::from("foo/index.html"));
    }

    #[test]
    fn wget_downloads_to_output_document() {
        let dir = unix::temp_dir("wget");
        let output = dir.join("foo");
        let server = spawn_test_server(b"hello wget");

        let status = super::main(&[
            "-q".into(),
            "-O".into(),
            output.display().to_string().into(),
            server.url.clone().into(),
        ]);
        assert_eq!(status, 0);
        assert_eq!(fs::read(&output).expect("read output"), b"hello wget");

        let _ = fs::remove_dir_all(dir);
        server.join();
    }

    #[test]
    fn wget_posts_data_with_headers() {
        let dir = unix::temp_dir("wget");
        let output = dir.join("foo");
        let payload = r#"{"arch":"x86_64","pkg":"seed"}"#;
        let server = spawn_inspecting_test_server(b"ok");

        let status = super::main(&args(&[
            "-q",
            "-O",
            output.to_str().expect("utf-8 path"),
            "--post-data",
            payload,
            "--header",
            "Authorization: Bearer test-token",
            "--header",
            "Content-Type: application/json",
            &server.url,
        ]));
        assert_eq!(status, 0);
        assert_eq!(fs::read(&output).expect("read output"), b"ok");

        let request = server.join_with_request();
        assert_eq!(request.request_line, "POST / HTTP/1.1");
        assert_eq!(request.header("authorization"), Some("Bearer test-token"));
        assert_eq!(request.header("content-type"), Some("application/json"));
        assert_eq!(request.body, payload.as_bytes());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn wget_posts_file_with_headers() {
        let dir = unix::temp_dir("wget");
        let output = dir.join("foo");
        let payload_path = dir.join("payload.json");
        let payload = br#"{"arch":"x86_64","pkg":"seed-from-file"}"#;
        fs::write(&payload_path, payload).expect("write payload");
        let server = spawn_inspecting_test_server(b"ok");

        let status = super::main(&args(&[
            "-q",
            "-O",
            output.to_str().expect("utf-8 path"),
            "--post-file",
            payload_path.to_str().expect("utf-8 path"),
            "--header",
            "Authorization: Bearer file-token",
            "--header",
            "Content-Type: application/json",
            &server.url,
        ]));
        assert_eq!(status, 0);
        assert_eq!(fs::read(&output).expect("read output"), b"ok");

        let request = server.join_with_request();
        assert_eq!(request.request_line, "POST / HTTP/1.1");
        assert_eq!(request.header("authorization"), Some("Bearer file-token"));
        assert_eq!(request.header("content-type"), Some("application/json"));
        assert_eq!(request.body, payload);

        let _ = fs::remove_dir_all(dir);
    }

    struct TestServer {
        url: String,
        handle: thread::JoinHandle<()>,
    }

    impl TestServer {
        fn join(self) {
            self.handle.join().expect("server thread");
        }
    }

    struct InspectingTestServer {
        url: String,
        handle: thread::JoinHandle<()>,
        request_rx: mpsc::Receiver<CapturedRequest>,
    }

    impl InspectingTestServer {
        fn join_with_request(self) -> CapturedRequest {
            let request = self.request_rx.recv().expect("captured request");
            self.handle.join().expect("server thread");
            request
        }
    }

    struct CapturedRequest {
        request_line: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    impl CapturedRequest {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers.get(name).map(String::as_str)
        }
    }

    fn spawn_test_server(body: &'static [u8]) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).expect("read request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("write headers");
            stream.write_all(body).expect("write body");
        });

        TestServer {
            url: format!("http://{addr}/"),
            handle,
        }
    }

    fn spawn_inspecting_test_server(body: &'static [u8]) -> InspectingTestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        let (request_tx, request_rx) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let request = read_request(&mut stream);
            request_tx.send(request).expect("send request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("write headers");
            stream.write_all(body).expect("write body");
        });

        InspectingTestServer {
            url: format!("http://{addr}/"),
            handle,
            request_rx,
        }
    }

    fn read_request(stream: &mut std::net::TcpStream) -> CapturedRequest {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut chunk).expect("read request");
            assert!(read > 0, "request ended before headers");
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(pos) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break pos;
            }
        };

        let headers_raw = std::str::from_utf8(&buffer[..header_end])
            .expect("utf-8 headers")
            .to_string();
        let content_length = headers_raw
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    Some(value.trim().parse::<usize>().expect("content length"))
                } else {
                    None
                }
            })
            .unwrap_or(0);
        let body_start = header_end + 4;

        while buffer.len() < body_start + content_length {
            let read = stream.read(&mut chunk).expect("read request body");
            assert!(read > 0, "request ended before body");
            buffer.extend_from_slice(&chunk[..read]);
        }

        let mut lines = headers_raw.lines();
        let request_line = lines.next().expect("request line").to_string();
        let headers = lines
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
            })
            .collect();

        CapturedRequest {
            request_line,
            headers,
            body: buffer[body_start..body_start + content_length].to_vec(),
        }
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_tls12_cert_expired_fails() {
        assert_ne!(
            super::main(&args(&["-q", "-O", "-", "https://tls12.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_tls13_cert_expired_fails() {
        assert_ne!(
            super::main(&args(&["-q", "-O", "-", "https://tls13.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_sha256_succeeds() {
        assert_eq!(
            super::main(&args(&["-q", "-O", "-", "https://sha256.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_ecc256_succeeds() {
        assert_eq!(
            super::main(&args(&["-q", "-O", "-", "https://ecc256.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_rsa2048_succeeds() {
        assert_eq!(
            super::main(&args(&["-q", "-O", "-", "https://rsa2048.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_expired_fails() {
        assert_ne!(
            super::main(&args(&["-q", "-O", "-", "https://expired.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_wrong_host_fails() {
        assert_ne!(
            super::main(&args(&["-q", "-O", "-", "https://wrong.host.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_self_signed_fails() {
        assert_ne!(
            super::main(&args(&["-q", "-O", "-", "https://self-signed.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_untrusted_root_fails() {
        assert_ne!(
            super::main(&args(&[
                "-q",
                "-O",
                "-",
                "https://untrusted-root.badssl.com/"
            ])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_expired_succeeds_with_no_check() {
        assert_eq!(
            super::main(&args(&["-kq", "-O", "-", "https://expired.badssl.com/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn target_kominka_succeeds() {
        assert_eq!(
            super::main(&args(&["-q", "-O", "-", "https://kominka.17166969.xyz/"])),
            0
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn target_r2_tls_connects() {
        let result = super::run(&args(&[
            "-q",
            "-O",
            "-",
            "https://pub-15b3a4c25627476493c0e1a68993f4d8.r2.dev/",
        ]));
        match result {
            Ok(()) => {}
            Err(errors) => {
                for e in &errors {
                    let msg = e.to_string();
                    assert!(
                        !msg.contains("certificate")
                            && !msg.contains("tls")
                            && !msg.contains("TLS"),
                        "unexpected TLS error: {msg}"
                    );
                }
            }
        }
    }
}
