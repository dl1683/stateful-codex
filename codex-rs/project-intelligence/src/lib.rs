//! Project-scoped structured knowledge for Stateful Codex.

mod context_map;
mod hierarchy;
mod storage;

pub use context_map::ContextMapCoverage;
pub use context_map::ContextMapEntry;
pub use context_map::ContextMapEntryId;
pub use context_map::ContextMapError;
pub use context_map::ContextMapFreshness;
pub use context_map::NewContextMapEntry;
pub use hierarchy::HierarchyError;
pub use hierarchy::HierarchyNode;
pub use hierarchy::HierarchyNodeId;
pub use hierarchy::NewHierarchyNode;
pub use hierarchy::NodeKind;
pub use hierarchy::NodeLifecycle;
pub use hierarchy::ProjectRelativePath;
pub use hierarchy::RegionAnchor;
pub use hierarchy::SourceFingerprint;
pub use storage::HierarchySourceUpdate;
pub use storage::HierarchyStore;
pub use storage::HierarchyStoreError;
