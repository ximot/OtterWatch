use crate::pressure::{PressureAverages, PressureEntry, PressureSnapshot};
use crate::{OperationTimings, ProgramUsageStats, SystemStats};
use std::fmt::Write as _;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    cursor,
    terminal::{self, ClearType},
    ExecutableCommand,
};
use tokio::sync::RwLock;
use tokio::time::{self, Duration as TokioDuration};

const REFRESH_INTERVAL_SECS: u64 = 1;
pub fn spawn_console_view(
    stats_handle: Arc<RwLock<SystemStats>>,
    timings_handle: Arc<RwLock<OperationTimings>>,
    program_handle: Arc<RwLock<ProgramUsageStats>>,
    pressure_handle: Arc<RwLock<PressureSnapshot>>,
) {
    tokio::spawn(async move {
        let mut interval = time::interval(TokioDuration::from_secs(REFRESH_INTERVAL_SECS));
        loop {
            interval.tick().await;

            let stats = stats_handle.read().await.clone();
            let timings = timings_handle.read().await.clone();
            let program = program_handle.read().await.clone();
            let pressure = pressure_handle.read().await.clone();

            let mut buffer = String::new();
            let _ = writeln!(buffer, "OtterWatch Console Monitor (--gui)");
            let _ = writeln!(buffer, "Press Ctrl+C to exit\n");

            let _ = writeln!(buffer, "System stats:");
            let _ = writeln!(buffer, "  CPU usage : {:>6.2}%", stats.cpu_usage);
            let _ = writeln!(
                buffer,
                "  Memory    : {} used / {} total (available: {})",
                format_bytes_from_kib(stats.used_memory),
                format_bytes_from_kib(stats.total_memory),
                format_bytes_from_kib(stats.available_memory)
            );
            let _ = writeln!(
                buffer,
                "  Swap      : {} used / {} total",
                format_bytes_from_kib(stats.swap_usage),
                format_bytes_from_kib(stats.swap_total)
            );
            buffer.push_str("\n");

            let _ = writeln!(buffer, "Program stats:");
            let _ = writeln!(buffer, "  CPU usage : {:>6.2}%", program.cpu_percent);
            let _ = writeln!(
                buffer,
                "  Memory    : {}",
                format_bytes(program.memory_bytes)
            );
            buffer.push_str("\n");

            let _ = writeln!(buffer, "PSI (avg10/avg60/avg300 %):");
            let _ = writeln!(
                buffer,
                "  {}",
                format_pressure_line("CPU", pressure.cpu.as_ref())
            );
            let _ = writeln!(
                buffer,
                "  {}",
                format_pressure_line("Mem", pressure.memory.as_ref())
            );
            let _ = writeln!(
                buffer,
                "  {}",
                format_pressure_line("I/O", pressure.io.as_ref())
            );
            buffer.push_str("\n");

            let _ = writeln!(buffer, "Read timings (ms):");
            let _ = writeln!(buffer, "  CPU    : {}", format_duration(timings.cpu_read));
            let _ = writeln!(
                buffer,
                "  Memory : {}",
                format_duration(timings.memory_read)
            );
            let _ = writeln!(buffer, "  Disk   : {}", format_duration(timings.disk_read));
            let _ = writeln!(
                buffer,
                "  Network: {}",
                format_duration(timings.network_read)
            );
            let _ = writeln!(buffer, "  DB save: {}", format_duration(timings.db_write));
            let _ = writeln!(buffer, "  Cycle  : {}", format_duration(timings.cycle));

            refresh_terminal(&buffer);
        }
    });
}

fn refresh_terminal(buffer: &str) {
    const ANSI_CLEAR_AND_HOME: &str = "\u{001b}[2J\u{001b}[H";

    let mut stdout = io::stdout();
    let result = stdout
        .execute(terminal::Clear(ClearType::All))
        .and_then(|s| s.execute(cursor::MoveTo(0, 0)));

    if result.is_err() {
        let _ = stdout.write_all(ANSI_CLEAR_AND_HOME.as_bytes());
        let _ = stdout.flush();
    }

    let _ = stdout.write_all(buffer.as_bytes());
    let _ = stdout.flush();
}

fn format_duration(duration: Option<Duration>) -> String {
    duration
        .map(|d| format!("{:>7.2}", d.as_secs_f64() * 1_000.0))
        .unwrap_or_else(|| "    --".to_string())
}

fn format_pressure_line(label: &str, entry: Option<&PressureEntry>) -> String {
    let some = entry.and_then(|e| e.some.as_ref());
    let full = entry.and_then(|e| e.full.as_ref());

    format!(
        "{:<4} some: {:<24} full: {:<24}",
        label,
        format_pressure_level(some),
        format_pressure_level(full)
    )
}

fn format_pressure_level(data: Option<&PressureAverages>) -> String {
    data.map(|d| {
        format!(
            "{:.2}/{:.2}/{:.2} (t {:>6})",
            d.avg10, d.avg60, d.avg300, d.total
        )
    })
    .unwrap_or_else(|| "--".to_string())
}

fn format_bytes_from_kib(value_kib: u64) -> String {
    format_bytes(value_kib.saturating_mul(1024))
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".to_string();
    }

    let mut val = bytes as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }

    format!("{:.2} {}", val, UNITS[unit])
}
