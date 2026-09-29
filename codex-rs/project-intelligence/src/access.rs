use std::fmt;

use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProjectKnowledgeAccess {
    #[default]
    ReadWrite,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectKnowledgeOperation {
    HierarchyCreateNode,
    HierarchyUpdateSourceState,
    HierarchyUpdateRegionSource,
    ContextMapCreateEntry,
    ContextMapUpdateEntry,
    BlackboardCreateEntry,
    BlackboardUpdateEntry,
    BlackboardCreateRelation,
    BlackboardAcquireCompletionFence,
    ContextMapRefresh,
    ContextMapRefreshFile,
}

impl ProjectKnowledgeOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HierarchyCreateNode => "hierarchy.createNode",
            Self::HierarchyUpdateSourceState => "hierarchy.updateSourceState",
            Self::HierarchyUpdateRegionSource => "hierarchy.updateRegionSource",
            Self::ContextMapCreateEntry => "contextMap.createEntry",
            Self::ContextMapUpdateEntry => "contextMap.updateEntry",
            Self::BlackboardCreateEntry => "blackboard.createEntry",
            Self::BlackboardUpdateEntry => "blackboard.updateEntry",
            Self::BlackboardCreateRelation => "blackboard.createRelation",
            Self::BlackboardAcquireCompletionFence => "blackboard.acquireCompletionFence",
            Self::ContextMapRefresh => "contextMap.refresh",
            Self::ContextMapRefreshFile => "contextMap.refreshFile",
        }
    }
}

impl fmt::Display for ProjectKnowledgeOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
#[error("project_knowledge_read_only: operation={operation}, project_id={project_id}")]
pub struct ProjectKnowledgeReadOnlyError {
    pub operation: ProjectKnowledgeOperation,
    pub project_id: String,
}
