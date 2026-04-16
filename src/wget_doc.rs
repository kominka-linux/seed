use crate::common::doc::{AppletDoc, DocExample, DocOption};

pub fn doc() -> &'static AppletDoc {
    &DOC
}

const DOC: AppletDoc = AppletDoc {
    name: "wget",
    section: "1",
    summary: "retrieve files over HTTP and HTTPS",
    usage: &["wget [OPTIONS] URL..."],
    description: &[
        "wget fetches each URL and writes the response body to standard output or a file.",
        "POST requests can be sent with inline data via --post-data or by reading the request body from a file with --post-file.",
    ],
    options: &[
        DocOption {
            flags: "-q",
            help: "Suppress normal output.",
        },
        DocOption {
            flags: "-O FILE",
            help: "Write the response body to FILE instead of a derived filename.",
        },
        DocOption {
            flags: "-P DIR",
            help: "Write output files below DIR.",
        },
        DocOption {
            flags: "-k, --no-check-certificate",
            help: "Disable TLS certificate verification.",
        },
        DocOption {
            flags: "--ca-certificate FILE",
            help: "Load trusted root certificates from FILE.",
        },
        DocOption {
            flags: "--header 'Name: Value'",
            help: "Add an HTTP request header. May be repeated.",
        },
        DocOption {
            flags: "--post-data DATA",
            help: "Send DATA as the POST request body.",
        },
        DocOption {
            flags: "--post-file FILE",
            help: "Read the POST request body from FILE.",
        },
        DocOption {
            flags: "--help",
            help: "Show a short usage summary.",
        },
    ],
    examples: &[
        DocExample {
            command: "wget -q -O- https://example.com/",
            help: "Write the response body to standard output.",
        },
        DocExample {
            command: "wget --post-file payload.json --header 'Content-Type: application/json' https://example.com/api",
            help: "Send a JSON request body loaded from payload.json.",
        },
    ],
    see_also: &["man(1)"],
};

#[cfg(test)]
mod tests {
    use super::doc;

    #[test]
    fn wget_doc_mentions_post_file() {
        let doc = doc();
        assert_eq!(doc.name, "wget");
        assert!(
            doc.options
                .iter()
                .any(|option| option.flags == "--post-file FILE")
        );
    }
}
