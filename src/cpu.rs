use std::fs;
use std::time::Duration;

pub async fn read_cpu_stats() -> f64 {
    let (total1, idle1) = read_cpu_usage();
    // Dajemy czas dla CPU niech się napracuje przez sek :)
    tokio::time::sleep(Duration::from_secs(1)).await;
    // Sprawdzamy ile się napracował :)
    let (total2, idle2) = read_cpu_usage();

    let total_diff = total2 - total1;
    let idle_diff = idle2 - idle1;

    100f64 * (total_diff - idle_diff) as f64 / total_diff as f64
}

// Czytamy zawartośc /proc/stat i parsujemy na nasze potrzeby
fn read_cpu_usage() -> (u64, u64) {
    let content = fs::read_to_string("/proc/stat").unwrap();
    let line = content.lines().next().unwrap();
    let values: Vec<&str> = line.split_whitespace().collect();
    let user: u64 = values[1].parse().unwrap();
    let nice: u64 = values[2].parse().unwrap();
    let system: u64 = values[3].parse().unwrap();
    let idle: u64 = values[4].parse().unwrap();
    let total = user + nice + system + idle;
    (total, idle)
}
