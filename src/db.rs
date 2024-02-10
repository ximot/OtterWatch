use rusqlite::Connection;

pub fn init_db() -> rusqlite::Result<()> {
    let conn = Connection::open("system_stats.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS stats (
            id INTEGER PRIMARY KEY,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            cpu_usage REAL,
            used_memory INTEGER,
            avail_memory INTEGER,
            total_memory INTEGER,
            swap_free_memory INTEGER,
            swap_total_memory INTEGER
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS system (
            id INTEGER PRIMARY KEY,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            hostname TEXT,
            os_name TEXT,
            kernel_version	TEXT,
            boot_time TEXT,
            cpu_name TEXT,
            cpu_cores NUMERIC
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS disks (
            id INTEGER PRIMARY KEY,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            disk_name TEXT,
            read_count NUMERIC,
            write_count NUMERIC,
            read_io_time NUMERIC,
            write_io_time NUMERIC
        )",
        [],
    )?;
    Ok(())
}
