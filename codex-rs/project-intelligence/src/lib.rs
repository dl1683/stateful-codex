//! Project-scoped structured knowledge for Stateful Codex.

mod hierarchy;
mod storage;

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
