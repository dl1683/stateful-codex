use codex_project_store::DomainManifest;

/// Actual compiled intelligence ownership, without opening a database or home.
/// Canonical readers and trusted installers share this migration definition.
pub fn project_store_domain() -> DomainManifest<'static> {
    DomainManifest {
        domain: "project-intelligence",
        migrations: &crate::blackboard_storage::MIGRATOR,
    }
}
