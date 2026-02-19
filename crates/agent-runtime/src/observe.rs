use rusqlite::Connection;

/// Create the auto-injected observability tables for an agent.
pub fn create_observability_tables(db: &Connection) -> Result<(), rusqlite::Error> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS _health (
            timestamp INTEGER NOT NULL DEFAULT (strftime('%s','now')),
            status TEXT NOT NULL DEFAULT 'running',
            last_action INTEGER,
            error TEXT
        );

        CREATE TABLE IF NOT EXISTS _metrics (
            timestamp INTEGER NOT NULL DEFAULT (strftime('%s','now')),
            metric TEXT NOT NULL,
            value REAL NOT NULL
        );

        CREATE TABLE IF NOT EXISTS _log (
            timestamp INTEGER NOT NULL DEFAULT (strftime('%s','now')),
            level TEXT NOT NULL,
            message TEXT NOT NULL
        );"
    )?;
    Ok(())
}

/// Check if an agent is healthy (has written to _health within the heartbeat interval).
pub fn check_health(db: &Connection, heartbeat_interval_secs: i64) -> Result<bool, rusqlite::Error> {
    let result: Result<i64, _> = db.query_row(
        "SELECT MAX(timestamp) FROM _health",
        [],
        |row| row.get(0),
    );
    match result {
        Ok(ts) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Ok(now - ts <= heartbeat_interval_secs)
        }
        Err(_) => Ok(false),
    }
}
