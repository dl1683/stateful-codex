//! Engine-independent project-store placement, metadata and local opening.
//! Native engine storage routing remains unchanged.

mod domains;
mod filesystem;
mod location;
mod opener;
mod reader;
mod registry;

pub use domains::DomainManifest;
pub use domains::PreparedDomains;
pub use filesystem::FilesystemQualificationError;
pub use filesystem::qualify_local_filesystem;
pub use location::ProjectStoreLocation;
pub use location::ResolveError;
pub use opener::ProjectStoreOpenError;
pub use opener::initialize_local_store;
pub use opener::inspect_store;
pub use reader::ProjectStoreReader;
pub use registry::StoreIdentity;
pub use registry::StoreRegistryError;
pub use registry::initialize_connection;
pub use registry::read_identity;
