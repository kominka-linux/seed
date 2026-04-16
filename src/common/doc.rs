pub struct AppletDoc {
    pub name: &'static str,
    pub section: &'static str,
    pub summary: &'static str,
    pub usage: &'static [&'static str],
    pub description: &'static [&'static str],
    pub options: &'static [DocOption],
    pub examples: &'static [DocExample],
    pub see_also: &'static [&'static str],
}

pub struct DocOption {
    pub flags: &'static str,
    pub help: &'static str,
}

pub struct DocExample {
    pub command: &'static str,
    pub help: &'static str,
}

pub fn render_generic_manpage(applet: &str, source: &str, date: &str) -> String {
    let mut out = String::new();
    out.push_str(".TH \"");
    out.push_str(&roff(applet));
    out.push_str("\" \"1\" \"");
    out.push_str(&roff(date));
    out.push_str("\" \"");
    out.push_str(&roff(source));
    out.push_str("\"\n");

    section(&mut out, "NAME");
    out.push_str(&roff(applet));
    out.push_str(" \\- seed applet\n");

    section(&mut out, "SYNOPSIS");
    out.push_str(".B ");
    out.push_str(&roff(&format!("{applet} [ARGS...]")));
    out.push('\n');

    section(&mut out, "DESCRIPTION");
    out.push_str(&roff(&format!(
        "{applet} is a seed multicall applet. Detailed applet-specific documentation has not been authored yet."
    )));
    out.push('\n');
    out.push_str(".PP\n");
    out.push_str(&roff(&format!(
        "Use '{applet} --help' for a short usage summary."
    )));
    out.push('\n');

    out
}

pub fn render_manpage(doc: &AppletDoc, source: &str, date: &str) -> String {
    let mut out = String::new();
    out.push_str(".TH \"");
    out.push_str(&roff(doc.name));
    out.push_str("\" \"");
    out.push_str(&roff(doc.section));
    out.push_str("\" \"");
    out.push_str(&roff(date));
    out.push_str("\" \"");
    out.push_str(&roff(source));
    out.push_str("\"\n");

    section(&mut out, "NAME");
    out.push_str(&roff(doc.name));
    out.push_str(" \\- ");
    out.push_str(&roff(doc.summary));
    out.push('\n');

    section(&mut out, "SYNOPSIS");
    for usage in doc.usage {
        out.push_str(".B ");
        out.push_str(&roff(usage));
        out.push('\n');
    }

    if !doc.description.is_empty() {
        section(&mut out, "DESCRIPTION");
        for (idx, paragraph) in doc.description.iter().enumerate() {
            if idx > 0 {
                out.push_str(".PP\n");
            }
            out.push_str(&roff(paragraph));
            out.push('\n');
        }
    }

    if !doc.options.is_empty() {
        section(&mut out, "OPTIONS");
        for option in doc.options {
            out.push_str(".TP\n");
            out.push_str(".B ");
            out.push_str(&roff(option.flags));
            out.push('\n');
            out.push_str(&roff(option.help));
            out.push('\n');
        }
    }

    if !doc.examples.is_empty() {
        section(&mut out, "EXAMPLES");
        for example in doc.examples {
            out.push_str(".TP\n");
            out.push_str(".B ");
            out.push_str(&roff(example.command));
            out.push('\n');
            out.push_str(&roff(example.help));
            out.push('\n');
        }
    }

    if !doc.see_also.is_empty() {
        section(&mut out, "SEE ALSO");
        out.push_str(&roff(&doc.see_also.join(", ")));
        out.push('\n');
    }

    out
}

fn section(out: &mut String, name: &str) {
    out.push_str(".SH ");
    out.push_str(&roff(name));
    out.push('\n');
}

fn roff(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('-', "\\-")
        .replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{AppletDoc, DocExample, DocOption, render_generic_manpage, render_manpage};

    const DOC: AppletDoc = AppletDoc {
        name: "demo",
        section: "1",
        summary: "demo summary",
        usage: &["demo [OPTIONS] FILE"],
        description: &["The demo applet demonstrates shared docs."],
        options: &[DocOption {
            flags: "--flag VALUE",
            help: "Use VALUE for the demo flag.",
        }],
        examples: &[DocExample {
            command: "demo --flag x file",
            help: "Run the demo applet.",
        }],
        see_also: &["man(1)"],
    };

    #[test]
    fn manpage_renders_expected_sections() {
        let page = render_manpage(&DOC, "seed", "");
        assert!(page.contains(".SH NAME"));
        assert!(page.contains(".SH SYNOPSIS"));
        assert!(page.contains(".SH OPTIONS"));
        assert!(page.contains(".SH EXAMPLES"));
    }

    #[test]
    fn generic_manpage_renders_synopsis() {
        let page = render_generic_manpage("echo", "seed", "");
        assert!(page.contains(".SH SYNOPSIS"));
        assert!(page.contains("echo [ARGS...]"));
    }
}
