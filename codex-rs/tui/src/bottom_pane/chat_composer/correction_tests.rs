use super::*;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc::unbounded_channel;

fn composer() -> ChatComposer {
    let (tx, _rx) = unbounded_channel();
    ChatComposer::new(
        /*has_input_focus*/ true, AppEventSender::new(tx),
        /*enhanced_keys_supported*/ false, "Ask".into(), /*disable_paste_burst*/ true,
    )
}

fn key(composer: &mut ChatComposer, code: KeyCode) -> InputResult {
    composer.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE)).0
}

fn draft(composer: &ChatComposer) -> HistoryEntry {
    HistoryEntry {
        text: composer.current_text(),
        text_elements: composer.current_text_elements(),
        local_image_paths: composer.local_image_paths(),
        remote_image_urls: composer.attachments.remote_image_urls(),
        mention_bindings: composer.snapshot_mention_bindings(),
        pending_pastes: composer.draft.pending_pastes.clone(),
    }
}

#[test]
fn raw_and_prepared_correction_keep_suffix_and_element_ranges() {
    let mut composer = composer();
    let text = "/memory CoRrEcT e@1  @tool  ";
    let start = text.find("@tool").unwrap();
    let element = TextElement::new((start..start + 5).into(), Some("@tool".into()));
    let bindings = vec![MentionBinding {
        sigil: '@', mention: "tool".into(), path: "plugin://tool".into(),
    }];
    composer.set_text_content_with_mention_bindings(
        text.into(), vec![element.clone()], Vec::new(), bindings.clone(),
    );
    composer.set_current_cursor(text.len());
    composer.sync_popups();
    let before = draft(&composer);
    let args = "CoRrEcT e@1  @tool  ";
    let expected = vec![element.map_range(|range| ByteRange {
        start: range.start - 8, end: range.end - 8,
    })];
    assert!(composer.config.trim_submission);
    assert_eq!(key(&mut composer, KeyCode::Enter), InputResult::CommandWithArgs(
        SlashCommand::Memory, args.into(), expected.clone(),
    ));
    assert_eq!(composer.prepare_inline_args_submission(/*record_history*/ false),
        Some((args.into(), expected)));
    composer.record_pending_slash_command_history();
    assert_eq!(composer.history.startup_local_history(), &[before.clone()]);
    assert_eq!(key(&mut composer, KeyCode::Up), InputResult::None);
    assert_eq!(draft(&composer), before);
}

#[test]
fn expanded_corrections_route_locally_and_recall_rich_drafts() {
    for prefix in ["", "/memory ", "/memory correct e@1 "] {
        let mut composer = composer();
        composer.set_text_content(prefix.into(), Vec::new(), Vec::new());
        composer.set_current_cursor(prefix.len());
        let payload = format!("{}  ", "é".repeat(LARGE_PASTE_CHAR_THRESHOLD + 1));
        let paste = match prefix {
            "" => format!("/memory correct e@1 {payload}"),
            "/memory " => format!("correct e@1 {payload}"),
            _ => payload.clone(),
        };
        composer.handle_paste(paste);
        composer.handle_paste(" ".into());
        composer.attach_image(PathBuf::from("/tmp/correction.png"));
        let before = draft(&composer);
        let result = key(&mut composer, KeyCode::Enter);
        let InputResult::CommandWithArgs(cmd, raw, elements) = result else {
            panic!("correction escaped local dispatch");
        };
        assert_eq!(cmd, SlashCommand::Memory);
        let (args, elements) = if composer.current_text().is_empty() {
            (raw, elements)
        } else {
            composer.prepare_inline_args_submission(/*record_history*/ false).unwrap()
        };
        let expected = format!("correct e@1 {payload} [Image #1]");
        assert_eq!((args, elements), (expected.clone(), vec![TextElement::new(
            (expected.len() - 10..expected.len()).into(), Some("[Image #1]".into()),
        )]));
        composer.record_pending_slash_command_history();
        assert_eq!(composer.history.startup_local_history(), &[before.clone()]);
        composer.take_recent_submission_images();
        key(&mut composer, KeyCode::Up);
        assert_eq!(draft(&composer), before);
    }
}

#[test]
fn expanded_busy_tab_parses_corrections_but_preserves_literal_controls() {
    for prefix in ["", " ", "!"] {
        let mut composer = composer();
        composer.set_task_running(/*running*/ true);
        composer.set_text_content(prefix.into(), Vec::new(), Vec::new());
        composer.set_current_cursor(prefix.len());
        let text = format!("/memory correct e@1 {}  ", "x".repeat(LARGE_PASTE_CHAR_THRESHOLD));
        composer.handle_paste(text.clone());
        let InputResult::Queued { text: queued, action, .. } = key(&mut composer, KeyCode::Tab)
        else { panic!("expected queued input") };
        let expected = match prefix {
            "" => (text, QueuedInputAction::ParseSlash),
            " " => (text.trim().into(), QueuedInputAction::Plain),
            _ => (format!("!{}", text.trim()), QueuedInputAction::RunShell),
        };
        assert_eq!((queued, action), expected);
        assert_eq!(composer.history.startup_local_history().len(), 1);
    }
    let mut composer = composer();
    composer.config = ChatComposerConfig::plain_text();
    composer.handle_paste("/memory correct e@1  padded  ".into());
    assert_eq!(key(&mut composer, KeyCode::Enter), InputResult::Submitted {
        text: "/memory correct e@1  padded".into(), text_elements: Vec::new(),
    });
}

#[test]
fn oversized_expansion_restores_entire_draft() {
    let mut composer = composer();
    composer.handle_paste(format!("/memory correct e@1 {}", "x".repeat(MAX_USER_INPUT_TEXT_CHARS)));
    composer.attach_image(PathBuf::from("/tmp/retained.png"));
    let end = composer.current_text().len();
    composer.insert_selected_mention(end..end, "@tool", Some("plugin://tool"));
    let before = draft(&composer);
    assert_eq!(key(&mut composer, KeyCode::Enter), InputResult::None);
    assert_eq!(draft(&composer), before);
}

#[test]
fn typing_other_commands_and_leading_space_pastes_keep_existing_policy() {
    for (text, command, args) in [
        ("/memory add note  padded  ", SlashCommand::Memory, "add note  padded"),
        ("/review  padded  ", SlashCommand::Review, "padded"),
    ] {
        let mut composer = composer();
        for ch in text.chars() { key(&mut composer, KeyCode::Char(ch)); }
        assert_eq!(key(&mut composer, KeyCode::Enter), InputResult::CommandWithArgs(
            command, args.into(), Vec::new(),
        ));
        assert_eq!(composer.prepare_inline_args_submission(/*record_history*/ false),
            Some((args.into(), Vec::new())));
    }
    let mut composer = composer();
    composer.handle_paste(format!(" /memory correct e@1 {}  ", "x".repeat(1100)));
    assert_eq!(key(&mut composer, KeyCode::Enter), InputResult::Submitted {
        text: format!("/memory correct e@1 {}", "x".repeat(1100)), text_elements: Vec::new(),
    });
}
