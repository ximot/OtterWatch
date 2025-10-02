mod app_config;
mod console_ui;
mod cpu;
mod db;
mod disk_io;
mod memory;
mod network;
mod osinfo;
mod pressure;
mod process;

use crate::osinfo::get_os_info_api;
use crate::pressure::PressureSnapshot;
use actix_cors::Cors;
use actix_web::{web, App, HttpResponse, HttpServer};
use log::{error, warn};
use once_cell::sync::Lazy;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time;

type DbPool = Pool<SqliteConnectionManager>;

#[derive(Debug, Deserialize)]
struct Settings {
    interval_secs: u64,
    listen_addr: String,
    db_file_name: String,
    db_save: bool,
    db_history_days: u64,
    exclude_interfaces: String,
}

#[derive(Serialize, Clone)]
pub(crate) struct SystemStats {
    cpu_usage: f64,
    used_memory: u64,
    available_memory: u64,
    total_memory: u64,
    swap_usage: u64,
    swap_total: u64,
}

impl SystemStats {
    fn new() -> Self {
        Self {
            cpu_usage: 0f64,
            total_memory: 0,
            used_memory: 0,
            available_memory: 0,
            swap_usage: 0,
            swap_total: 0,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct OperationTimings {
    pub(crate) cpu_read: Option<Duration>,
    pub(crate) memory_read: Option<Duration>,
    pub(crate) disk_read: Option<Duration>,
    pub(crate) network_read: Option<Duration>,
    pub(crate) db_write: Option<Duration>,
    pub(crate) cycle: Option<Duration>,
}

#[derive(Clone, Default)]
pub(crate) struct ProgramUsageStats {
    pub(crate) cpu_percent: f64,
    pub(crate) memory_bytes: u64,
}

async fn clean_history_data_in_db(days_old: u64, pool: Arc<DbPool>) {
    const TIMER_TRIGGER: u64 = 86400;
    let mut interval = time::interval(Duration::from_secs(TIMER_TRIGGER));

    loop {
        interval.tick().await;
        let pool = Arc::clone(&pool);
        let remove_result = tokio::task::spawn_blocking(move || -> Result<(), DbTaskError> {
            let conn = pool.get().map_err(DbTaskError::Pool)?;
            let stmt = "DELETE FROM stats WHERE timestamp < datetime('now', ?1)";
            conn.execute(stmt, rusqlite::params![format!("-{} days", days_old)])
                .map(|_| ())
                .map_err(DbTaskError::Sqlite)
        })
        .await;

        match remove_result {
            Ok(Ok(())) => {
                println!("Historic data has been truncated! (over {} days)", days_old);
            }
            Ok(Err(db_err)) => {
                error!("Failed to truncate history data: {}", db_err);
            }
            Err(join_err) => {
                error!("Failed to execute history cleanup task: {}", join_err);
            }
        }
    }
}

async fn collect_and_save_stats(
    interval_secs: u64,
    db_save: bool,
    excluded_interfaces: Arc<Vec<String>>,
    pool: Arc<DbPool>,
) {
    let mut interval = time::interval(Duration::from_secs(interval_secs));
    let ticks_per_second = process::clock_ticks_per_second();
    let page_size = process::page_size_bytes();
    let mut prev_process_ticks: Option<u64> = None;

    loop {
        interval.tick().await;

        let loop_start = Instant::now();

        let cpu_measure_start = Instant::now();
        let cpu_usage = cpu::read_cpu_stats().await;
        let cpu_duration = cpu_measure_start.elapsed();

        let memory_measure_start = Instant::now();
        let (mem_total, mem_free, mem_avail, swap_total, swap_free) = memory::read_memory_info();
        let memory_duration = memory_measure_start.elapsed();

        let mut disk_duration = None;
        let mut network_duration = None;
        let mut db_write_duration = None;

        let mem_used = mem_total.saturating_sub(mem_free);
        {
            let mut data = GLOBAL_DATA.write().await;
            data.cpu_usage = cpu_usage.0;
            data.used_memory = mem_used;
            data.total_memory = mem_total;
            data.swap_usage = swap_total.saturating_sub(swap_free);
            data.swap_total = swap_total;
            data.available_memory = mem_avail;
        }

        if db_save {
            let disk_start = Instant::now();
            let disk_info = disk_io::get_physical_disk_io_stats();
            disk_duration = Some(disk_start.elapsed());

            let network_start = Instant::now();
            let network_info = network::get_network_io_stats(excluded_interfaces.as_ref());
            network_duration = Some(network_start.elapsed());
            let pool = Arc::clone(&pool);
            let db_start = Instant::now();
            let db_task = tokio::task::spawn_blocking(move || -> Result<(), DbTaskError> {
                let conn = pool.get().map_err(DbTaskError::Pool)?;
                persist_stats(
                    &conn,
                    cpu_usage,
                    mem_used,
                    mem_avail,
                    mem_total,
                    swap_free,
                    swap_total,
                    disk_info,
                    network_info,
                )
                .map_err(DbTaskError::Sqlite)
            })
            .await;
            db_write_duration = Some(db_start.elapsed());

            match db_task {
                Ok(Ok(())) => {}
                Ok(Err(err)) => error!("Failed to persist system stats: {}", err),
                Err(join_err) => error!("DB writer task panicked: {}", join_err),
            }
        }

        let cycle_duration = loop_start.elapsed();

        {
            let mut timings = GLOBAL_TIMINGS.write().await;
            timings.cpu_read = Some(cpu_duration);
            timings.memory_read = Some(memory_duration);
            timings.disk_read = disk_duration;
            timings.network_read = network_duration;
            timings.db_write = db_write_duration;
            timings.cycle = Some(cycle_duration);
        }

        if let Ok(snapshot) = process::read_self_snapshot() {
            let cpu_percent = if let Some(prev_ticks) = prev_process_ticks {
                if cycle_duration.as_secs_f64() > 0.0 && ticks_per_second > 0 {
                    let delta_ticks = snapshot.total_ticks.saturating_sub(prev_ticks);
                    (delta_ticks as f64 / ticks_per_second as f64) / cycle_duration.as_secs_f64()
                        * 100.0
                } else {
                    0.0
                }
            } else {
                0.0
            };
            prev_process_ticks = Some(snapshot.total_ticks);
            let rss_bytes = snapshot.rss_pages.saturating_mul(page_size);

            let mut program_stats = GLOBAL_PROGRAM_STATS.write().await;
            program_stats.cpu_percent = cpu_percent;
            program_stats.memory_bytes = rss_bytes;
        }

        let pressure_snapshot = pressure::read_pressure_snapshot();
        {
            let mut pressure_data = GLOBAL_PRESSURE.write().await;
            *pressure_data = pressure_snapshot;
        }

        if let Err(err) = memory::read_process_swap_usage_with_names() {
            warn!("Failed to read process swap usage: {}", err);
        }
    }
}

async fn system_stats() -> HttpResponse {
    let stats = format!("{{ \"My CPU Usage\" : \"{:.2}\",\n\"Ram Usage\" : \"{:}\",\n\"Ram Total\" : \"{:}\",\n\"Ram Available\" : \"{:}\",\n\"Swap Usage\": \"{:}\",\n\"Swap Total\": \"{:}\" }}",
                            GLOBAL_DATA.read().await.cpu_usage,
                            GLOBAL_DATA.read().await.used_memory,
                            GLOBAL_DATA.read().await.total_memory,
                            GLOBAL_DATA.read().await.available_memory,
                            GLOBAL_DATA.read().await.swap_usage,
                            GLOBAL_DATA.read().await.swap_total);
    HttpResponse::Ok().content_type("text/plain").body(stats)
}

async fn system_info() -> HttpResponse {
    match get_os_info_api() {
        Ok(body) => HttpResponse::Ok()
            .content_type("application/json")
            .body(body),
        Err(err) => {
            error!("Failed to serialize OS info: {}", err);
            HttpResponse::InternalServerError().finish()
        }
    }
}

static GLOBAL_DATA: Lazy<Arc<RwLock<SystemStats>>> =
    Lazy::new(|| Arc::new(RwLock::new(SystemStats::new())));

static GLOBAL_TIMINGS: Lazy<Arc<RwLock<OperationTimings>>> =
    Lazy::new(|| Arc::new(RwLock::new(OperationTimings::default())));

static GLOBAL_PROGRAM_STATS: Lazy<Arc<RwLock<ProgramUsageStats>>> =
    Lazy::new(|| Arc::new(RwLock::new(ProgramUsageStats::default())));

static GLOBAL_PRESSURE: Lazy<Arc<RwLock<PressureSnapshot>>> =
    Lazy::new(|| Arc::new(RwLock::new(PressureSnapshot::default())));

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let use_gui = std::env::args().any(|arg| arg == "--gui");

    let config = app_config::load_config().map_err(|err| {
        error!("Failed to load configuration from file: {}", err);
        io::Error::new(io::ErrorKind::Other, err.to_string())
    })?;

    if let Err(err) = db::init_db(&config.db_file_name) {
        error!("Failed to initialize database: {}", err);
        return Err(io::Error::new(io::ErrorKind::Other, err.to_string()));
    }

    osinfo::show_and_save_os_info_to_db(&config.db_file_name);

    let manager = SqliteConnectionManager::file(&config.db_file_name);
    let pool = Pool::builder().max_size(8).build(manager).map_err(|err| {
        error!("Failed to create SQLite pool: {}", err);
        io::Error::new(io::ErrorKind::Other, err.to_string())
    })?;
    let pool = Arc::new(pool);

    let exclude_interfaces = Arc::new(
        config
            .exclude_interfaces
            .split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<String>>(),
    );

    tokio::spawn(collect_and_save_stats(
        config.interval_secs,
        config.db_save,
        Arc::clone(&exclude_interfaces),
        Arc::clone(&pool),
    ));

    if use_gui {
        println!("Starting console view (--gui enabled)");
        console_ui::spawn_console_view(
            Arc::clone(&GLOBAL_DATA),
            Arc::clone(&GLOBAL_TIMINGS),
            Arc::clone(&GLOBAL_PROGRAM_STATS),
            Arc::clone(&GLOBAL_PRESSURE),
        );
    }

    if let Err(err) = process::find_top_processes() {
        warn!("Failed to read top processes: {}", err);
    }

    if config.db_save {
        tokio::spawn(clean_history_data_in_db(
            config.db_history_days,
            Arc::clone(&pool),
        ));
    }

    let listen_addr = config.listen_addr.clone();

    HttpServer::new(|| {
        App::new()
            .wrap(Cors::permissive())
            .route("/system-stats", web::get().to(system_stats))
            .route("/system-info", web::get().to(system_info))
    })
    .bind(&listen_addr)?
    .run()
    .await
}

fn persist_stats(
    conn: &Connection,
    cpu_usage: (f64, f64),
    mem_used: u64,
    mem_avail: u64,
    mem_total: u64,
    swap_free: u64,
    swap_total: u64,
    mut disk_info: Vec<disk_io::DiskInfo>,
    mut network_info: Vec<network::NetworkInterface>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO stats (cpu_usage, cpu_io_wait, used_memory, avail_memory, total_memory, swap_free_memory, swap_total_memory) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![cpu_usage.0, cpu_usage.1, mem_used, mem_avail, mem_total, swap_free, swap_total],
    )?;

    while let Some(item) = disk_info.pop() {
        conn.execute(
            "INSERT INTO disks (disk_name, read_count, write_count, read_io_time, write_io_time) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![item.devices, item.read_ops, item.write_ops, item.read_time_ms, item.write_time_ms],
        )?;
    }

    while let Some(item) = network_info.pop() {
        conn.execute(
            "INSERT INTO network (interface_name, bytes_received, bytes_transmitted) VALUES (?1, ?2, ?3)",
            params![item.interface_name, item.bytes_received, item.bytes_transmitted],
        )?;
    }

    Ok(())
}

#[derive(Debug)]
enum DbTaskError {
    Pool(r2d2::Error),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for DbTaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DbTaskError::Pool(err) => write!(f, "pool error: {}", err),
            DbTaskError::Sqlite(err) => write!(f, "sqlite error: {}", err),
        }
    }
}

impl std::error::Error for DbTaskError {}
