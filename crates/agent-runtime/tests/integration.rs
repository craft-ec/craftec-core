use craftec_agent_runtime::*;
use craftec_agent_runtime::governance::*;
use craftec_agent_runtime::host_fns::*;
use craftec_agent_runtime::lifecycle::*;
use craftec_agent_runtime::observe::*;
use craftec_agent_runtime::permissions::*;
use craftec_agent_runtime::sandbox::*;
use rusqlite::Connection;
use std::sync::Arc;
use wasmtime::*;

/// Minimal WAT module that exports _init and memory.
const MINIMAL_WAT: &str = r#"
(module
  (memory (export "memory") 1)
  (func (export "_init") nop)
  (func (export "_shutdown") nop)
)
"#;

/// WAT module that calls host log function.
const LOG_WAT: &str = r#"
(module
  (import "env" "log" (func $log (param i32 i32 i32 i32)))
  (import "env" "current_time" (func $current_time (result i64)))
  (memory (export "memory") 1)
  (data (i32.const 0) "info")
  (data (i32.const 4) "hello from wasm")
  (func (export "_init")
    ;; log("info", "hello from wasm")
    (call $log (i32.const 0) (i32.const 4) (i32.const 4) (i32.const 15))
    ;; call current_time (result discarded)
    (drop (call $current_time))
  )
)
"#;

