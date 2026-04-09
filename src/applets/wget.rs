use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig};

use crate::common::applet::{AppletResult, finish};
use crate::common::error::AppletError;
use crate::common::io::stdout;

const APPLET: &str = "wget";

#[derive(Debug, Default)]
struct Options {
    quiet: bool,
    output_document: Option<String>,
    output_dir: Option<String>,
}

pub fn main(args: &[String]) -> i32 {
    finish(run(args))
}

fn run(args: &[String]) -> AppletResult {
    let (options, urls) = parse_args(args)?;
    let agent = make_agent();
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
    let mut parsing_flags = true;
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            index += 1;
            continue;
        }

        if parsing_flags && arg == "-O" {
            let Some(value) = args.get(index + 1) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "O")]);
            };
            options.output_document = Some(value.clone());
            index += 2;
            continue;
        }

        if parsing_flags && arg == "-P" {
            let Some(value) = args.get(index + 1) else {
                return Err(vec![AppletError::option_requires_arg(APPLET, "P")]);
            };
            options.output_dir = Some(value.clone());
            index += 2;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                match flag {
                    'q' => options.quiet = true,
                    _ => return Err(vec![AppletError::invalid_option(APPLET, flag)]),
                }
            }
            index += 1;
            continue;
        }

        urls.push(arg.clone());
        index += 1;
    }

    if urls.is_empty() {
        return Err(vec![AppletError::new(APPLET, "missing URL")]);
    }

    Ok((options, urls))
}

fn make_agent() -> Agent {
    Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .unversioned_rustls_crypto_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .build(),
        )
        .build()
        .new_agent()
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
}
