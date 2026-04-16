use crate::common::doc::{AppletDoc, DocExample, DocOption};

pub fn doc() -> &'static AppletDoc {
    &DOC
}

const DOC: AppletDoc = AppletDoc {
    name: "tar",
    section: "1",
    summary: "create, extract, and list tar archives",
    usage: &[
        "tar -c[fjJzOvk] ARCHIVE [FILE...]",
        "tar -x[fjJzOvkO] ARCHIVE [MEMBER...]",
        "tar -t[fjJzv] ARCHIVE [MEMBER...]",
        "tar --help",
    ],
    description: &[
        "tar supports create, extract, and list modes with traditional short options, including old-style forms such as 'tar xf archive.tar'.",
        "Extraction supports GNU-compatible --strip-components and --overwrite behavior for common packaging and build-script workflows.",
    ],
    options: &[
        DocOption {
            flags: "-c",
            help: "Create an archive.",
        },
        DocOption {
            flags: "-x",
            help: "Extract an archive.",
        },
        DocOption {
            flags: "-t",
            help: "List archive contents.",
        },
        DocOption {
            flags: "-f FILE",
            help: "Use FILE as the archive path. Use '-' for standard input or output.",
        },
        DocOption {
            flags: "-C DIR",
            help: "Change to DIR before processing following file operands.",
        },
        DocOption {
            flags: "-v",
            help: "Print member names as they are processed.",
        },
        DocOption {
            flags: "-O",
            help: "Write extracted file contents to standard output.",
        },
        DocOption {
            flags: "-k",
            help: "Keep existing files when extracting.",
        },
        DocOption {
            flags: "-z, -j, -J",
            help: "Use gzip, bzip2, or xz compression.",
        },
        DocOption {
            flags: "-X FILE",
            help: "Read exclude patterns from FILE.",
        },
        DocOption {
            flags: "--overwrite",
            help: "Overwrite existing files during extraction.",
        },
        DocOption {
            flags: "--strip-components N",
            help: "Strip N leading path components when extracting.",
        },
        DocOption {
            flags: "--help",
            help: "Show a short usage summary.",
        },
    ],
    examples: &[
        DocExample {
            command: "tar czf archive.tar.gz dir",
            help: "Create a gzip-compressed archive from dir.",
        },
        DocExample {
            command: "tar xf archive.tar --strip-components=1",
            help: "Extract an archive while dropping the first path component from each member.",
        },
    ],
    see_also: &["gzip(1)", "bzip2(1)", "xz(1)"],
};

#[cfg(test)]
mod tests {
    use super::doc;

    #[test]
    fn tar_doc_mentions_strip_components() {
        let doc = doc();
        assert_eq!(doc.name, "tar");
        assert!(
            doc.options
                .iter()
                .any(|option| option.flags == "--strip-components N")
        );
    }
}
