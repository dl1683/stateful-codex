//! One page of a requested-kind recall under the response byte budget: whole items in
//! order; an item no page could hold whole is cut to what fits (and marked incomplete,
//! with its whole-content route), and an item whose metadata alone cannot fit is passed
//! with a count, so every page makes progress and the cursor never repeats.

use std::collections::HashSet;

use codex_project_intelligence::BlackboardEntry;
use serde_json::Value;
use serde_json::json;

/// What one page placed.
pub(super) struct Page {
    /// The first item not on this page.
    pub(super) next: usize,
    /// Entries shown on this page (whole or cut).
    pub(super) shown: HashSet<String>,
    /// Entries shown cut, not whole.
    pub(super) cut: Vec<String>,
    /// Items passed because not even their metadata fit.
    pub(super) skipped: usize,
}

/// Places `items[start..]` into `result["requested"]["items"]` while the serialized result
/// stays within `limit` bytes.
pub(super) fn fill_page(result: &mut Value, items: &[Value], start: usize, limit: usize) -> Page {
    let mut page = Page {
        next: start,
        shown: HashSet::new(),
        cut: Vec::new(),
        skipped: 0,
    };
    for item in &items[start..] {
        let id = item["entryId"].as_str().unwrap_or_default().to_string();
        if push_if_fits(result, item.clone(), limit) {
            page.shown.insert(id);
            page.next += 1;
            continue;
        }
        if !page.shown.is_empty() {
            break;
        }
        // No page can hold this item whole: the longest cut of its content that fits.
        let content = item["content"].as_str().unwrap_or_default();
        let cut_item = |bytes: usize| {
            let mut cut = item.clone();
            cut["content"] = json!(prefix(content, bytes));
            cut["contentComplete"] = json!(false);
            cut["wholeContent"] = json!(format!("memory_read with entryId {id}"));
            cut
        };
        let (mut low, mut high) = (0usize, content.len());
        while low < high {
            let middle = (low + high).div_ceil(2);
            if serialized_with(result, cut_item(middle)) <= limit {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        page.next += 1;
        if push_if_fits(result, cut_item(low), limit) {
            page.shown.insert(id.clone());
            page.cut.push(id);
            break;
        }
        page.skipped += 1;
    }
    page
}

fn push_if_fits(result: &mut Value, item: Value, limit: usize) -> bool {
    if let Some(items) = result["requested"]["items"].as_array_mut() {
        items.push(item);
    }
    if result.to_string().len() <= limit {
        return true;
    }
    if let Some(items) = result["requested"]["items"].as_array_mut() {
        items.pop();
    }
    false
}

fn serialized_with(result: &mut Value, item: Value) -> usize {
    if let Some(items) = result["requested"]["items"].as_array_mut() {
        items.push(item);
    }
    let length = result.to_string().len();
    if let Some(items) = result["requested"]["items"].as_array_mut() {
        items.pop();
    }
    length
}

/// One entry as `memory_read` with `entryId` returns it: whole when `fits` accepts it,
/// otherwise its longest prefix that fits, marked incomplete.
pub(super) fn whole_entry_result(entry: &BlackboardEntry, fits: impl Fn(&Value) -> bool) -> Value {
    let content = &entry.value.content;
    let mut result = json!({
        "entryId": entry.id.to_string(),
        "revision": entry.revision,
        "kind": entry.value.kind,
        "state": entry.state,
        "content": content,
        "contentComplete": true,
    });
    if fits(&result) {
        return result;
    }
    let (mut low, mut high) = (0usize, content.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        result["content"] = json!(prefix(content, middle));
        if fits(&result) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    result["content"] = json!(prefix(content, low));
    result["contentComplete"] = json!(false);
    result
}

/// At most `bytes` bytes of `text`, cut on a character boundary, with "..." when cut.
fn prefix(text: &str, bytes: usize) -> String {
    if text.len() <= bytes {
        return text.to_string();
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}
