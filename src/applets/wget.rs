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

#[derive(Debug, Default)]
struct Options {
    quiet: bool,
    output_document: Option<String>,
    output_dir: Option<String>,
    no_check_certificate: bool,
    ca_certificate: Option<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (options, urls) = parse_args(args)?;
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

fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options::default();
    let mut urls = Vec::new();
    let mut cursor = ArgCursor::new(args);

    while let Some(arg) = cursor.next_token() {
        if !cursor.parsing_flags() || !arg.starts_with('-') || arg.len() == 1 {
            urls.push(arg.to_owned());
            continue;
        }

        if let Some(val) = arg.strip_prefix("--ca-certificate=") {
            options.ca_certificate = Some(val.to_owned());
            continue;
        }

        match arg {
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
            "--no-check-certificate" => {
                options.no_check_certificate = true;
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

    Ok((options, urls))
}

fn make_agent(options: &Options) -> Result<Agent, AppletError> {
    let ssl_cert_file = std::env::var("SSL_CERT_FILE").ok();
    let ca_file = options
        .ca_certificate
        .as_deref()
        .or_else(|| ssl_cert_file.as_deref());

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
    let pem = std::fs::read(path)
        .map_err(|e| AppletError::from_io(APPLET, "reading", Some(path), e))?;
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
    let mut response = agent
        .get(url)
        .call()
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

#[cfg(test)]
mod tests {
    use super::{Options, default_filename, output_path, parse_args};
    use crate::common::unix;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_supports_quiet_output_document_and_directory() {
        let (options, urls) = parse_args(&[
            "-q".to_string(),
            "-O".to_string(),
            "out".to_string(),
            "-P".to_string(),
            "dir".to_string(),
            "http://example.com".to_string(),
        ])
        .expect("parse");

        assert!(options.quiet);
        assert_eq!(options.output_document.as_deref(), Some("out"));
        assert_eq!(options.output_dir.as_deref(), Some("dir"));
        assert_eq!(urls, vec![String::from("http://example.com")]);
    }

    #[test]
    fn parse_no_check_certificate_short() {
        let (options, _) =
            parse_args(&args(&["-k", "http://example.com"])).expect("parse");
        assert!(options.no_check_certificate);
    }

    #[test]
    fn parse_no_check_certificate_long() {
        let (options, _) =
            parse_args(&args(&["--no-check-certificate", "http://example.com"])).expect("parse");
        assert!(options.no_check_certificate);
    }

    #[test]
    fn parse_ca_certificate_separate_arg() {
        let (options, _) =
            parse_args(&args(&["--ca-certificate", "/etc/ssl/certs/ca-certificates.crt", "http://example.com"]))
                .expect("parse");
        assert_eq!(
            options.ca_certificate.as_deref(),
            Some("/etc/ssl/certs/ca-certificates.crt")
        );
    }

    #[test]
    fn parse_ca_certificate_equals_form() {
        let (options, _) =
            parse_args(&args(&["--ca-certificate=/etc/ssl/certs/ca-certificates.crt", "http://example.com"]))
                .expect("parse");
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
            "-q".to_string(),
            "-O".to_string(),
            output.display().to_string(),
            server.url.clone(),
        ]);
        assert_eq!(status, 0);
        assert_eq!(fs::read(&output).expect("read output"), b"hello wget");

        let _ = fs::remove_dir_all(dir);
        server.join();
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

    // ── badssl.com network tests ────────────────────────────────────────────
    // Run with: cargo test -- --ignored

    #[test]
    #[ignore = "requires network access"]
    fn badssl_tls12_cert_expired_fails() {
        // tls12.badssl.com has a certificate that expired in 2018 and was never renewed.
        assert_ne!(super::main(&args(&["-q", "-O", "-", "https://tls12.badssl.com/"])), 0);
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_tls13_cert_expired_fails() {
        // tls13.badssl.com has a certificate that expired in 2018 and was never renewed.
        assert_ne!(super::main(&args(&["-q", "-O", "-", "https://tls13.badssl.com/"])), 0);
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_sha256_succeeds() {
        assert_eq!(super::main(&args(&["-q", "-O", "-", "https://sha256.badssl.com/"])), 0);
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_ecc256_succeeds() {
        assert_eq!(super::main(&args(&["-q", "-O", "-", "https://ecc256.badssl.com/"])), 0);
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_rsa2048_succeeds() {
        assert_eq!(super::main(&args(&["-q", "-O", "-", "https://rsa2048.badssl.com/"])), 0);
    }

    #[test]
    #[ignore = "requires network access"]
    fn badssl_expired_fails() {
        assert_ne!(super::main(&args(&["-q", "-O", "-", "https://expired.badssl.com/"])), 0);
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
            super::main(&args(&["-q", "-O", "-", "https://untrusted-root.badssl.com/"])),
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
        // The R2 bucket root returns 404, but the TLS handshake and connection succeed.
        // wget exits non-zero because of the HTTP error, not a TLS failure.
        // We verify the error message doesn't mention a TLS/cert problem.
        let result = super::run(&args(&[
            "-q",
            "-O",
            "-",
            "https://pub-15b3a4c25627476493c0e1a68993f4d8.r2.dev/",
        ]));
        match result {
            Ok(()) => {} // if it somehow succeeds, fine
            Err(errors) => {
                for e in &errors {
                    let msg = e.to_string();
                    assert!(
                        !msg.contains("certificate") && !msg.contains("tls") && !msg.contains("TLS"),
                        "unexpected TLS error: {msg}"
                    );
                }
            }
        }
    }
}
