//! Model-delivered usable route coverage.
use super::*;
use pretty_assertions::assert_eq;
use sha2::Digest;

async fn route_query(
    server: &mut TestAppServer,
    responses_server: &wiremock::MockServer,
    thread: &str,
    text: &str,
) -> Result<Value> {
    let output = model_output(
        server,
        responses_server,
        thread,
        "context_map_query",
        json!({"text":text}),
    )
    .await?;
    serde_json::from_str(&output).map_err(|error| anyhow::anyhow!("{error}: {output}"))
}

#[tokio::test]
async fn c2r3_public_route_counts_eligible_structured_knowledge_and_reports_upgrade_unavailable()
-> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let _: codex_app_server_protocol::ProjectUpdateResponse = server
        .request(|request_id| ClientRequest::ProjectUpdate {
            request_id,
            params: codex_app_server_protocol::ProjectUpdateParams {
                project_id: project.clone(),
                name: None,
                metadata: None,
                roots: Some(vec![codex_app_server_protocol::ProjectRoot {
                    path: home.path().abs(),
                }]),
            },
        })
        .await?;
    let seeded = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[record("template", "Template agent note.")]}),
    )
    .await?;
    assert_eq!(seeded["recorded"], json!(1));
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let store = pi::BlackboardStore::open(&sqlite).await?;
    let template = store
        .root_projection(pi::RootBlackboardQuery {
            project_id: project.clone(),
            max_entries: 256,
        })
        .await?
        .data
        .remove(0)
        .entry
        .value;
    let hierarchy = pi::HierarchyStore::open(&sqlite).await?;
    let directory = pi::HierarchyNodeId::parse("evidence-directory")?;
    hierarchy
        .create_node(
            directory.clone(),
            pi::NewHierarchyNode {
                project_id: project.clone(),
                parent_id: Some(template.node_id.clone()),
                kind: pi::NodeKind::Directory,
                project_root: Some(home.path().to_string_lossy().into_owned()),
                relative_path: pi::ProjectRelativePath::root(),
                region_anchor: None,
                source_fingerprint: None,
            },
        )
        .await?;
    let file = pi::HierarchyNodeId::parse("evidence-file")?;
    let bytes = b"Fixture evidence.\n";
    std::fs::write(home.path().join("evidence.txt"), bytes)?;
    let fingerprint =
        pi::SourceFingerprint::parse(format!("sha256:{:x}", sha2::Sha256::digest(bytes)))?;
    hierarchy
        .create_node(
            file.clone(),
            pi::NewHierarchyNode {
                project_id: project.clone(),
                parent_id: Some(directory),
                kind: pi::NodeKind::File,
                project_root: Some(home.path().to_string_lossy().into_owned()),
                relative_path: pi::ProjectRelativePath::parse("evidence.txt")?,
                region_anchor: None,
                source_fingerprint: Some(fingerprint.clone()),
            },
        )
        .await?;
    let evidence_id = pi::ContextMapEntryId::parse("evidence-context")?;
    pi::ContextMapStore::open(&sqlite)
        .await?
        .create_entry(
            evidence_id.clone(),
            pi::NewContextMapEntry {
                project_id: project.clone(),
                node_id: file,
                source_fingerprint: fingerprint.clone(),
                description: "Fixture evidence".to_string(),
                routing_terms: vec!["fixture".to_string()],
                coverage: pi::ContextMapCoverage::Complete,
            },
        )
        .await?;
    for (id, value) in [("copy", "QX704"), ("control", "18")] {
        let mut entry = template.clone();
        entry.content = "Purchase receipt identifier.".to_string();
        entry.structured_value = Some(pi::BlackboardStructuredValue {
            value: value.to_string(),
            unit: None,
        });
        entry.evidence = vec![pi::BlackboardEvidenceLink {
            context_map_entry_id: evidence_id.clone(),
            source_fingerprint: fingerprint.clone(),
            line_range: None,
        }];
        store
            .create_entry(pi::BlackboardEntryId::parse(id)?, entry)
            .await?;
    }
    let initial = route_query(&mut server, &responses_server, &thread, "fixture").await?;
    assert_eq!(initial["data"][0]["freshness"], json!("current"));
    assert_eq!(
        initial["data"][0]["knownKnowledge"],
        json!({"rootEntries":2,"deeperEntries":0})
    );
    let direct: StatefulMemoryAddResponse = server
        .request(|request_id| ClientRequest::StatefulMemoryAdd {
            request_id,
            params: StatefulMemoryAddParams {
                expected_project_id: project.clone(),
                thread_id: thread.clone(),
                kind: StatefulMemoryAddKind::Note,
                content: "QX704".to_string(),
                scope: None,
                reason: None,
                client_action_id: "retire-value".to_string(),
                background_section: true,
            },
        })
        .await?;
    forget(
        &mut server,
        &project,
        &thread,
        &direct.item.entry_id,
        direct.item.revision,
    )
    .await?;
    for attempt in 0..2 {
        let output = route_query(&mut server, &responses_server, &thread, "fixture").await?;
        assert_eq!(
            (
                output["knowledgeCoverageAvailable"].clone(),
                output["data"][0]["knownKnowledge"].clone()
            ),
            (json!(true), json!({"rootEntries":1,"deeperEntries":0}))
        );
        assert_eq!(
            store
                .get_source_eligible_entry(&project, &pi::BlackboardEntryId::parse("copy")?)
                .await?,
            None
        );
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    let pool = sqlite
        .open_read_write_pool(&sqlite.home().join("project_intelligence_1.sqlite"))
        .await?;
    sqlx::query("INSERT INTO capture_identity_coverage(project_id, watermark) SELECT ?, MAX(rowid) FROM blackboard_entry_revisions")
        .bind(&project).execute(&pool).await?;
    for attempt in 0..2 {
        assert!(!store.maintain_capture_identities(&project).await?);
        let output = route_query(&mut server, &responses_server, &thread, "fixture").await?;
        assert_eq!(output["knowledgeCoverageAvailable"], json!(false));
        assert!(
            output["data"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item.get("knownKnowledge").is_none())
        );
        let empty = route_query(
            &mut server,
            &responses_server,
            &thread,
            "unmatchedrouteidentifier",
        )
        .await?;
        assert_eq!(
            (
                empty["data"].clone(),
                empty["knowledgeCoverageAvailable"].clone()
            ),
            (json!([]), json!(false))
        );
        if attempt == 0 {
            reopen(&mut server, &home, &thread).await?;
        }
    }
    pool.close().await;
    assert!(server.shutdown_gracefully().await?.success());
    Ok(())
}
