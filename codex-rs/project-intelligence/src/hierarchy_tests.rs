use pretty_assertions::assert_eq;

use super::*;

fn node(kind: NodeKind) -> NewHierarchyNode {
    NewHierarchyNode {
        project_id: "project-1".to_string(),
        parent_id: Some(HierarchyNodeId::parse("parent-1").expect("valid ID")),
        kind,
        project_root: Some("C:\\workspace".to_string()),
        relative_path: ProjectRelativePath::parse("src/lib.rs").expect("valid path"),
        region_anchor: None,
        source_fingerprint: Some(
            SourceFingerprint::parse("sha256:abc").expect("valid fingerprint"),
        ),
    }
}

#[test]
fn relative_paths_are_host_independent_and_cannot_escape_the_root() {
    assert_eq!(
        ProjectRelativePath::parse("src/module/file.rs")
            .expect("portable relative path")
            .as_str(),
        "src/module/file.rs"
    );
    for invalid in [
        "/absolute",
        "trailing/",
        "double//separator",
        "./same",
        "../escape",
        "nested/../escape",
        "windows\\separator",
    ] {
        assert!(ProjectRelativePath::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn hierarchy_kinds_enforce_generic_filesystem_shape() {
    let mut project = node(NodeKind::Project);
    project.parent_id = None;
    project.project_root = None;
    project.relative_path = ProjectRelativePath::root();
    project.source_fingerprint = None;
    assert_eq!(project.validate(), Ok(()));

    let mut directory = node(NodeKind::Directory);
    directory.relative_path = ProjectRelativePath::root();
    assert_eq!(directory.validate(), Ok(()));

    let file = node(NodeKind::File);
    assert_eq!(file.validate(), Ok(()));

    let mut region = node(NodeKind::Region);
    region.region_anchor =
        Some(RegionAnchor::new("symbol", "crate::module::function").expect("valid generic anchor"));
    assert_eq!(region.validate(), Ok(()));
}

#[test]
fn region_anchor_is_generic_but_bounded() {
    assert_eq!(
        RegionAnchor::new("line-range", "120:145").expect("valid anchor"),
        RegionAnchor {
            scheme: "line-range".to_string(),
            locator: "120:145".to_string(),
        }
    );
    assert!(RegionAnchor::new("LegalClause", "4.2").is_err());
    assert!(RegionAnchor::new("symbol", "").is_err());
}

#[test]
fn source_fingerprints_are_opaque_but_bounded() {
    assert_eq!(
        SourceFingerprint::parse("sha256:abc")
            .expect("valid fingerprint")
            .as_str(),
        "sha256:abc"
    );
    assert!(SourceFingerprint::parse("").is_err());
    assert!(SourceFingerprint::parse("sha256:abc\nforged").is_err());
}

#[test]
fn invalid_kind_shapes_are_rejected_as_whole_objects() {
    let mut file = node(NodeKind::File);
    file.relative_path = ProjectRelativePath::root();
    assert_eq!(file.validate(), Err(HierarchyError::InvalidFileNode));

    let region = node(NodeKind::Region);
    assert_eq!(region.validate(), Err(HierarchyError::InvalidRegionNode));

    let mut project = node(NodeKind::Project);
    project.parent_id = None;
    assert_eq!(project.validate(), Err(HierarchyError::InvalidProjectNode));
}
