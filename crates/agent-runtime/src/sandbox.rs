use std::time::Duration;
use wasmtime::*;

/// Configuration for a WASM sandbox.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum memory in bytes (default: 64MB).
    pub max_memory_bytes: usize,
    /// Fuel limit for instruction metering (default: 1_000_000).
    pub fuel_limit: u64,
    /// Maximum execution time.
    pub max_execution_time: Duration,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024,
            fuel_limit: 1_000_000,
            max_execution_time: Duration::from_secs(30),
        }
    }
}

/// Isolated WASM execution sandbox. Each agent gets its own instance.
pub struct WasmSandbox {
    engine: Engine,
    config: SandboxConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("wasm error: {0}")]
    Wasm(#[from] anyhow::Error),
    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),
}

impl WasmSandbox {
    pub fn new(config: SandboxConfig) -> Result<Self, SandboxError> {
        let mut engine_config = Config::new();
        engine_config.consume_fuel(true);
        let engine = Engine::new(&engine_config)?;
        Ok(Self { engine, config })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Create a new store with resource limits applied.
    pub fn create_store<T>(&self, data: T) -> Result<Store<T>, SandboxError> {
        let mut store = Store::new(&self.engine, data);
        store.set_fuel(self.config.fuel_limit)?;
        store.set_epoch_deadline(1);
        Ok(store)
    }

    /// Load and instantiate a WASM module with the given linker.
    pub fn instantiate<T>(
        &self,
        store: &mut Store<T>,
        wasm_bytes: &[u8],
        linker: &Linker<T>,
    ) -> Result<Instance, SandboxError> {
        let module = Module::new(&self.engine, wasm_bytes)?;

        // Validate memory limits
        for import in module.imports() {
            if let ExternType::Memory(mem_type) = import.ty() {
                let max_pages = (self.config.max_memory_bytes / 65536) as u64;
                if mem_type.minimum() > max_pages {
                    return Err(SandboxError::ResourceLimit(format!(
                        "module requests {} pages, max is {}",
                        mem_type.minimum(),
                        max_pages
                    )));
                }
            }
        }

        let instance = linker.instantiate(&mut *store, &module)?;
        Ok(instance)
    }
}
