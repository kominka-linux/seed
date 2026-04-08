use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use crate::common::applet::AppletResult;
use crate::common::error::AppletError;
use crate::common::io::{BUFFER_SIZE, Input, open_input, stdout};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Options {
    pub(crate) decompress: bool,
    pub(crate) stdout: bool,
    pub(crate) force: bool,
    pub(crate) keep: bool,
    pub(crate) test: bool,
    pub(crate) level: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            decompress: false,
            stdout: false,
            force: false,
            keep: false,
            test: false,
            level: 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Invocation {
    pub(crate) decompress: bool,
    pub(crate) stdout: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LevelRange {
    ZeroToNine,
    OneToNine,
}

pub(crate) type ProcessReaderToWriter =
    fn(Input, &mut dyn Write, Options, Option<&str>, Option<u64>) -> Result<(), AppletError>;

pub(crate) struct Codec<'a> {
    pub(crate) applet: &'static str,
    pub(crate) level_range: LevelRange,
    pub(crate) compressed_suffix: &'a [u8],
    pub(crate) decompressed_suffixes: &'a [(&'a [u8], &'a [u8])],
    pub(crate) input_size_hint: fn(&str) -> Option<u64>,
    pub(crate) process_reader_to_writer: ProcessReaderToWriter,
}

impl LevelRange {
    fn parse_digit(self, flag: char) -> Option<u32> {
        match self {
            Self::ZeroToNine if flag.is_ascii_digit() => flag.to_digit(10),
            Self::OneToNine if matches!(flag, '1'..='9') => flag.to_digit(10),
            _ => None,
        }
    }
}

pub(crate) fn run<F>(
    applet: &'static str,
    args: &[String],
    invocation: Invocation,
    levels: LevelRange,
    mut process_target: F,
) -> AppletResult
where
    F: FnMut(&str, Options) -> Result<(), AppletError>,
{
    let (options, files) = parse_args(applet, args, invocation, levels)?;
    let targets = if files.is_empty() {
        vec![String::from("-")]
    } else {
        files
    };

    let mut errors = Vec::new();
    for path in &targets {
        if let Err(err) = process_target(path, options) {
            errors.push(err);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub(crate) fn run_with_codec(
    codec: &Codec<'_>,
    args: &[String],
    invocation: Invocation,
) -> AppletResult {
    run(
        codec.applet,
        args,
        invocation,
        codec.level_range,
        |path, options| process_target_with_codec(codec, path, options),
    )
}

pub(crate) fn parse_args(
    applet: &'static str,
    args: &[String],
    invocation: Invocation,
    levels: LevelRange,
) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    let mut options = Options {
        decompress: invocation.decompress,
        stdout: invocation.stdout,
        ..Options::default()
    };
    let mut files = Vec::new();
    let mut parsing_flags = true;

    for arg in args {
        if parsing_flags && arg == "--" {
            parsing_flags = false;
            continue;
        }

        if parsing_flags && arg.starts_with('-') && arg.len() > 1 {
            for flag in arg[1..].chars() {
                if let Some(level) = levels.parse_digit(flag) {
                    options.level = level;
                    continue;
                }

                match flag {
                    'c' => options.stdout = true,
                    'd' => options.decompress = true,
                    'f' => options.force = true,
                    'k' => options.keep = true,
                    't' => {
                        options.decompress = true;
                        options.test = true;
                    }
                    'z' => options.decompress = false,
                    _ => return Err(vec![AppletError::invalid_option(applet, flag)]),
                }
            }
            continue;
        }

        files.push(arg.clone());
    }

    Ok((options, files))
}

#[cfg(test)]
pub(crate) fn parse_args_with_codec(
    codec: &Codec<'_>,
    args: &[String],
    invocation: Invocation,
) -> Result<(Options, Vec<String>), Vec<AppletError>> {
    parse_args(codec.applet, args, invocation, codec.level_range)
}

pub(crate) fn process_target<CompressPath, DecompressPath, InputSizeHint, Process>(
    applet: &'static str,
    path: &str,
    options: Options,
    compressed_path: CompressPath,
    decompressed_path: DecompressPath,
    input_size_hint: InputSizeHint,
    process: Process,
) -> Result<(), AppletError>
where
    CompressPath: Fn(&Path) -> PathBuf,
    DecompressPath: Fn(&Path) -> Result<PathBuf, AppletError>,
    InputSizeHint: Fn(&str) -> Option<u64>,
    Process:
        Fn(Input, &mut dyn Write, Options, Option<&str>, Option<u64>) -> Result<(), AppletError>,
{
    if path == "-" {
        let input =
            open_input("-").map_err(|err| AppletError::from_io(applet, "opening", None, err))?;
        return if options.test {
            process_to_sink(input, options, None, process)
        } else {
            let mut output = stdout();
            process(input, &mut output, options, None, None)
        };
    }

    if options.test {
        let input = open_input(path)
            .map_err(|err| AppletError::from_io(applet, "opening", Some(path), err))?;
        return process_to_sink(input, options, Some(path), process);
    }

    if options.stdout {
        let input = open_input(path)
            .map_err(|err| AppletError::from_io(applet, "opening", Some(path), err))?;
        let mut output = stdout();
        let size_hint = compression_input_size(path, options, &input_size_hint);
        return process(input, &mut output, options, Some(path), size_hint);
    }

    let source = Path::new(path);
    let destination = if options.decompress {
        decompressed_path(source)?
    } else {
        compressed_path(source)
    };

    if destination.exists() && !options.force {
        return Err(AppletError::new(
            applet,
            format!("not overwriting {}", destination.display()),
        ));
    }

    let input =
        open_input(path).map_err(|err| AppletError::from_io(applet, "opening", Some(path), err))?;
    let size_hint = compression_input_size(path, options, &input_size_hint);
    let (temp_path, temp_file) = create_temp_output(applet, &destination)?;
    let mut writer = BufWriter::with_capacity(BUFFER_SIZE, temp_file);

    if let Err(err) = process(input, &mut writer, options, Some(path), size_hint) {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    if options.force && destination.exists() {
        fs::remove_file(&destination).map_err(|err| {
            let _ = fs::remove_file(&temp_path);
            AppletError::from_io(applet, "removing", Some(path_for_error(&destination)), err)
        })?;
    }

    fs::rename(&temp_path, &destination).map_err(|err| {
        let _ = fs::remove_file(&temp_path);
        AppletError::from_io(applet, "renaming", Some(path_for_error(&destination)), err)
    })?;

    if !options.keep {
        fs::remove_file(source)
            .map_err(|err| AppletError::from_io(applet, "removing", Some(path), err))?;
    }

    Ok(())
}

pub(crate) fn process_target_with_codec(
    codec: &Codec<'_>,
    path: &str,
    options: Options,
) -> Result<(), AppletError> {
    process_target(
        codec.applet,
        path,
        options,
        |source| compressed_path_with_suffix(source, codec.compressed_suffix),
        |source| decompressed_path_with_suffixes(codec.applet, source, codec.decompressed_suffixes),
        codec.input_size_hint,
        codec.process_reader_to_writer,
    )
}

pub(crate) fn compressed_path_with_suffix(path: &Path, suffix: &[u8]) -> PathBuf {
    let mut name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .as_bytes()
        .to_vec();
    name.extend_from_slice(suffix);
    path.with_file_name(OsString::from_vec(name))
}

pub(crate) fn decompressed_path_with_suffixes(
    applet: &'static str,
    path: &Path,
    suffixes: &[(&[u8], &[u8])],
) -> Result<PathBuf, AppletError> {
    let name = path.file_name().unwrap_or(path.as_os_str()).as_bytes();
    let decoded = suffixes
        .iter()
        .find_map(|(suffix, replacement)| {
            name.ends_with(suffix).then(|| {
                let stem = &name[..name.len() - suffix.len()];
                [stem, *replacement].concat()
            })
        })
        .ok_or_else(|| AppletError::new(applet, format!("unknown suffix -- {}", path.display())))?;

    if decoded.is_empty() {
        return Err(AppletError::new(
            applet,
            format!("unknown suffix -- {}", path.display()),
        ));
    }

    Ok(path.with_file_name(OsString::from_vec(decoded)))
}

pub(crate) fn input_size_hint(path: &str) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

#[cfg(test)]
pub(crate) fn temp_dir(label: &str) -> PathBuf {
    crate::common::unix::temp_dir(label)
}

fn process_to_sink<Process>(
    input: Input,
    mut options: Options,
    path: Option<&str>,
    process: Process,
) -> Result<(), AppletError>
where
    Process:
        Fn(Input, &mut dyn Write, Options, Option<&str>, Option<u64>) -> Result<(), AppletError>,
{
    options.decompress = true;
    let mut sink = io::sink();
    process(input, &mut sink, options, path, None)
}

fn compression_input_size<InputSizeHint>(
    path: &str,
    options: Options,
    input_size_hint: &InputSizeHint,
) -> Option<u64>
where
    InputSizeHint: Fn(&str) -> Option<u64>,
{
    if options.decompress {
        None
    } else {
        input_size_hint(path)
    }
}

fn create_temp_output(
    applet: &'static str,
    destination: &Path,
) -> Result<(PathBuf, File), AppletError> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let base = destination
        .file_name()
        .unwrap_or(destination.as_os_str())
        .as_bytes()
        .to_vec();

    for attempt in 0..1024_u32 {
        let mut name = base.clone();
        name.extend_from_slice(format!(".seed-tmp-{}-{attempt}", std::process::id()).as_bytes());
        let candidate = parent.join(OsString::from_vec(name));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(AppletError::from_io(
                    applet,
                    "creating",
                    Some(path_for_error(&candidate)),
                    err,
                ));
            }
        }
    }

    Err(AppletError::new(
        applet,
        format!(
            "failed to create temporary output for {}",
            destination.display()
        ),
    ))
}

fn path_for_error(path: &Path) -> &str {
    path.to_str().unwrap_or("<non-utf8 path>")
}
