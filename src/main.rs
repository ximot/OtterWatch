mod cpu;
mod db;
mod disk_io;
mod memory;
mod osinfo;

use actix_web::{web, App, HttpResponse, HttpServer};
use config::{Config, File};
use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use sysinfo::System;
use tokio::time;

#[derive(Debug, Deserialize)]
struct Settings {
    interval_secs: u64,
    listen_addr: String,
    db_file_name: String,
    db_save: bool,
    db_history_days: u64,
}

#[derive(Serialize)]
struct SystemStats {
    cpu_usage: f32,
    used_memory: u64,
    total_memory: u64,
}

fn load_confg() -> Result<Settings, config::ConfigError> {
    let mut settings = Config::default();
    settings
        .merge(File::with_name("Settings"))?
        .merge(config::Environment::with_prefix("APP"))?;
    settings.try_into()
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

async fn collect_and_save_stats(interval_secs: &u64, db_file_name: &String, db_save: &bool) {
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
            "INSERT INTO stats (cpu_usage, used_memory, avail_memory, total_memory, swap_free_memory, swap_total_memory) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![cpu_usage, mem_used, mem_avail, mem_total, swap_free, swap_total],
        ).expect("Failed to insert stats");

        println!("Stats saved: CPU usage: {:.2}%, Used memory: {} KB, Available memory {} KB, Total memory: {} KB, Swap free: {} KB, Swap Total: {} KB",
                 cpu_usage, mem_used, mem_avail, mem_total, swap_free, swap_total);

        let mut disk_info = disk_io::get_physical_disk_io_stats();
        while (!disk_info.is_empty()) {
            let mut item = disk_info.pop().unwrap();
            conn.execute(
                "INSERT INTO disks (disk_name, read_count, write_count, read_io_time, write_io_time) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![item.devices, item.read_ops, item.write_ops, item.read_time_ms, item.write_time_ms])
                .expect("Faile to insert disk stats");
        }
    }
}

async fn system_stats() -> HttpResponse {
    let mut system = System::new_all();
    system.refresh_all();
    let cpu_usage = system.global_cpu_info().cpu_usage();
    let total_memory = system.total_memory();
    let free_memory = system.free_memory();
    let used_memory = system.used_memory();
    //let cpu_name = system.cpus()[0].brand();
    let cpu_speed = system.cpus()[0].frequency();
    let mut cpu_real_usage = 0f32;

    tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
    system.refresh_cpu();

    for cpu in system.cpus() {
        cpu_real_usage += cpu.cpu_usage()
    }

    let cpu_0: f32 = system.cpus()[0].cpu_usage();
    let cpu_1: f32 = system.cpus()[1].cpu_usage();
    let cpu_2: f32 = system.cpus()[2].cpu_usage();
    let cpu_3: f32 = system.cpus()[3].cpu_usage();
    let cpu_4: f32 = system.cpus()[4].cpu_usage();
    let cpu_5: f32 = system.cpus()[5].cpu_usage();
    let cpu_6: f32 = system.cpus()[6].cpu_usage();
    let cpu_7: f32 = system.cpus()[7].cpu_usage();
    let cpu_8: f32 = system.cpus()[8].cpu_usage();
    let cpu_9: f32 = system.cpus()[9].cpu_usage();
    let cpu_10: f32 = system.cpus()[10].cpu_usage();
    let cpu_11: f32 = system.cpus()[11].cpu_usage();

    cpu_real_usage = cpu_real_usage / system.cpus().len() as f32;

    let stats = format!(
        "CPU Usage: {:.2}%\nCPU Usage (AVG): {:.2}%\nCPU 1 Usage: {:.2}%\nCPU 2 Usage: {:.2}%\nCPU 3 Usage: {:.2}%\nCPU 4 Usage: {:.2}%\nCPU 5 Usage: {:.2}%\nCPU 6 Usage: {:.2}%\nCPU 7 Usage: {:.2}%\nCPU 8 Usage: {:.2}%\nCPU 9 Usage: {:.2}%\nCPU 10 Usage: {:.2}%\nCPU 11 Usage: {:.2}%\nCPU 12 Usage: {:.2}%\nTotal Memory: {} B\nUsed Memory: {} B\nFree Memory: {} B\nCPU Frequency: {} MHz",
        cpu_real_usage, cpu_usage, cpu_0, cpu_1 , cpu_2 , cpu_3 , cpu_4 , cpu_5 , cpu_6 , cpu_7 , cpu_8 , cpu_9 , cpu_10, cpu_11  , total_memory, used_memory, free_memory, cpu_speed
    );
    HttpResponse::Ok().content_type("text/plain").body(stats)
}

async fn system_stats_my() -> HttpResponse {
    let stats = format!("My CPU Usage: {:.2}%\n", cpu::read_cpu_stats().await);
    HttpResponse::Ok().content_type("text/plain").body(stats)
}

async fn system_stats_json() -> HttpResponse {
    let mut system = System::new_all();
    system.refresh_all();
    let stats = SystemStats {
        cpu_usage: system.global_cpu_info().cpu_usage(),
        used_memory: system.used_memory(),
        total_memory: system.total_memory(),
    };

    HttpResponse::Ok().json(stats)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = load_confg().expect("Failed to load configuration from file");
    let file_db_name = config.db_file_name.clone();

    db::init_db().expect("Failed to initialize database");
    // osinfo::show_os_info();
    // osinfo::save_os_info_to_db(&file_db_name);
    osinfo::show_and_save_os_info_to_db(&file_db_name);

    tokio::spawn(async move {
        collect_and_save_stats(&config.interval_secs, &config.db_file_name, &config.db_save).await;
    });

    tokio::spawn(async move {
        clean_history_data_in_db(config.db_history_days, file_db_name)
            .await
            .unwrap();
    });

    HttpServer::new(|| {
        App::new()
            .route("/system-stats", web::get().to(system_stats))
            .route("/system-stats-json", web::get().to(system_stats_json))
            .route("/system-stats-my", web::get().to(system_stats_my))
    })
    .bind(&config.listen_addr)?
    .run()
    .await
}
