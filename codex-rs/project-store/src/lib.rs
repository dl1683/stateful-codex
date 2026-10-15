//! Engine-independent ownership of a project's local knowledge-store location.
//! Resolution does not create a store or change native engine storage routing.

mod filesystem;
mod location;
mod registry;

pub use filesystem::FilesystemQualificationError;
pub use filesystem::qualify_local_filesystem;
pub use location::ProjectStoreLocation;
pub use location::ResolveError;
pub use registry::StoreIdentity;
pub use registry::StoreRegistryError;
pub use registry::initialize_connection;
pub use registry::read_identity;
