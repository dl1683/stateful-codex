use super::*;
use crate::services::ProjectIntelligenceServices;
use crate::tools::blackboard_write::BlackboardBatchRecordTool;
use crate::tools::capture_repair_tests::call;
use crate::visible_root::VisibleRootRegistry;
use codex_extension_api::ToolCallSource;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolPayload;
use codex_state::SqliteConfig;
use codex_thread_store::InMemoryThreadStore;
use codex_utils_absolute_path::test_support::PathExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::alloc::GlobalAlloc;
use std::alloc::Layout;
use std::alloc::System;
use std::cell::Cell;
use std::future::Future;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;
use tempfile::TempDir;

thread_local! {
    static ALLOCATED: Cell<Option<usize>> = const { Cell::new(/*value*/ None) };
}

struct CountingAllocator;

// Count requested bytes only on the measured executor-poll thread. Input and
// fixture construction happen before measurement; other concurrent tests do not
// affect it. Count reallocations in full rather than just their growth.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.with(|count| count.set(count.get().map(|bytes| bytes + layout.size())));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATED.with(|count| count.set(count.get().map(|bytes| bytes + size)));
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[tokio::test]
async fn c3r3_oversized_proposal_executor_allocations_are_bounded_before_decode() {
    let home = TempDir::new().unwrap();
    let services =
        ProjectIntelligenceServices::new(SqliteConfig::new_for_testing(home.path().abs()));
    let store = services.blackboard().await.unwrap();
    let before = store.project_revision("project-1").await.unwrap();
    let tool = BlackboardBatchRecordTool::new(
        "project-1".into(),
        "thread-1".into(),
        services.clone(),
        Arc::new(InMemoryThreadStore::default()),
        /*event_sink*/ None,
        VisibleRootRegistry::default(),
    );
    let record = json!({"sourceId":"s".repeat(/*n*/ 64),"digest":"a".repeat(/*n*/ 64),
        "sourceRevision":1,"partIndex":0,"spans":[{"startByte":0,"endByte":1,"role":"body"}],
        "category":"background","interpretation":"x"});
    for size in [1048576, 8388608] {
        let mut large_record = record.clone();
        large_record["interpretation"] = json!("x".repeat(size));
        for records in [
            json!([large_record]),
            json!(vec![record.clone(); size / 256]),
        ] {
            let raw = json!({"records":records,"type":"sourceProposal"}).to_string();
            assert!(raw.len() > size);
            for budget in [20, 9000] {
                let mut call = call(
                    "blackboard_record_batch",
                    json!({}),
                    budget,
                    ToolCallSource::Direct,
                );
                call.payload = ToolPayload::Function {
                    arguments: raw.clone(),
                };
                let allowance = call.response_byte_budget(super::super::MAX_RESPONSE_BYTES);
                let future = tool.handle(call);
                let mut future = std::pin::pin!(future);
                let mut context = Context::from_waker(Waker::noop());
                ALLOCATED.with(|count| count.set(Some(0)));
                let result = future.as_mut().poll(&mut context);
                let allocated = ALLOCATED.with(|count| count.replace(/*val*/ None).unwrap());
                let Poll::Ready(Err(FunctionCallError::RespondToModel(message))) = result else {
                    panic!("oversized refusal must finish before any asynchronous store work")
                };
                assert!(
                    allocated <= 1024,
                    "{size}-byte witness allocated {allocated} bytes"
                );
                eprintln!(
                    "C3r3 allocation witness: input={} allocated={allocated} allowance={allowance}",
                    raw.len()
                );
                assert!(serde_json::to_string(&message).unwrap().len() <= allowance);
                assert_eq!(
                    message,
                    if budget == 20 {
                        "budget_insufficient"
                    } else {
                        "source proposal exceeds the 32 KiB input bound; nothing written"
                    }
                );
            }
        }
    }
    assert_eq!(store.project_revision("project-1").await.unwrap(), before);
}

#[test]
fn c3r3_discriminator_scans_only_top_level_keys_with_bounded_key_decoding() {
    for input in [
        r#"{"type":"sourceProposal","records":[]}"#,
        r#"{"records":[{"interpretation":"type, [\" ]}"}],"type":null}"#,
        r#"{"records":[],"\u0074\u0079\u0070\u0065":"sourceProposal"}"#,
        r#"{"type":null,"type":"sourceProposal"}"#,
    ] {
        assert!(has_discriminator(input), "{input}");
    }
    for input in [
        r#"{"records":[{"type":"sourceProposal"}]}"#,
        r#"{"records":[],"content":"type"}"#,
        r#"[ {"type":"sourceProposal"} ]"#,
        r#"{"records":[],"types":"sourceProposal"}"#,
        r#"{"records":[],"broken":"trailing\"#,
    ] {
        assert!(!has_discriminator(input), "{input}");
    }
    let large_key = "\\u0078".repeat(/*n*/ 1048576);
    let input = format!("{{\"{large_key}\":[],\"type\":\"sourceProposal\"}}");
    ALLOCATED.with(|count| count.set(Some(0)));
    let detected = has_discriminator(&input);
    let allocated = ALLOCATED.with(|count| count.replace(/*val*/ None).unwrap());
    assert!(detected);
    assert_eq!(allocated, 0, "escaped key");
    let input = format!(
        "{{{}\"type\":\"sourceProposal\"}}",
        "\"a\":0,".repeat(/*n*/ 100000)
    );
    ALLOCATED.with(|count| count.set(Some(0)));
    let detected = has_discriminator(&input);
    let allocated = ALLOCATED.with(|count| count.replace(/*val*/ None).unwrap());
    assert!(detected);
    assert_eq!(allocated, 0, "many short keys");
    for mask in 0..16 {
        let mut key = String::new();
        for (index, ch) in "type".chars().enumerate() {
            if mask & (1 << index) == 0 {
                key.push(ch);
            } else {
                key.push_str(&format!("\\u{:04x}", u32::from(ch)));
            }
        }
        let input = format!("{{\"{key}\":null}}");
        assert!(has_discriminator(&input), "{input}");
        assert!(
            serde_json::from_str::<serde_json::Value>(&input)
                .unwrap()
                .get("type")
                .is_some()
        );
    }
}
