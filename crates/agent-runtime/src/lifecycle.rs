use crate::host_fns::{register_host_fns, register_tier1_host_fns, HostState};
use crate::observe::create_observability_tables;
use crate::permissions::PermissionSet;
use crate::sandbox::{SandboxConfig, SandboxError, WasmSandbox};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use wasmtime::*;

/// Agent metadata.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentManifest {
    /// Content ID of the WASM binary.
    pub cid: String,
    pub name: String,
    pub version: String,
    pub required_capabilities: Vec<crate::permissions::Capability>,
    /// 1 = whitelisted infrastructure, 2 = sandboxed user agent.
    pub tier: u8,
}

/// A running agent instance.
pub struct AgentInstance {
    pub manifest: AgentManifest,
    pub store: Store<HostState>,
    pub instance: Instance,
    pub db_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    #[error("sandbox error: {0}")]
    Sandbox(#[from] SandboxError),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("wasm error: {0}")]
    Wasm(#[from] anyhow::Error),
    #[error("agent has no _init or _start export")]
    NoEntryPoint,
}

/// Load an agent: create sandbox, database, and WASM instance.
pub fn load_agent(
    wasm_bytes: &[u8],
    manifest: AgentManifest,
    permissions: PermissionSet,
    db_dir: &Path,
    sandbox_config: Option<SandboxConfig>,
) -> Result<AgentInstance, LifecycleError> {
    let sandbox = WasmSandbox::new(sandbox_config.unwrap_or_default())?;

    // Each agent gets its own SQLite file
    let db_path = db_dir.join(format!("{}.sqlite", manifest.cid));
    let db = Connection::open(&db_path)?;
    create_observability_tables(&db)?;

    let host_state = HostState::new(db, permissions.clone());
    let mut store = sandbox.create_store(host_state)?;

    let mut linker = Linker::new(sandbox.engine());
    register_host_fns(&mut linker)?;

    // Tier 1 agents get extra host functions
    if manifest.tier == 1 {
        register_tier1_host_fns(&mut linker)?;
    }

    let instance = sandbox.instantiate(&mut store, wasm_bytes, &linker)?;

    Ok(AgentInstance {
        manifest,
        store,
        instance,
        db_path,
    })
}

/// Start an agent by calling its `_init` or `_start` export.
pub fn start_agent(agent: &mut AgentInstance) -> Result<(), LifecycleError> {
    // Try _init first, then _start
    if let Some(func) = agent.instance.get_func(&mut agent.store, "_init") {
        func.call(&mut agent.store, &[], &mut [])?;
        Ok(())
    } else if let Some(func) = agent.instance.get_func(&mut agent.store, "_start") {
        func.call(&mut agent.store, &[], &mut [])?;
        Ok(())
    } else {
        Err(LifecycleError::NoEntryPoint)
    }
}

/// Stop an agent. Calls `_shutdown` if it exists, then drops.
pub fn stop_agent(agent: &mut AgentInstance) -> Result<(), LifecycleError> {
    if let Some(func) = agent.instance.get_func(&mut agent.store, "_shutdown") {
        // Best-effort shutdown call
        let _ = func.call(&mut agent.store, &[], &mut []);
    }
    Ok(())
}

/// Registry of running agents.
pub struct AgentRegistry {
    agents: HashMap<String, AgentManifest>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    pub fn register(&mut self, manifest: AgentManifest) {
        self.agents.insert(manifest.cid.clone(), manifest);
    }

    pub fn unregister(&mut self, cid: &str) {
        self.agents.remove(cid);
    }

    pub fn get(&self, cid: &str) -> Option<&AgentManifest> {
        self.agents.get(cid)
    }

    pub fn list(&self) -> Vec<&AgentManifest> {
        self.agents.values().collect()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
