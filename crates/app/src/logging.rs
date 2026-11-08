use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::nonpoison::Mutex;
use std::time::SystemTime;

use log::{Level, LevelFilter, Log, Metadata, Record};

pub const LOG_FILE_NAME: &str = "stream-tools.log";
const LOG_ENV: &str = "STREAM_TOOLS_LOG";
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;

struct Sink {
    file: Option<File>,
    path: PathBuf,
    bytes_written: u64,
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

struct Logger;

static LOGGER: Logger = Logger;

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(record);
        match record.level() {
            Level::Error | Level::Warn => eprintln!("{line}"),
            _ => println!("{line}"),
        }
        write_to_file(record.level(), &line);
    }

    fn flush(&self) {}
}

pub fn log_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(LOG_FILE_NAME)
}

fn sink() -> &'static Mutex<Sink> {
    SINK.get_or_init(|| {
        let path = log_path();
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| eprintln!("cannot open log file {}: {e}", path.display()))
            .ok();
        let bytes_written = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Mutex::new(Sink {
            file,
            path,
            bytes_written,
        })
    })
}

fn parse_level(value: &str) -> Option<LevelFilter> {
    match value.trim().to_ascii_lowercase().as_str() {
        "debug" | "trace" => Some(LevelFilter::Debug),
        "info" => Some(LevelFilter::Info),
        "warn" | "warning" => Some(LevelFilter::Warn),
        "error" => Some(LevelFilter::Error),
        "off" | "none" => Some(LevelFilter::Off),
        _ => None,
    }
}

pub fn init() {
    let default_level = if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    let level = std::env::var(LOG_ENV)
        .ok()
        .and_then(|value| parse_level(&value))
        .unwrap_or(default_level);

    if let Err(e) = log::set_logger(&LOGGER) {
        eprintln!("cannot install the logger: {e}");
        return;
    }
    log::set_max_level(level);

    log::info!("log file: {}", log_path().display());
    log::debug!("log level: {level}");
}

fn write_to_file(level: Level, line: &str) {
    let mut sink = sink().lock();
    let payload_len = line.len() as u64 + 1;
    if sink.bytes_written + payload_len > MAX_LOG_BYTES {
        rotate(&mut sink);
    }
    let Some(file) = sink.file.as_mut() else {
        return;
    };
    if file.write_all(line.as_bytes()).is_ok()
        && file.write_all(b"\n").is_ok()
        && file.flush().is_ok()
    {
        sink.bytes_written += payload_len;
    } else if level >= Level::Warn {
        eprintln!("cannot write to log file {}", sink.path.display());
    }
}

fn rotate(sink: &mut Sink) {
    let backup = sink.path.with_extension("log.1");
    let reopened = std::fs::rename(&sink.path, &backup).ok().and_then(|_| {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sink.path)
            .map_err(|e| eprintln!("cannot reopen log file: {e}"))
            .ok()
    });
    if let Some(fresh) = reopened {
        sink.file = Some(fresh);
        sink.bytes_written = 0;
    }
}

fn format_line(record: &Record<'_>) -> String {
    let since_epoch = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let head = record.target().split("::").next().unwrap_or_default();
    let scope = match head {
        "music" => "music: ",
        "chat" => "chat: ",
        _ => "",
    };
    format!(
        "{} {:5} {scope}{}",
        format_unix_timestamp(since_epoch.as_secs(), since_epoch.subsec_millis()),
        level_name(record.level()),
        record.args()
    )
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}

fn format_unix_timestamp(secs: u64, millis: u32) -> String {
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (hour, min, sec) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{millis:03}Z")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
