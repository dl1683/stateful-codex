use std::collections::HashSet;

use crate::ContextMapCoverage;
use crate::ContextMapEntryId;
use crate::ContextMapEntryUpdate;
use crate::HierarchyNodeId;
use crate::HierarchyRegionSourceUpdate;
use crate::HierarchySourceUpdate;
use crate::NewContextMapEntry;
use crate::NewHierarchyNode;
use crate::NodeKind;
use crate::NodeLifecycle;
use crate::ProjectRelativePath;
use crate::RegionAnchor;

use super::ProjectIndexer;
use super::ProjectIndexerError;
use super::scan::ScannedFile;
use super::stable_id;
use super::stable_id_text;

const LINES_PER_REGION: usize = 64;
const MAX_REGIONS_PER_FILE: usize = 64;
pub(super) const MAX_REGION_DESCRIPTION_BYTES: usize = 4_096;
const MAX_ROUTING_TERMS: usize = 32;
const MAX_ROUTING_TERM_BYTES: usize = 128;
pub(super) const MAX_PROJECT_REGIONS: usize = 50_000;

pub(super) struct ScannedRegion {
    pub(super) start_line: usize,
    pub(super) end_line: usize,
    pub(super) description: String,
    pub(super) routing_terms: Vec<String>,
    pub(super) coverage: ContextMapCoverage,
}

pub(super) fn scan_regions(
    relative_path: &str,
    text: Option<&str>,
    exact_text: bool,
) -> (Vec<ScannedRegion>, bool) {
    let Some(text) = text else {
        return (Vec::new(), false);
    };
    let lines = text.lines().collect::<Vec<_>>();
    let mut regions = Vec::new();
    let mut heading = None;
    let mut line_index = 0;
    while line_index < lines.len() && regions.len() < MAX_REGIONS_PER_FILE {
        let start_line = line_index + 1;
        let inherited_heading = heading;
        let maximum_end = (line_index + LINES_PER_REGION).min(lines.len());
        let mut end_index = line_index;
        let mut description = String::new();
        while end_index < maximum_end {
            let candidate = describe_region(
                relative_path,
                start_line,
                end_index + 1,
                inherited_heading,
                &lines[line_index..=end_index],
            );
            if candidate.len() > MAX_REGION_DESCRIPTION_BYTES && end_index > line_index {
                break;
            }
            description = candidate;
            end_index += 1;
            if description.len() > MAX_REGION_DESCRIPTION_BYTES {
                break;
            }
        }
        let complete = exact_text && description.len() <= MAX_REGION_DESCRIPTION_BYTES;
        description.truncate(description.floor_char_boundary(MAX_REGION_DESCRIPTION_BYTES));
        heading = lines[line_index..end_index]
            .iter()
            .rev()
            .find_map(|line| line.trim().starts_with('#').then(|| line.trim()))
            .or(heading);
        regions.push(ScannedRegion {
            start_line,
            end_line: end_index,
            routing_terms: routing_terms(relative_path, inherited_heading),
            description,
            coverage: if complete {
                ContextMapCoverage::Complete
            } else {
                ContextMapCoverage::Partial
            },
        });
        line_index = end_index;
    }
    (regions, line_index < lines.len())
}

fn describe_region(
    relative_path: &str,
    start_line: usize,
    end_line: usize,
    inherited_heading: Option<&str>,
    lines: &[&str],
) -> String {
    let mut description = format!("{relative_path}:{start_line}-{end_line}");
    if let Some(heading) = inherited_heading {
        description.push_str(" | ");
        description.push_str(heading);
    }
    description.push_str(" | ");
    description.push_str(&lines.join("\n"));
    description
}

pub(super) async fn sync_file_regions(
    indexer: &ProjectIndexer,
    project_id: &str,
    file_id: &HierarchyNodeId,
    file: &ScannedFile,
) -> Result<(), ProjectIndexerError> {
    let mut seen = HashSet::with_capacity(file.regions.len());
    for (cell_index, region) in file.regions.iter().enumerate() {
        let locator = format!("{}-{}", region.start_line, region.end_line);
        let cell = cell_index.to_string();
        let region_id = stable_id(
            "region",
            &[project_id, &file.project_root, &file.relative_path, &cell],
        )?;
        upsert_region_node(indexer, project_id, file_id, file, &region_id, &locator).await?;
        upsert_region_context(indexer, project_id, file, region, &region_id, &cell).await?;
        seen.insert(region_id);
    }
    for child in indexer.hierarchy.list_children(project_id, file_id).await? {
        if child.value.kind == NodeKind::Region
            && child.lifecycle == NodeLifecycle::Active
            && !seen.contains(&child.id)
        {
            indexer
                .hierarchy
                .update_source_state(
                    project_id,
                    &child.id,
                    HierarchySourceUpdate {
                        expected_revision: child.revision,
                        lifecycle: NodeLifecycle::Missing,
                        source_fingerprint: child.value.source_fingerprint,
                    },
                )
                .await?;
        }
    }
    Ok(())
}

