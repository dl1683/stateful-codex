use std::sync::Arc;

use codex_project_intelligence::BlackboardStore;
use codex_project_intelligence::BlackboardStoreError;
use codex_project_intelligence::ContextMapStore;
use codex_project_intelligence::ContextMapStoreError;
use codex_project_intelligence::HierarchyStore;
use codex_project_intelligence::HierarchyStoreError;
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

    pub(super) async fn runtime(&self) -> Result<&StatefulRunStore, StatefulRunStoreError> {
        self.runtime
            .get_or_try_init(|| StatefulRunStore::open(&self.sqlite))
            .await
    }
}
