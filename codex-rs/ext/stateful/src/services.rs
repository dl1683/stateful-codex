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
use codex_project_intelligence::RepositoryObservationStore;
use codex_project_intelligence::RepositoryObservationStoreError;
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
    repository_observations: Arc<OnceCell<RepositoryObservationStore>>,
    /// Projects whose checkout changes could not be fully processed: no turn of any thread
    /// advances their observation baseline until a turn's start completes the comparison.
    checkout_holds: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Projects whose never-built index a read already tried to build in this process.
    index_attempts: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// One checkout reconciliation per project at a time: comparison, hold updates and
    /// baseline publication happen under this single permit.
    checkout_locks: Arc<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
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
            repository_observations: Arc::new(OnceCell::new()),
            checkout_holds: Arc::default(),
            index_attempts: Arc::default(),
            checkout_locks: Arc::default(),
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

    pub(super) async fn repository_observations(
        &self,
    ) -> Result<&RepositoryObservationStore, RepositoryObservationStoreError> {
        self.repository_observations
            .get_or_try_init(|| RepositoryObservationStore::open(&self.sqlite))
            .await
    }

    /// Holds or releases the project's checkout baseline for every thread of this process.
    pub(super) fn set_checkout_hold(&self, project_id: &str, held: bool) {
        let mut holds = self
            .checkout_holds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if held {
            holds.insert(project_id.to_string());
        } else {
            holds.remove(project_id);
        }
    }

    /// The project's checkout reconciliation permit.
    pub(super) fn checkout_lock(&self, project_id: &str) -> Arc<tokio::sync::Semaphore> {
        self.checkout_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(project_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
    }

    /// Claims the one on-demand index attempt for a project; false once claimed.
    pub(super) fn claim_index_attempt(&self, project_id: &str) -> bool {
        self.index_attempts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(project_id.to_string())
    }

    pub(super) fn checkout_held(&self, project_id: &str) -> bool {
        self.checkout_holds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(project_id)
    }

    pub(super) async fn runtime(&self) -> Result<&StatefulRunStore, StatefulRunStoreError> {
        self.runtime
            .get_or_try_init(|| StatefulRunStore::open(&self.sqlite))
            .await
    }
}
