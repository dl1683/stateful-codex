//! Pinned validation identities, separate from the migrations applied to a store.

use sqlx::SqlSafeStr;
use sqlx::migrate::Migration;
use sqlx::migrate::MigrationType;

use crate::migrations::STATE_MIGRATOR;

pub(super) fn canonical(version: i64) -> Option<Migration> {
    let (description, sql) = match version {
        1..=58 => {
            return STATE_MIGRATOR
                .iter()
                .find(|migration| migration.version == version)
                .cloned();
        }
        59 => (
            "thread attachment reverse lookup",
            include_str!("../../migration_catalog/0059_thread_attachment_reverse_lookup.sql"),
        ),
        60 => (
            "guardian review feedback",
            include_str!("../../migration_catalog/0060_guardian_review_feedback.sql"),
        ),
        _ => return None,
    };
    Some(Migration::new(
        version,
        description.into(),
        MigrationType::Simple,
        sql.into_sql_str(),
        /*no_tx*/ false,
    ))
}

pub(super) fn fork(version: i64) -> Option<Migration> {
    let (description, sql) = match version {
        59 => (
            "backfill owner token",
            include_str!("../../migration_catalog/0059_backfill_owner_token.sql"),
        ),
        60 => (
            "thread project generation",
            include_str!("../../migration_catalog/0060_thread_project_generation.sql"),
        ),
        _ => return None,
    };
    Some(Migration::new(
        version,
        description.into(),
        MigrationType::Simple,
        sql.into_sql_str(),
        /*no_tx*/ false,
    ))
}
