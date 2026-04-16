#[cfg(feature = "multicall")]
pub fn short_help(applet: &str) -> String {
    if let Some(specific) = specific_short_help(applet) {
        return specific.to_string();
    }

    format!(
        "{applet} - seed applet\n\nusage: {applet} [ARGS...]\n\nTry 'man {applet}' for details.\n"
    )
}

#[cfg(feature = "multicall")]
fn specific_short_help(applet: &str) -> Option<&'static str> {
    match applet {
        "tar" => Some(crate::applets::tar::short_help()),
        #[cfg(feature = "wget")]
        "wget" => Some(crate::wget::short_help()),
        _ => None,
    }
}
