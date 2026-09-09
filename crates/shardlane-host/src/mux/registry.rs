//! Backend registry: the sole assembly point that names backends. Consumers
//! hold [`MuxRegistry`] (or an `Arc` of it) and route [`InstanceRef`]s by
//! backend id; nothing above this module names a backend.

use std::sync::Arc;

use super::herdr::HerdrBackend;
use super::{InstanceListing, InstanceRef, Multiplexer, MultiplexerConnection, MuxError};

#[derive(Clone, Default)]
pub struct MuxRegistry {
    backends: Vec<Arc<dyn Multiplexer>>,
}

impl MuxRegistry {
    /// Registry with the builtin backends (Herdr first, tmux MVP second).
    /// Registration does no I/O; backend enumeration happens per call.
    pub fn with_builtins() -> Self {
        let mut registry = Self {
            backends: Vec::new(),
        };
        registry.register(Arc::new(HerdrBackend::default()));
        registry.register(Arc::new(super::tmux::TmuxBackend));
        registry.register(Arc::new(super::uuyc::UuycBackend));
        registry.register(Arc::new(super::luvus::LuvusBackend::default()));
        registry
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn register(&mut self, backend: Arc<dyn Multiplexer>) {
        self.backends.push(backend);
    }

    pub fn backend(&self, id: &str) -> Option<Arc<dyn Multiplexer>> {
        self.backends.iter().find(|b| b.id() == id).cloned()
    }

    pub fn backends(&self) -> &[Arc<dyn Multiplexer>] {
        &self.backends
    }

    /// Concatenated instance enumeration; backends whose enumeration is
    /// unavailable are skipped (their callers degrade via capabilities).
    pub fn list_instances(&self) -> Vec<InstanceListing> {
        self.backends
            .iter()
            .filter_map(|backend| backend.list_instances())
            .flatten()
            .collect()
    }

    fn route(&self, reference: &InstanceRef) -> Result<Arc<dyn Multiplexer>, MuxError> {
        self.backend(&reference.backend)
            .ok_or_else(|| MuxError::Api(format!("unknown backend {:?}", reference.backend)))
    }

    pub fn open_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        self.route(reference)?.open_instance(reference)
    }

    pub fn connect_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        self.route(reference)?.connect_instance(reference)
    }
}