pub(super) async fn mark_file_regions_missing(
    indexer: &ProjectIndexer,
    project_id: &str,
    file_id: &HierarchyNodeId,
) -> Result<(), ProjectIndexerError> {
    for child in indexer.hierarchy.list_children(project_id, file_id).await? {
        if child.value.kind == NodeKind::Region && child.lifecycle == NodeLifecycle::Active {
            indexer
                .hierarchy
                .update_source_state(
                    project_id,
                    &child.id,
                    HierarchySourceUpdate {
                        expected_revision: child.revision,
                        lifecycle: NodeLifecycle::Missing,
                        source_fingerprint: child.value.source_fingerprint,
                    },
                )
                .await?;
        }
    }
    Ok(())
}

async fn upsert_region_node(
    indexer: &ProjectIndexer,
    project_id: &str,
    file_id: &HierarchyNodeId,
    file: &ScannedFile,
    region_id: &HierarchyNodeId,
    locator: &str,
) -> Result<(), ProjectIndexerError> {
    let value = NewHierarchyNode {
        project_id: project_id.to_string(),
        parent_id: Some(file_id.clone()),
        kind: NodeKind::Region,
        project_root: Some(file.project_root.clone()),
        relative_path: ProjectRelativePath::parse(&file.relative_path)?,
        region_anchor: Some(RegionAnchor::new("lines", locator)?),
        source_fingerprint: Some(file.fingerprint.clone()),
    };
    let Some(existing) = indexer.hierarchy.get_node(project_id, region_id).await? else {
        indexer
            .hierarchy
            .create_node(region_id.clone(), value)
            .await?;
        return Ok(());
    };
    let current_anchor = value.region_anchor.clone();
    let mut identity = value;
    identity.region_anchor = existing.value.region_anchor.clone();
    identity.source_fingerprint = existing.value.source_fingerprint.clone();
    if existing.value != identity {
        return Err(ProjectIndexerError::IdentityConflict(region_id.to_string()));
    }
    if existing.value.region_anchor.as_ref() != current_anchor.as_ref()
        || existing.lifecycle != NodeLifecycle::Active
        || existing.value.source_fingerprint.as_ref() != Some(&file.fingerprint)
    {
        indexer
            .hierarchy
            .update_region_source(
                project_id,
                region_id,
                HierarchyRegionSourceUpdate {
                    expected_revision: existing.revision,
                    lifecycle: NodeLifecycle::Active,
                    region_anchor: RegionAnchor::new("lines", locator)?,
                    source_fingerprint: file.fingerprint.clone(),
                },
            )
            .await?;
    }
    Ok(())
}

async fn upsert_region_context(
    indexer: &ProjectIndexer,
    project_id: &str,
    file: &ScannedFile,
    region: &ScannedRegion,
    region_id: &HierarchyNodeId,
    cell: &str,
) -> Result<(), ProjectIndexerError> {
    let id = ContextMapEntryId::parse(stable_id_text(
        "context",
        &[
            project_id,
            &file.project_root,
            &file.relative_path,
            "cell",
            cell,
        ],
    ))?;
    let value = NewContextMapEntry {
        project_id: project_id.to_string(),
        node_id: region_id.clone(),
        source_fingerprint: file.fingerprint.clone(),
        description: region.description.clone(),
        routing_terms: region.routing_terms.clone(),
        coverage: region.coverage,
    };
    let Some(existing) = indexer.context_map.get_entry(project_id, &id).await? else {
        indexer.context_map.create_entry(id, value).await?;
        return Ok(());
    };
    if existing.value != value {
        indexer
            .context_map
            .update_entry(
                project_id,
                &id,
                ContextMapEntryUpdate {
                    expected_revision: existing.revision,
                    source_fingerprint: value.source_fingerprint,
                    description: value.description,
                    routing_terms: value.routing_terms,
                    coverage: value.coverage,
                },
            )
            .await?;
    }
    Ok(())
}

fn routing_terms(relative_path: &str, heading: Option<&str>) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for token in std::iter::once(relative_path)
        .chain(heading)
        .flat_map(|source| {
            source.split(|character: char| {
                !(character.is_alphanumeric() || matches!(character, '_' | '-'))
            })
        })
    {
        let token = token.trim();
        if token.len() < 2 || token.len() > MAX_ROUTING_TERM_BYTES {
            continue;
        }
        let normalized = token.to_lowercase();
        if seen.insert(normalized.clone()) {
            terms.push(normalized);
            if terms.len() == MAX_ROUTING_TERMS {
                break;
            }
        }
    }
    terms
}
