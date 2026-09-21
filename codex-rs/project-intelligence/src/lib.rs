//! Project-scoped structured knowledge for Stateful Codex.

mod hierarchy;

pub use hierarchy::HierarchyError;
pub use hierarchy::HierarchyNode;
pub use hierarchy::HierarchyNodeId;
pub use hierarchy::NewHierarchyNode;
pub use hierarchy::NodeKind;
pub use hierarchy::NodeLifecycle;
pub use hierarchy::ProjectRelativePath;
pub use hierarchy::RegionAnchor;
