//! Engine-neutral project-store foundations. No engine home or native routing.

mod registry;

pub use registry::StoreIdentity;
pub use registry::StoreRegistryError;
pub use registry::initialize_connection;
pub use registry::read_identity;
