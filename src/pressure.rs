use log::warn;
use std::fs;

#[derive(Clone, Default)]
pub(crate) struct PressureAverages {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
    pub total: u64,
}

#[derive(Clone, Default)]
pub(crate) struct PressureEntry {
    pub some: Option<PressureAverages>,
    pub full: Option<PressureAverages>,
}

#[derive(Clone, Default)]
pub(crate) struct PressureSnapshot {
    pub cpu: Option<PressureEntry>,
    pub memory: Option<PressureEntry>,
    pub io: Option<PressureEntry>,
}

pub(crate) fn read_pressure_snapshot() -> PressureSnapshot {
    PressureSnapshot {
        cpu: read_pressure_file("cpu"),
        memory: read_pressure_file("memory"),
        io: read_pressure_file("io"),
    }
}

fn read_pressure_file(name: &str) -> Option<PressureEntry> {
    let path = format!("/proc/pressure/{}", name);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to read {}: {}", path, err);
            }
            return None;
        }
    };

    let mut entry = PressureEntry::default();
    for line in content.lines() {
        if let Some((level, averages)) = parse_pressure_line(line) {
            match level.as_str() {
                "some" => entry.some = Some(averages),
                "full" => entry.full = Some(averages),
                _ => {}
            }
        }
    }

    if entry.some.is_some() || entry.full.is_some() {
        Some(entry)
    } else {
        None
    }
}

fn parse_pressure_line(line: &str) -> Option<(String, PressureAverages)> {
    let mut tokens = line.split_whitespace();
    let level = tokens.next()?.to_string();

    let mut averages = PressureAverages::default();
    for token in tokens {
        if let Some(value) = token.strip_prefix("avg10=") {
            averages.avg10 = value.parse().unwrap_or(0.0);
        } else if let Some(value) = token.strip_prefix("avg60=") {
            averages.avg60 = value.parse().unwrap_or(0.0);
        } else if let Some(value) = token.strip_prefix("avg300=") {
            averages.avg300 = value.parse().unwrap_or(0.0);
        } else if let Some(value) = token.strip_prefix("total=") {
            averages.total = value.parse().unwrap_or(0);
        }
    }

    Some((level, averages))
}
