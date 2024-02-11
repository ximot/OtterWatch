mod app_config;
mod cpu;
mod db;
mod disk_io;
mod memory;
mod network;
mod osinfo;

use actix_web::{web, App, HttpResponse, HttpServer};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::time::Duration;
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
    cpu_usage: f32,
    used_memory: u64,
    total_memory: u64,
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

    let mut interval = time::interval(Duration::from_secs(*interval_secs));
    let conn = Connection::open(db_file_name).expect("DB connection failed!");

    loop {
        interval.tick().await;

        let cpu_usage = cpu::read_cpu_stats().await;
        let (mem_total, mem_free, mem_avail, swap_total, swap_free) = memory::read_memory_info();

        let mem_used = mem_total - mem_free;
        conn.execute(
            "INSERT INTO stats (cpu_usage, cpu_io_wait, used_memory, avail_memory, total_memory, swap_free_memory, swap_total_memory) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![cpu_usage.0, cpu_usage.1, mem_used, mem_avail, mem_total, swap_free, swap_total],
        ).expect("Failed to insert stats");

        println!("Stats saved: CPU usage: {:.2}%, CPU IO Wait: {:.2}%, Used memory: {} KB, Available memory {} KB, Total memory: {} KB, Swap free: {} KB, Swap Total: {} KB",
                 cpu_usage.0, cpu_usage.1, mem_used, mem_avail, mem_total, swap_free, swap_total);

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
    }
}

async fn system_stats() -> HttpResponse {
    let stats = format!("My CPU Usage: {:.2}%\n", cpu::read_cpu_stats().await.0);
    HttpResponse::Ok().content_type("text/plain").body(stats)
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

    tokio::spawn(async move {
        clean_history_data_in_db(config.db_history_days, file_db_name)
            .await
            .unwrap();
    });

    HttpServer::new(|| App::new().route("/system-stats", web::get().to(system_stats)))
        .bind(&config.listen_addr)?
        .run()
        .await
}