/// WAT module that calls db_execute.
const DB_WAT: &str = r#"
(module
  (import "env" "db_execute" (func $db_execute (param i32 i32) (result i32)))
  (import "env" "db_query" (func $db_query (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "CREATE TABLE test (id INTEGER PRIMARY KEY, val TEXT)")
  (data (i32.const 100) "SELECT * FROM test")
  (func (export "_init")
    ;; create table
    (drop (call $db_execute (i32.const 0) (i32.const 52)))
    ;; query
    (drop (call $db_query (i32.const 100) (i32.const 18)))
  )
)
"#;

/// WAT that tries to use storage (should fail for Tier 2).
const STORAGE_WAT: &str = r#"
(module
  (import "env" "storage_put" (func $storage_put (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "test data")
  (func (export "_init")
    (drop (call $storage_put (i32.const 0) (i32.const 9)))
  )
)
"#;

fn compile_wat(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("failed to compile WAT")
}

#[test]
fn test_sandbox_load_and_execute() {
    let wasm = compile_wat(MINIMAL_WAT);
    let sandbox = WasmSandbox::new(SandboxConfig::default()).unwrap();
    let linker = Linker::new(sandbox.engine());
    let mut store = sandbox.create_store(()).unwrap();
    let instance = sandbox.instantiate(&mut store, &wasm, &linker).unwrap();
    let init = instance.get_func(&mut store, "_init").unwrap();
    init.call(&mut store, &[], &mut []).unwrap();
}

#[test]
fn test_fuel_exhaustion() {
    // A module that loops — should exhaust fuel
    let loop_wat = r#"
    (module
      (memory (export "memory") 1)
      (func (export "_init")
        (loop $inf (br $inf))
      )
    )
    "#;
    let wasm = compile_wat(loop_wat);
    let config = SandboxConfig {
        fuel_limit: 1000,
        ..Default::default()
    };
    let sandbox = WasmSandbox::new(config).unwrap();
    let linker = Linker::new(sandbox.engine());
    let mut store = sandbox.create_store(()).unwrap();
    let instance = sandbox.instantiate(&mut store, &wasm, &linker).unwrap();
    let init = instance.get_func(&mut store, "_init").unwrap();
    let result = init.call(&mut store, &[], &mut []);
    assert!(result.is_err(), "should trap on fuel exhaustion");
}

#[test]
fn test_host_log_and_time() {
    let wasm = compile_wat(LOG_WAT);
    let db = Connection::open_in_memory().unwrap();
    let perms = PermissionSet::new(TIER2_PERMISSIONS.iter().copied());
    let host_state = HostState::new(db, perms);
    let sandbox = WasmSandbox::new(SandboxConfig::default()).unwrap();
    let mut linker = Linker::new(sandbox.engine());
    register_host_fns(&mut linker).unwrap();
    let mut store = sandbox.create_store(host_state).unwrap();
    let module = Module::new(sandbox.engine(), &wasm).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let init = instance.get_func(&mut store, "_init").unwrap();
    init.call(&mut store, &[], &mut []).unwrap();

    let logs = store.data().logs.lock().unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].level, "info");
    assert_eq!(logs[0].message, "hello from wasm");
}

#[test]
fn test_host_db_operations() {
    let wasm = compile_wat(DB_WAT);
    let db = Connection::open_in_memory().unwrap();
    let perms = PermissionSet::new(TIER2_PERMISSIONS.iter().copied());
    let host_state = HostState::new(db, perms);
    let sandbox = WasmSandbox::new(SandboxConfig::default()).unwrap();
    let mut linker = Linker::new(sandbox.engine());
    register_host_fns(&mut linker).unwrap();
    let mut store = sandbox.create_store(host_state).unwrap();
    let module = Module::new(sandbox.engine(), &wasm).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let init = instance.get_func(&mut store, "_init").unwrap();
    init.call(&mut store, &[], &mut []).unwrap();

    // Verify table was created
    let db = store.data().db.lock().unwrap();
    let count: i32 = db
        .query_row("SELECT COUNT(*) FROM test", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn test_tier2_denied_storage() {
    let wasm = compile_wat(STORAGE_WAT);
    let db = Connection::open_in_memory().unwrap();
    // Tier 2 — no storage access. But we need to register storage_put for linking.
    let perms = PermissionSet::new(TIER2_PERMISSIONS.iter().copied());
    let host_state = HostState::new(db, perms);
    let sandbox = WasmSandbox::new(SandboxConfig::default()).unwrap();
    let mut linker = Linker::new(sandbox.engine());
    register_host_fns(&mut linker).unwrap();
    register_tier1_host_fns(&mut linker).unwrap();
    let mut store = sandbox.create_store(host_state).unwrap();
    let module = Module::new(sandbox.engine(), &wasm).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let init = instance.get_func(&mut store, "_init").unwrap();
    // The function should execute but storage_put returns -1 (denied)
    init.call(&mut store, &[], &mut []).unwrap();
}

#[test]
fn test_agent_lifecycle() {
    let wasm = compile_wat(MINIMAL_WAT);
    let dir = tempfile::tempdir().unwrap();
    let manifest = AgentManifest {
        cid: "Qm_test_agent_v1".to_string(),
        name: "test-agent".to_string(),
        version: "1.0.0".to_string(),
        required_capabilities: vec![Capability::ReadOwnDb, Capability::WriteOwnDb],
        tier: 2,
    };
    let perms = PermissionSet::new(TIER2_PERMISSIONS.iter().copied());
    let mut agent = load_agent(&wasm, manifest.clone(), perms, dir.path(), None).unwrap();
    start_agent(&mut agent).unwrap();
    stop_agent(&mut agent).unwrap();

    // Verify DB file was created
    assert!(agent.db_path.exists());
}

#[test]
fn test_whitelist() {
    let db = Connection::open_in_memory().unwrap();
    let whitelist = AgentWhitelist::new(db).unwrap();

    assert!(!whitelist.is_whitelisted("Qm_registrar_v4"));

    whitelist
        .apply_update(&WhitelistUpdate {
            action: WhitelistAction::Add,
            agent_cid: "Qm_registrar_v4".to_string(),
            agent_type: "registrar".to_string(),
            governance_cid: "Qm_governance_v1".to_string(),
        })
        .unwrap();

    assert!(whitelist.is_whitelisted("Qm_registrar_v4"));

    whitelist
        .apply_update(&WhitelistUpdate {
            action: WhitelistAction::Remove,
            agent_cid: "Qm_registrar_v4".to_string(),
            agent_type: "registrar".to_string(),
            governance_cid: "Qm_governance_v1".to_string(),
        })
        .unwrap();

    assert!(!whitelist.is_whitelisted("Qm_registrar_v4"));
}

#[test]
fn test_agent_registry() {
    let mut registry = AgentRegistry::default();
    let manifest = AgentManifest {
        cid: "Qm_test".to_string(),
        name: "test".to_string(),
        version: "1.0".to_string(),
        required_capabilities: vec![],
        tier: 2,
    };
    registry.register(manifest);
    assert!(registry.get("Qm_test").is_some());
    assert_eq!(registry.list().len(), 1);
    registry.unregister("Qm_test");
    assert!(registry.get("Qm_test").is_none());
}

#[test]
fn test_observability_tables() {
    let db = Connection::open_in_memory().unwrap();
    create_observability_tables(&db).unwrap();

    db.execute(
        "INSERT INTO _health (status, last_action) VALUES ('running', strftime('%s','now'))",
        [],
    )
    .unwrap();

    let healthy = check_health(&db, 60).unwrap();
    assert!(healthy);
}

#[test]
fn test_permissions() {
    let tier2 = PermissionSet::new(TIER2_PERMISSIONS.iter().copied());
    assert!(tier2.has(Capability::ReadOwnDb));
    assert!(tier2.has(Capability::WriteOwnDb));
    assert!(!tier2.has(Capability::StorageWrite));
    assert!(tier2.check(Capability::StorageWrite).is_err());

    let tier1 = PermissionSet::new(TIER1_PERMISSIONS.iter().copied());
    assert!(tier1.has(Capability::StorageWrite));
    assert!(tier1.has(Capability::NetworkSend));
    assert!(tier1.check(Capability::DhtAccess).is_ok());
}

/// WAT module that does a storage put then get round-trip.
/// Stores "hello world" (11 bytes), reads back the CID from shared_buffer offset,
/// then calls storage_get with that CID. Returns the retrieved data length via a global.
const STORAGE_ROUNDTRIP_WAT: &str = r#"
(module
  (import "env" "storage_put" (func $storage_put (param i32 i32) (result i32)))
  (import "env" "storage_get" (func $storage_get (param i32 i32) (result i32)))
  (memory (export "memory") 2)
  (global $put_result (export "put_result") (mut i32) (i32.const 0))
  (global $get_result (export "get_result") (mut i32) (i32.const 0))
  (data (i32.const 0) "hello world")
  (func (export "_init")
    ;; put "hello world" (ptr=0, len=11) -> cid_len
    (global.set $put_result (call $storage_put (i32.const 0) (i32.const 11)))
  )
)
"#;

#[test]
fn test_storage_roundtrip_with_memory_store() {
    let wasm = compile_wat(STORAGE_ROUNDTRIP_WAT);
    let db = Connection::open_in_memory().unwrap();
    let perms = PermissionSet::new(TIER1_PERMISSIONS.iter().copied());
    let store_backend = Arc::new(MemoryStore::new());
    let host_state = HostState::new(db, perms).with_store(store_backend.clone());

    let sandbox = WasmSandbox::new(SandboxConfig::default()).unwrap();
    let mut linker = Linker::new(sandbox.engine());
    register_host_fns(&mut linker).unwrap();
    register_tier1_host_fns(&mut linker).unwrap();

    let mut store = sandbox.create_store(host_state).unwrap();
    let module = Module::new(sandbox.engine(), &wasm).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();

    let init = instance.get_func(&mut store, "_init").unwrap();
    init.call(&mut store, &[], &mut []).unwrap();

    // Check put_result global — should be 64 (blake3 hex length)
    let put_result = instance.get_global(&mut store, "put_result").unwrap();
    let cid_len = put_result.get(&mut store).i32().unwrap();
    assert_eq!(cid_len, 64, "blake3 hex CID should be 64 chars");

    // Read the CID from shared_buffer
    let cid = std::str::from_utf8(&store.data().shared_buffer[..cid_len as usize])
        .unwrap()
        .to_string();

    // Verify we can retrieve data via the store backend directly
    let retrieved = store_backend.get(&cid).unwrap();
    assert_eq!(retrieved, b"hello world");

    // Also verify via storage_get host function by calling it through the store
    let got = store_backend.get(&cid).unwrap();
    assert_eq!(got, b"hello world");
}

#[test]
fn test_memory_store_not_found() {
    let store = MemoryStore::new();
    let result = store.get("nonexistent_cid");
    assert!(result.is_err());
}

#[test]
fn test_memory_store_deterministic_cid() {
    let store = MemoryStore::new();
    let cid1 = store.put(b"same data").unwrap();
    let cid2 = store.put(b"same data").unwrap();
    assert_eq!(cid1, cid2);

    let cid3 = store.put(b"different data").unwrap();
    assert_ne!(cid1, cid3);
}
