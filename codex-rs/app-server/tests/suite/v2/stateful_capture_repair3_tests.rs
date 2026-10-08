//! Delivered-root and route-coverage witnesses for the final C2 exclusions.
use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn c2r3_public_corrected_successor_root_preserves_history_after_forget_and_reopen()
-> Result<()> {
    for (words, direct_words) in [
        ("QX704", "QX704"),
        ("🛑", "🛑"),
        (
            "Old linked meridian note.",
            "Independent linked source fence.",
        ),
    ] {
        let (home, mut server, project, thread, responses_server) = setup().await?;
        let recorded = model_call(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_record_batch",
            json!({"records":[record("original", words)]}),
        )
        .await?;
        assert_eq!(recorded["recorded"], json!(1));
        let sqlite = SqliteConfig::new_for_testing(home.path().abs());
        let store = pi::BlackboardStore::open(&sqlite).await?;
        let original = store
            .root_projection(pi::RootBlackboardQuery {
                project_id: project.clone(),
                max_entries: 256,
            })
            .await?
            .data
            .remove(0)
            .entry;
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join("project_intelligence_1.sqlite"))
            .await?;
        if words != direct_words {
            // Seed valid links to the real captured ingress part. Public Forget below
            // must exclude that part without needing a wording match on the predecessor.
            let linked = sqlx::query("INSERT INTO capture_entry_sources SELECT ?, source.source_id, 0, chunk.end_byte, '\"evidence\"' FROM capture_sources AS source JOIN capture_source_chunks AS chunk ON chunk.source_id = source.source_id AND chunk.start_byte = 0 WHERE source.project_id = ? LIMIT 1")
                .bind(original.id.as_str()).bind(&project).execute(&pool).await?;
            assert_eq!(linked.rows_affected(), 1);
        }
        let corrected: StatefulMemoryCorrectResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryCorrect {
                request_id,
                params: StatefulMemoryCorrectParams {
                    expected_project_id: project.clone(),
                    thread_id: thread.clone(),
                    entry_id: original.id.to_string(),
                    expected_revision: original.revision,
                    content: "Keep the Cedar kit.".to_string(),
                    background_section: true,
                },
            })
            .await?;
        let direct: StatefulMemoryAddResponse = server
            .request(|request_id| ClientRequest::StatefulMemoryAdd {
                request_id,
                params: StatefulMemoryAddParams {
                    expected_project_id: project.clone(),
                    thread_id: thread.clone(),
                    kind: StatefulMemoryAddKind::Note,
                    content: direct_words.to_string(),
                    scope: None,
                    reason: None,
                    client_action_id: "fresh-direct".to_string(),
                    background_section: true,
                },
            })
            .await?;
        if words != direct_words {
            sqlx::query("INSERT INTO capture_entry_sources SELECT ?, source_id, start_byte, end_byte, role FROM capture_entry_sources WHERE entry_id = ?")
                .bind(&direct.item.entry_id).bind(original.id.as_str()).execute(&pool).await?;
        }
        forget(
            &mut server,
            &project,
            &thread,
            &direct.item.entry_id,
            direct.item.revision,
        )
        .await?;
        if words != direct_words {
            let exclusions: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM capture_source_exclusions WHERE entry_id = ?",
            )
            .bind(&direct.item.entry_id)
            .fetch_one(&pool)
            .await?;
            assert_eq!(exclusions, 1);
        }
        assert!(!store.entry_source_eligible(&project, &original.id).await?);
        pool.close().await;
        let archived = store.get_entry(&project, &original.id).await?.unwrap();
        assert_eq!(
            (archived.value.content.as_str(), archived.state),
            (words, pi::BlackboardEntryState::Superseded)
        );
        for attempt in 0..2 {
            let log = responses::mount_sse_once(
                &responses_server,
                responses::sse(vec![responses::ev_completed("root-witness")]),
            )
            .await;
            run_turn(
                &mut server,
                &thread,
                "Inspect the current project knowledge.",
            )
            .await?;
            let packet = log.single_request().body_json();
            let roots = packet["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["role"] == "developer")
                .filter_map(|item| item["content"].as_array())
                .flatten()
                .filter_map(|part| part["text"].as_str())
                .filter(|text| {
                    text.trim().starts_with("<stateful_project>")
                        || text.trim().starts_with("<stateful_project_update>")
                })
                .collect::<Vec<_>>();
            let root = roots.last().expect("current delivered root or amendment");
            assert!(root.contains(&corrected.item.content), "{root}");
            assert!(!root.contains(words), "{root}");
            assert_eq!(
                store.get_entry(&project, &original.id).await?,
                Some(archived.clone())
            );
            if attempt == 0 {
                reopen(&mut server, &home, &thread).await?;
            }
        }
        assert!(server.shutdown_gracefully().await?.success());
    }
    Ok(())
}
