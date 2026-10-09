use codex_extension_api::ToolName;

use super::read_only_script;
use super::tool_has_effects;

#[test]
fn only_plain_allowlisted_reads_are_read_only() {
    for script in [
        "cat README.md",
        "ls -la src",
        "grep -rn \"fn main\" src",
        "rg TODO --glob '*.rs'",
        "head -n 20 src/lib.rs",
        "git status",
        "git log --oneline -5",
        "git diff HEAD -- src/lib.rs",
        "find . -name '*.py'",
        "wc -l src/lib.rs",
    ] {
        assert!(read_only_script(script), "{script}");
    }
    for script in [
        "echo x > notes.txt",
        "cat a >> b",
        "ls; rm -rf build",
        "cat a | tee b",
        "rg 'TODO|FIXME' src",
        "cat $(ls)",
        "grep -r foo . && touch done",
        "rm notes.txt",
        "python3 -c print",
        "make",
        "sed -i s/a/b/ file",
        "git commit -m wip",
        "git -C repo status",
        "git diff --output=patch.txt",
        "rg --pre ./decode foo",
        "find . -delete",
        "find . -exec rm {} ;",
        "/bin/cat README.md",
        "cat 'unterminated",
        "Get-Content README.md",
        "",
    ] {
        assert!(!read_only_script(script), "{script}");
    }
}

#[test]
fn only_reading_and_bookkeeping_tools_have_no_effects() {
    for name in [
        "update_plan",
        "view_image",
        "stateful_run_update",
        "blackboard_query",
        "exec_command",
    ] {
        assert!(!tool_has_effects(&ToolName::plain(name)), "{name}");
    }
    for name in [
        "apply_patch",
        "write_stdin",
        "spawn_agent",
        "exec",
        "unknown_tool",
    ] {
        assert!(tool_has_effects(&ToolName::plain(name)), "{name}");
    }
    assert!(tool_has_effects(&ToolName::namespaced(
        "mcp__files",
        "read"
    )));
}
