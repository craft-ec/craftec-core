use crate::permissions::{Capability, PermissionSet};
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wasmtime::*;

/// Shared state accessible by host functions for a single agent.
pub struct HostState {
    pub db: Arc<Mutex<Connection>>,
    pub permissions: PermissionSet,
    pub config: HashMap<String, String>,
    pub logs: Arc<Mutex<Vec<LogEntry>>>,
    pub events: Arc<Mutex<Vec<Event>>>,
    /// Shared memory buffer for passing data between host and guest.
    pub shared_buffer: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub event_type: String,
    pub payload: String,
}

impl HostState {
    pub fn new(db: Connection, permissions: PermissionSet) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            permissions,
            config: HashMap::new(),
            logs: Arc::new(Mutex::new(Vec::new())),
            events: Arc::new(Mutex::new(Vec::new())),
            shared_buffer: vec![0u8; 4096],
        }
    }
}

/// Register standard host functions into the linker.
pub fn register_host_fns(linker: &mut Linker<HostState>) -> Result<(), anyhow::Error> {
    // log(level_ptr, level_len, msg_ptr, msg_len)
    linker.func_wrap(
        "env",
        "log",
        |mut caller: Caller<'_, HostState>, level_ptr: i32, level_len: i32, msg_ptr: i32, msg_len: i32| {
            let memory = caller.get_export("memory").and_then(|e| e.into_memory());
            let memory = match memory {
                Some(m) => m,
                None => return,
            };
            let data = memory.data(&caller);
            let level = std::str::from_utf8(
                &data[level_ptr as usize..(level_ptr + level_len) as usize],
            )
            .unwrap_or("unknown")
            .to_string();
            let message = std::str::from_utf8(
                &data[msg_ptr as usize..(msg_ptr + msg_len) as usize],
            )
            .unwrap_or("")
            .to_string();
            tracing::info!(agent_level = %level, "{}", message);
            let logs = caller.data().logs.clone();
            logs.lock().unwrap().push(LogEntry { level, message });
        },
    )?;

    // current_time() -> u64
    linker.func_wrap("env", "current_time", |_caller: Caller<'_, HostState>| -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    })?;

    // emit_event(type_ptr, type_len, payload_ptr, payload_len)
    linker.func_wrap(
        "env",
        "emit_event",
        |mut caller: Caller<'_, HostState>, type_ptr: i32, type_len: i32, payload_ptr: i32, payload_len: i32| {
            let memory = caller.get_export("memory").and_then(|e| e.into_memory());
            let memory = match memory {
                Some(m) => m,
                None => return,
            };
            let data = memory.data(&caller);
            let event_type = std::str::from_utf8(
                &data[type_ptr as usize..(type_ptr + type_len) as usize],
            )
            .unwrap_or("")
            .to_string();
            let payload = std::str::from_utf8(
                &data[payload_ptr as usize..(payload_ptr + payload_len) as usize],
            )
            .unwrap_or("")
            .to_string();
            let events = caller.data().events.clone();
            events.lock().unwrap().push(Event { event_type, payload });
        },
    )?;

    // db_execute(sql_ptr, sql_len) -> i32 (rows affected or -1 on error)
    linker.func_wrap(
        "env",
        "db_execute",
        |mut caller: Caller<'_, HostState>, sql_ptr: i32, sql_len: i32| -> i32 {
            if caller.data().permissions.check(Capability::WriteOwnDb).is_err() {
                return -1;
            }
            let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m,
                None => return -1,
            };
            let data = memory.data(&caller);
            let sql = match std::str::from_utf8(&data[sql_ptr as usize..(sql_ptr + sql_len) as usize]) {
                Ok(s) => s.to_string(),
                Err(_) => return -1,
            };
            let db = caller.data().db.clone();
            let db_guard = db.lock().unwrap();
            match db_guard.execute(&sql, []) {
                Ok(n) => n as i32,
                Err(_) => -1,
            }
        },
    )?;

    // db_query(sql_ptr, sql_len) -> i32 (number of rows or -1 on error)
    linker.func_wrap(
        "env",
        "db_query",
        |mut caller: Caller<'_, HostState>, sql_ptr: i32, sql_len: i32| -> i32 {
            if caller.data().permissions.check(Capability::ReadOwnDb).is_err() {
                return -1;
            }
            let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m,
                None => return -1,
            };
            let data = memory.data(&caller);
            let sql = match std::str::from_utf8(&data[sql_ptr as usize..(sql_ptr + sql_len) as usize]) {
                Ok(s) => s.to_string(),
                Err(_) => return -1,
            };
            let db = caller.data().db.clone();
            let db_guard = db.lock().unwrap();
            let result = db_guard.prepare(&sql).and_then(|mut stmt| {
                let count = stmt.query_map([], |_row| Ok(()))?.count();
                Ok(count as i32)
            });
            match result {
                Ok(n) => n,
                Err(_) => -1,
            }
        },
    )?;

    // get_config(key_ptr, key_len) -> i32 (value length written to shared buffer, or -1)
    linker.func_wrap(
        "env",
        "get_config",
        |mut caller: Caller<'_, HostState>, key_ptr: i32, key_len: i32| -> i32 {
            let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m,
                None => return -1,
            };
            let data = memory.data(&caller);
            let key = match std::str::from_utf8(&data[key_ptr as usize..(key_ptr + key_len) as usize]) {
                Ok(s) => s.to_string(),
                Err(_) => return -1,
            };
            match caller.data().config.get(&key) {
                Some(val) => val.len() as i32,
                None => -1,
            }
        },
    )?;

    Ok(())
}

