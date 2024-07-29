mod app_config;
mod cpu;
mod db;
mod disk_io;
mod memory;
mod network;
mod osinfo;
mod process;

use crate::osinfo::get_os_info_api;
use actix_cors::Cors;
use actix_files::Files;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer, Result};
use once_cell::sync::Lazy;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time;

#[derive(Debug, Deserialize)]
struct Settings {
    interval_secs: u64,
    listen_addr: String,
    db_file_name: String,
    db_save: bool,
    db_history_days: u64,
    exclude_interfaces: String,
}

#[derive(Serialize)]
struct SystemStats {
    cpu_usage: f64,
    used_memory: u64,
    available_memory: u64,
    total_memory: u64,
    swap_usage: u64,
    swap_total: u64,
}

impl SystemStats {
    fn new() -> Self {
        SystemStats {
            cpu_usage: 0f64,
            total_memory: 0,
            used_memory: 0,
            available_memory: 0,
            swap_usage: 0,
            swap_total: 0,
        }
    }
}

async fn clean_history_data_in_db(days_old: u64, db_file_name: String) -> rusqlite::Result<usize> {
    const TIMER_TRIGGER: u64 = 86400;
    let mut interval = time::interval(Duration::from_secs(TIMER_TRIGGER));
    let conn = Connection::open(db_file_name.clone()).expect("DB connection failed!");

    loop {
        interval.tick().await;
        let stmt = "DELETE FROM stats WHERE timestamp < datetime('now', ?1)";
        conn.execute(stmt, rusqlite::params![format!("-{} days", days_old)])
            .expect("Error truncating history data");
        println!("Historic data has been truncated! (over {} days)", days_old);
    }
}

async fn collect_and_save_stats(
    interval_secs: &u64,
    db_file_name: &String,
    db_save: &bool,
    excluded_interfaces: Vec<String>,
) {
    if *db_save == false {
        return;
    }

    use std::time::Instant;
    let now = Instant::now();

    let mut interval = time::interval(Duration::from_secs(*interval_secs));
    let conn = Connection::open(db_file_name).expect("DB connection failed!");

    let elapsed = now.elapsed();
    //println!("Elapsed pre: {:.2?}", elapsed);

    loop {
        let now = Instant::now();
        interval.tick().await;

        let cpu_usage = cpu::read_cpu_stats().await;
        let (mem_total, mem_free, mem_avail, swap_total, swap_free) = memory::read_memory_info();

        let mut data = GLOBAL_DATA.write().await;
        data.cpu_usage = cpu_usage.0;

        let mem_used = mem_total - mem_free;
        data.used_memory = mem_used;
        data.total_memory = mem_total;
        data.swap_usage = swap_total - swap_free;
        data.swap_total = swap_total;
        data.available_memory = mem_avail;

        conn.execute(
            "INSERT INTO stats (cpu_usage, cpu_io_wait, used_memory, avail_memory, total_memory, swap_free_memory, swap_total_memory) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![cpu_usage.0, cpu_usage.1, mem_used, mem_avail, mem_total, swap_free, swap_total],
        ).expect("Failed to insert stats");

        // println!("Stats saved: CPU usage: {:.2}%, CPU IO Wait: {:.2}%, Used memory: {} KB, Available memory {} KB, Total memory: {} KB, Swap free: {} KB, Swap Total: {} KB",
        //          cpu_usage.0, cpu_usage.1, mem_used, mem_avail, mem_total, swap_free, swap_total);

        let mut disk_info = disk_io::get_physical_disk_io_stats();
        while !disk_info.is_empty() {
            let item = disk_info.pop().unwrap();
            conn.execute(
                "INSERT INTO disks (disk_name, read_count, write_count, read_io_time, write_io_time) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![item.devices, item.read_ops, item.write_ops, item.read_time_ms, item.write_time_ms])
                .expect("Failed to insert disk stats");
        }

        let mut network_info = network::get_network_io_stats(excluded_interfaces.clone());

        while !network_info.is_empty() {
            let item = network_info.pop().unwrap();
            conn.execute(
                "INSERT INTO network (interface_name, bytes_received, bytes_transmitted) VALUES (?1, ?2, ?3)",
                params![item.interface_name, item.bytes_received, item.bytes_transmitted],
            ).expect("Failed to insert network stats");
        }

        let elapsed = now.elapsed();
        //println!("TIME SAVE METRICS (ALL): {:.2?}", elapsed);
        //self_memory_stats().await.unwrap();
        memory::read_process_swap_usage_with_names().unwrap();
    }
}

async fn self_memory_stats() -> io::Result<()> {
    let path = "/proc/self/status";
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        if line.starts_with("VmRSS:") {
            println!("Self memory usage - {}", line);
            break;
        }
    }

    Ok(())
}

async fn system_stats() -> HttpResponse {
    let mut stats = format!("{{ \"My CPU Usage\" : \"{:.2}\",\n\"Ram Usage\" : \"{:}\",\n\"Ram Total\" : \"{:}\",\n\"Ram Available\" : \"{:}\",\n\"Swap Usage\": \"{:}\",\n\"Swap Total\": \"{:}\" }}",
                            GLOBAL_DATA.read().await.cpu_usage,
                            GLOBAL_DATA.read().await.used_memory,
                            GLOBAL_DATA.read().await.total_memory,
                            GLOBAL_DATA.read().await.available_memory,
                            GLOBAL_DATA.read().await.swap_usage,
                            GLOBAL_DATA.read().await.swap_total);
    HttpResponse::Ok().content_type("text/plain").body(stats)
}

async fn system_info() -> HttpResponse {
    let info = get_os_info_api();

    HttpResponse::Ok()
        .content_type("application/json")
        .body(info.unwrap())
}

static GLOBAL_DATA: Lazy<Arc<RwLock<SystemStats>>> =
    Lazy::new(|| Arc::new(RwLock::new(SystemStats::new())));

async fn index(_req: HttpRequest) -> Result<HttpResponse> {
    Ok(HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(include_str!("../static/index.html")))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = app_config::load_config().expect("Failed to load configuration from file");
    let file_db_name = config.db_file_name.clone();

    db::init_db().expect("Failed to initialize database");
    osinfo::show_and_save_os_info_to_db(&file_db_name);

    let exclude_interfaces = config
        .exclude_interfaces
        .split(',')
        .map(|s| s.trim().to_string())
        .collect::<Vec<String>>();

    tokio::spawn(async move {
        collect_and_save_stats(
            &config.interval_secs,
            &config.db_file_name,
            &config.db_save,
            exclude_interfaces,
        )
        .await;
    });

    let proc = process::find_top_processes();

    tokio::spawn(async move {
        clean_history_data_in_db(config.db_history_days, file_db_name)
            .await
            .unwrap();
    });

    HttpServer::new(|| {
        App::new()
            .wrap(Cors::permissive())
            .route("/system-stats", web::get().to(system_stats))
            .route("/system-info", web::get().to(system_info))
    })
    .bind(&config.listen_addr)?
    .run()
    .await
}
