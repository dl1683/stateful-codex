use super::ProposalBatch;
use serde_json::json;

#[test]
fn c3_proposal_decode_refuses_model_authority_identity_and_fabricated_source_fields() {
    let request = json!({"type":"sourceProposal","records":[{
        "sourceId":"s".repeat(64),"digest":"a".repeat(64),"sourceRevision":1,"partIndex":0,
        "spans":[{"startByte":0,"endByte":1,"role":"body"}],
        "category":"decision","interpretation":"A proposed reading"
    }]});
    assert!(serde_json::from_value::<ProposalBatch>(request.clone()).is_ok());
    for (field, value) in [
        ("authority", json!("humanDirect")),
        ("verification", json!("userConfirmed")),
        ("rootPromotion", json!("promoted")),
        ("actionId", json!("model-action")),
        ("generation", json!(2)),
        ("sourceOrder", json!(1)),
        ("scopeId", json!("invented-scope")),
        ("speaker", json!("invented-person")),
        ("userQuote", json!("invented quote")),
        ("origin", json!("assistant")),
        ("quote", json!("summary or compaction")),
    ] {
        let mut candidate = request.clone();
        candidate["records"][0][field] = value;
        assert!(
            serde_json::from_value::<ProposalBatch>(candidate).is_err(),
            "accepted {field}"
        );
    }
    for field in ["authority", "rootPromotion", "actionId", "relations"] {
        let mut candidate = request.clone();
        candidate[field] = json!("unsupported");
        assert!(
            serde_json::from_value::<ProposalBatch>(candidate).is_err(),
            "accepted batch {field}"
        );
    }
}
