//! Project-scoped structured knowledge for Stateful Codex.

mod context_map;
mod context_map_storage;
mod hierarchy;
mod storage;

pub use context_map::ContextMapCoverage;
pub use context_map::ContextMapEntry;
pub use context_map::ContextMapEntryId;
pub use context_map::ContextMapError;
pub use context_map::ContextMapFreshness;
pub use context_map::ContextMapHit;
pub use context_map::ContextMapQuery;
pub use context_map::NewContextMapEntry;
pub use context_map_storage::ContextMapStore;
pub use context_map_storage::ContextMapStoreError;
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