/// Register Tier 1 extra host functions (storage, network, DHT).
/// These are stubs — real implementations will integrate with CraftOBJ/CraftNET.
pub fn register_tier1_host_fns(linker: &mut Linker<HostState>) -> Result<(), anyhow::Error> {
    // storage_put(data_ptr, data_len) -> i32 (CID length or -1)
    linker.func_wrap(
        "env",
        "storage_put",
        |mut caller: Caller<'_, HostState>, data_ptr: i32, data_len: i32| -> i32 {
            if caller.data().permissions.check(Capability::StorageWrite).is_err() {
                return -1;
            }
            let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m,
                None => return -1,
            };
            let data = memory.data(&caller);
            let bytes = &data[data_ptr as usize..(data_ptr + data_len) as usize];
            let hash = blake3::hash(bytes);
            let cid = hash.to_hex().to_string();
            tracing::debug!(cid = %cid, size = data_len, "storage_put");
            cid.len() as i32
        },
    )?;

    // storage_get(cid_ptr, cid_len) -> i32 (data length or -1)
    linker.func_wrap(
        "env",
        "storage_get",
        |caller: Caller<'_, HostState>, _cid_ptr: i32, _cid_len: i32| -> i32 {
            if caller.data().permissions.check(Capability::StorageRead).is_err() {
                return -1;
            }
            // Stub: would fetch from CraftOBJ
            -1
        },
    )?;

    // network_send(peer_ptr, peer_len, msg_ptr, msg_len) -> i32
    linker.func_wrap(
        "env",
        "network_send",
        |caller: Caller<'_, HostState>, _peer_ptr: i32, _peer_len: i32, _msg_ptr: i32, _msg_len: i32| -> i32 {
            if caller.data().permissions.check(Capability::NetworkSend).is_err() {
                return -1;
            }
            // Stub: would send via CraftNET
            0
        },
    )?;

    // dht_put(key_ptr, key_len, val_ptr, val_len) -> i32
    linker.func_wrap(
        "env",
        "dht_put",
        |caller: Caller<'_, HostState>, _key_ptr: i32, _key_len: i32, _val_ptr: i32, _val_len: i32| -> i32 {
            if caller.data().permissions.check(Capability::DhtAccess).is_err() {
                return -1;
            }
            0
        },
    )?;

    // dht_get(key_ptr, key_len) -> i32
    linker.func_wrap(
        "env",
        "dht_get",
        |caller: Caller<'_, HostState>, _key_ptr: i32, _key_len: i32| -> i32 {
            if caller.data().permissions.check(Capability::DhtAccess).is_err() {
                return -1;
            }
            -1
        },
    )?;

    Ok(())
}
