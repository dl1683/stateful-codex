use std::collections::HashMap;
use std::sync::Arc;

use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::ContextMapStoreError;
use codex_project_intelligence::HierarchyNodeId;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::HierarchyStoreError;
use codex_project_intelligence::ProjectIndexer;
use codex_state::SqliteConfig;
use codex_stateful_runtime::StatefulRunStore;
use codex_stateful_runtime::StatefulRunStoreError;
use tokio::sync::OnceCell;

use crate::read_receipts::EvidenceReadReceipts;

#[derive(Clone)]
pub(super) struct ProjectIntelligenceServices {
    sqlite: SqliteConfig,
    blackboard: Arc<OnceCell<BlackboardStore>>,
    context_map: Arc<OnceCell<ContextMapStore>>,
    hierarchy: Arc<OnceCell<HierarchyStore>>,
    read_receipts: EvidenceReadReceipts,
    runtime: Arc<OnceCell<StatefulRunStore>>,
    /// One on-demand index per project, so concurrent first queries index it once and the
    /// others wait for that publication.
    on_demand_index: Arc<std::sync::Mutex<HashMap<String, Arc<OnceCell<()>>>>>,
}

impl ProjectIntelligenceServices {
    pub(super) fn new(sqlite: SqliteConfig) -> Self {
        Self {
            sqlite,
            blackboard: Arc::new(OnceCell::new()),
            context_map: Arc::new(OnceCell::new()),
            hierarchy: Arc::new(OnceCell::new()),
            read_receipts: EvidenceReadReceipts::default(),
            runtime: Arc::new(OnceCell::new()),
            on_demand_index: Arc::default(),
        }
    }

    pub(super) async fn blackboard(&self) -> Result<&BlackboardStore, BlackboardStoreError> {
        self.blackboard
            .get_or_try_init(|| BlackboardStore::open(&self.sqlite))
            .await
    }

    pub(super) async fn context_map(&self) -> Result<&ContextMapStore, ContextMapStoreError> {
        self.context_map
            .get_or_try_init(|| ContextMapStore::open(&self.sqlite))
            .await
    }

    pub(super) async fn hierarchy(&self) -> Result<&HierarchyStore, HierarchyStoreError> {
        self.hierarchy
            .get_or_try_init(|| HierarchyStore::open(&self.sqlite))
            .await
    }

    pub(super) fn read_receipts(&self) -> &EvidenceReadReceipts {
        &self.read_receipts
    }

    pub(super) fn on_demand_index(&self, project_id: &str) -> Arc<OnceCell<()>> {
        self.on_demand_index
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(project_id.to_string())
            .or_default()
            .clone()
    }

    /// The project's hierarchy node, created without a source scan when the project was
    /// never indexed, so a first write neither fails nor waits for a full refresh.
    pub(super) async fn project_node_id(
        &self,
        project_id: &str,
    ) -> Result<HierarchyNodeId, String> {
        let hierarchy = self.hierarchy().await.map_err(|error| error.to_string())?;
        if let Some(node) = hierarchy
            .project_node(project_id)
            .await
            .map_err(|error| error.to_string())?
        {
            return Ok(node.id);
        }
        let context_map = self
            .context_map()
            .await
            .map_err(|error| error.to_string())?;
        ProjectIndexer::new(hierarchy.clone(), context_map.clone())
            .ensure_project_node(project_id)
            .await
            .map(|node| node.id)
            .map_err(|error| error.to_string())
    }

    pub(super) async fn runtime(&self) -> Result<&StatefulRunStore, StatefulRunStoreError> {
        self.runtime
            .get_or_try_init(|| StatefulRunStore::open(&self.sqlite))
            .await
    }
}
