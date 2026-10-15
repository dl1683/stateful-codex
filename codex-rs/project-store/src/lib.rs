//! Engine-independent ownership of a project's local knowledge-store location.
//! Resolution does not create a store or change native engine storage routing.

mod location;

pub use location::ProjectStoreLocation;
pub use location::ResolveError;
