use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use codex_project_intelligence::ContextMapEntryId;
use codex_project_intelligence::ContextMapFreshness;
use codex_project_intelligence::ContextMapHit;
use codex_project_intelligence::HierarchySourceUpdate;
use codex_project_intelligence::NodeLifecycle;
use codex_project_intelligence::SourceFingerprint;
use sha2::Digest;
use sha2::Sha256;

use crate::services::ProjectIntelligenceServices;

const MAX_AUDITED_SOURCES: usize = 256;
const MAX_AUDITED_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_AUDITED_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SourceAuditStatus {
    Current,
    Stale,
    SourceUnavailable,
    Unchecked,
}

#[derive(Clone, Debug)]
pub(super) struct RootEvidenceAudit {
    pub(super) project_id: String,
    pub(super) statuses: HashMap<ContextMapEntryId, SourceAuditStatus>,
    pub(super) cache_key: Option<RootEvidenceAuditCacheKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RootEvidenceAuditCacheKey {
    sources: Vec<RootEvidenceAuditCacheSource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RootEvidenceAuditCacheSource {
    context_map_entry_id: ContextMapEntryId,
    project_root: String,
    relative_path: String,
    source_fingerprint: SourceFingerprint,
    filesystem_state: RootEvidenceFilesystemState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RootEvidenceFilesystemState {
    Present {
        canonical_path: PathBuf,
        bytes: u64,
        modified: SystemTime,
    },
    Missing,
}

pub(super) async fn root_evidence_audit_cache_key(
    project_roots: &[PathBuf],
    evidence_routes: &HashMap<ContextMapEntryId, ContextMapHit>,
) -> Option<RootEvidenceAuditCacheKey> {
    let project_roots = project_roots.to_vec();
    let mut sources = evidence_routes
        .iter()
        .map(|(entry_id, hit)| {
            (
                entry_id.clone(),
                hit.source.project_root.clone(),
                hit.source.relative_path.to_string(),
                hit.entry.value.source_fingerprint.clone(),
            )
        })
        .collect::<Vec<_>>();
    sources.sort_by(|left, right| left.0.as_str().cmp(right.0.as_str()));
    tokio::task::spawn_blocking(move || {
        let sources = sources
            .into_iter()
            .map(
                |(context_map_entry_id, project_root, relative_path, source_fingerprint)| {
                    let configured_root = project_roots
                        .iter()
                        .find(|root| root.as_os_str() == Path::new(&project_root).as_os_str())?;
                    let canonical_root = std::fs::canonicalize(configured_root).ok()?;
                    let source_path = canonical_root.join(&relative_path);
                    let filesystem_state = match std::fs::canonicalize(&source_path) {
                        Ok(canonical_path) => {
                            if !canonical_path.starts_with(&canonical_root) {
                                return None;
                            }
                            let metadata = std::fs::metadata(&canonical_path).ok()?;
                            RootEvidenceFilesystemState::Present {
                                canonical_path,
                                bytes: metadata.len(),
                                modified: metadata.modified().ok()?,
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            RootEvidenceFilesystemState::Missing
                        }
                        Err(_) => return None,
                    };
                    Some(RootEvidenceAuditCacheSource {
                        context_map_entry_id,
                        project_root,
                        relative_path,
                        source_fingerprint,
                        filesystem_state,
                    })
                },
            )
            .collect::<Option<Vec<_>>>()?;
        Some(RootEvidenceAuditCacheKey { sources })
    })
    .await
    .ok()
    .flatten()
}

pub(super) async fn audit_root_evidence(
    services: &ProjectIntelligenceServices,
    project_id: &str,
    project_roots: &[PathBuf],
    entry_ids: impl IntoIterator<Item = ContextMapEntryId>,
    cache_key: Option<RootEvidenceAuditCacheKey>,
) -> RootEvidenceAudit {
    let mut statuses = HashMap::new();
    let mut audited_bytes = 0_u64;
    let context_map = match services.context_map().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to open context map for source audit");
            return RootEvidenceAudit {
                project_id: project_id.to_string(),
                statuses,
                cache_key,
            };
        }
    };
    let hierarchy = match services.hierarchy().await {
        Ok(store) => store,
        Err(error) => {
            tracing::warn!(%project_id, %error, "failed to open hierarchy for source audit");
            return RootEvidenceAudit {
                project_id: project_id.to_string(),
                statuses,
                cache_key,
            };
        }
    };

    let mut entry_ids = entry_ids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    entry_ids.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    for (index, entry_id) in entry_ids.into_iter().enumerate() {
        if index >= MAX_AUDITED_SOURCES {
            statuses.insert(entry_id, SourceAuditStatus::Unchecked);
            continue;
        }
        let hit = match context_map.get_hit(project_id, &entry_id).await {
            Ok(Some(hit)) => hit,
            Ok(None) => {
                statuses.insert(entry_id, SourceAuditStatus::Unchecked);
                continue;
            }
            Err(error) => {
                tracing::warn!(
                    %project_id,
                    context_map_entry_id = %entry_id,
                    %error,
                    "failed to load source route for freshness audit"
                );
                statuses.insert(entry_id, SourceAuditStatus::Unchecked);
                continue;
            }
        };
        let remaining_bytes = MAX_TOTAL_AUDITED_BYTES.saturating_sub(audited_bytes);
        let check = check_source(project_roots, &hit, remaining_bytes).await;
        if let SourceCheck::Fingerprint(_, bytes) = &check {
            audited_bytes = audited_bytes.saturating_add(*bytes);
        }
        let status = reconcile_source_state(project_id, hierarchy, &hit, check).await;
        statuses.insert(entry_id, status);
    }

    RootEvidenceAudit {
        project_id: project_id.to_string(),
        statuses,
        cache_key,
    }
}

enum SourceCheck {
    Fingerprint(SourceFingerprint, u64),
    Missing,
    Unchecked,
}

async fn check_source(
    project_roots: &[PathBuf],
    hit: &ContextMapHit,
    remaining_bytes: u64,
) -> SourceCheck {
    let Some(configured_root) = project_roots
        .iter()
        .find(|root| root.as_os_str() == Path::new(&hit.source.project_root).as_os_str())
    else {
        return SourceCheck::Unchecked;
    };
    let root = configured_root.clone();
    let relative_path = hit.source.relative_path.to_string();
    match tokio::task::spawn_blocking(move || {
        fingerprint_source(&root, &relative_path, remaining_bytes)
    })
    .await
    {
        Ok(Ok((fingerprint, bytes))) => SourceCheck::Fingerprint(fingerprint, bytes),
        Ok(Err(SourceCheckError::Missing)) => SourceCheck::Missing,
        Ok(Err(SourceCheckError::Unchecked)) | Err(_) => SourceCheck::Unchecked,
    }
}

async fn reconcile_source_state(
    project_id: &str,
    hierarchy: &codex_project_intelligence::HierarchyStore,
    hit: &ContextMapHit,
    check: SourceCheck,
) -> SourceAuditStatus {
    let (status, lifecycle, fingerprint) = match check {
        SourceCheck::Fingerprint(fingerprint, _)
            if fingerprint == hit.entry.value.source_fingerprint =>
        {
            (
                SourceAuditStatus::Current,
                NodeLifecycle::Active,
                Some(fingerprint),
            )
        }
        SourceCheck::Fingerprint(fingerprint, _) => (
            SourceAuditStatus::Stale,
            NodeLifecycle::Replaced,
            Some(fingerprint),
        ),
        SourceCheck::Missing => (
            SourceAuditStatus::SourceUnavailable,
            NodeLifecycle::Missing,
            None,
        ),
        SourceCheck::Unchecked => return SourceAuditStatus::Unchecked,
    };
    let node = match hierarchy
        .get_node(project_id, &hit.entry.value.node_id)
        .await
    {
        Ok(Some(node)) => node,
        Ok(None) => return SourceAuditStatus::Unchecked,
        Err(error) => {
            tracing::warn!(
                %project_id,
                node_id = %hit.entry.value.node_id,
                %error,
                "failed to load source node for freshness audit"
            );
            return SourceAuditStatus::Unchecked;
        }
    };
    let fingerprint = fingerprint.or(node.value.source_fingerprint.clone());
    if node.lifecycle == lifecycle && node.value.source_fingerprint == fingerprint {
        return status;
    }
    match hierarchy
        .update_source_state(
            project_id,
            &node.id,
            HierarchySourceUpdate {
                expected_revision: node.revision,
                lifecycle,
                source_fingerprint: fingerprint,
            },
        )
        .await
    {
        Ok(_) => status,
        Err(error) => {
            tracing::warn!(
                %project_id,
                node_id = %node.id,
                %error,
                "failed to persist source freshness audit"
            );
            SourceAuditStatus::Unchecked
        }
    }
}

enum SourceCheckError {
    Missing,
    Unchecked,
}

fn fingerprint_source(
    configured_root: &Path,
    relative_path: &str,
    remaining_bytes: u64,
) -> Result<(SourceFingerprint, u64), SourceCheckError> {
    let root = std::fs::canonicalize(configured_root).map_err(classify_io_error)?;
    let source = std::fs::canonicalize(root.join(relative_path)).map_err(classify_io_error)?;
    if !source.starts_with(&root) {
        return Err(SourceCheckError::Unchecked);
    }
    let mut file = File::open(source).map_err(classify_io_error)?;
    let maximum_bytes = remaining_bytes.min(MAX_AUDITED_SOURCE_BYTES);
    if maximum_bytes == 0 || file.metadata().map_err(classify_io_error)?.len() > maximum_bytes {
        return Err(SourceCheckError::Unchecked);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_bytes = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(classify_io_error)?;
        if read == 0 {
            break;
        }
        total_bytes = total_bytes
            .checked_add(u64::try_from(read).map_err(|_| SourceCheckError::Unchecked)?)
            .ok_or(SourceCheckError::Unchecked)?;
        if total_bytes > maximum_bytes {
            return Err(SourceCheckError::Unchecked);
        }
        hasher.update(&buffer[..read]);
    }
    let fingerprint = SourceFingerprint::parse(format!("sha256:{:x}", hasher.finalize()))
        .map_err(|_| SourceCheckError::Unchecked)?;
    Ok((fingerprint, total_bytes))
}

fn classify_io_error(error: std::io::Error) -> SourceCheckError {
    if error.kind() == std::io::ErrorKind::NotFound {
        SourceCheckError::Missing
    } else {
        SourceCheckError::Unchecked
    }
}

pub(super) fn audited_context_freshness(
    audit: &RootEvidenceAudit,
    entry_id: &ContextMapEntryId,
    stored: ContextMapFreshness,
) -> Option<ContextMapFreshness> {
    match audit.statuses.get(entry_id) {
        Some(SourceAuditStatus::Current) => Some(ContextMapFreshness::Current),
        Some(SourceAuditStatus::Stale) => Some(ContextMapFreshness::Stale),
        Some(SourceAuditStatus::SourceUnavailable) => Some(ContextMapFreshness::SourceUnavailable),
        Some(SourceAuditStatus::Unchecked) | None => match stored {
            ContextMapFreshness::Stale | ContextMapFreshness::SourceUnavailable => Some(stored),
            ContextMapFreshness::Current => None,
        },
    }
}
