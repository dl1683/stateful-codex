use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn c3r3_public_large_proposal_string_and_record_array_refuse_without_writes_cold()
-> Result<()> {
    let (home, mut server, project, thread, responses_server) = setup().await?;
    let sqlite = SqliteConfig::new_for_testing(home.path().abs());
    let before = snapshot(&sqlite).await?;
    let proposal_record = json!({"sourceId":"s".repeat(/*n*/ 64),"digest":"a".repeat(/*n*/ 64),
        "sourceRevision":1,"partIndex":0,"spans":[{"startByte":0,"endByte":1,"role":"body"}],
        "category":"background","interpretation":"x"});
    let mut oversized = proposal_record.clone();
    oversized["interpretation"] = json!("x".repeat(/*n*/ 1048576));
    for arguments in [
        json!({"type":"sourceProposal","records":[oversized]}),
        json!({"records":vec![proposal_record;4096],"type":"sourceProposal"}),
    ] {
        assert!(arguments.to_string().len() > 1048576);
        let output = model_output(
            &mut server,
            &responses_server,
            &thread,
            "blackboard_record_batch",
            arguments,
        )
        .await?;
        assert_eq!(
            output,
            "source proposal exceeds the 32 KiB input bound; nothing written"
        );
        assert!(serde_json::to_string(&output)?.len() <= 9000);
        assert_eq!(snapshot(&sqlite).await?, before);
    }
    reopen(&mut server, &home, &thread).await?;
    assert_eq!(snapshot(&sqlite).await?, before);
    // The retained ordinary Agent control remains usable after the refusals.
    let result = model_call(
        &mut server,
        &responses_server,
        &thread,
        "blackboard_record_batch",
        json!({"records":[record("control", "ordinary Agent control")]}),
    )
    .await?;
    assert_eq!(
        (result["recorded"].clone(), result["failed"].clone()),
        (json!(1), json!(0))
    );
    assert!(
        pi::BlackboardStore::open(&sqlite)
            .await?
            .get_entry(
                &project,
                &pi::BlackboardEntryId::parse(result["results"][0]["entryId"].as_str().unwrap())?
            )
            .await?
            .is_some()
    );
    Ok(())
}
