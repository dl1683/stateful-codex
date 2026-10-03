use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::RecipeCheck;
use super::check_observation;
use crate::recipe_capture::ExitStatus;
use crate::recipe_capture::ObservedCommand;
use crate::recipe_capture::RecipeObservation;
use crate::recipe_capture::capture_conditions;
use crate::recipe_capture::observation;

fn command_in(root: &TempDir, script: &str) -> ObservedCommand {
    ObservedCommand {
        turn_id: "turn-1".to_string(),
        call_id: "call-1".to_string(),
        script: script.to_string(),
        cwd: root.path().to_path_buf(),
        completed: None,
    }
}

fn observed_in(root: &TempDir, script: &str) -> RecipeObservation {
    let mut command = command_in(root, script);
    command.completed = capture_conditions(&command, /*at_ms*/ 0);
    observation(&command, ExitStatus::Zero).expect("conditions captured")
}

/// A recipe stays current while its interpreter and manifest set are unchanged, and needs
/// a check once a manifest changes or is added, or the interpreter is replaced or deleted.
#[test]
fn a_recipe_needs_a_check_when_its_environment_changes() {
    let root = TempDir::new().expect("project root");
    let interpreter = root.path().join(".venv").join("python");
    std::fs::create_dir_all(interpreter.parent().expect("venv dir")).expect("venv dir");
    std::fs::write(&interpreter, "v1").expect("interpreter");
    let manifest = root.path().join("pyproject.toml");
    std::fs::write(&manifest, "[project]\nname='a'\n").expect("manifest");
    let observation = observed_in(&root, ".venv/python -m pytest tests -q");

    let fresh = check_observation(&observation);
    std::fs::write(&manifest, "[project]\nname='b'\n").expect("edit");
    let manifest_changed = check_observation(&observation);
    std::fs::write(&manifest, "[project]\nname='a'\n").expect("restore");
    std::fs::write(root.path().join("uv.lock"), "lock").expect("add lock");
    let manifest_added = check_observation(&observation);
    std::fs::remove_file(root.path().join("uv.lock")).expect("remove lock");
    std::fs::write(&interpreter, "v2 rebuilt").expect("replace interpreter");
    let interpreter_replaced = check_observation(&observation);
    std::fs::remove_file(&interpreter).expect("delete interpreter");
    let interpreter_gone = check_observation(&observation);

    assert_eq!(
        [
            fresh,
            manifest_changed,
            manifest_added,
            interpreter_replaced,
            interpreter_gone,
        ],
        [
            RecipeCheck::Current,
            RecipeCheck::NeedsCheck("pyproject.toml changed since".to_string()),
            RecipeCheck::NeedsCheck("uv.lock was added since".to_string()),
            RecipeCheck::NeedsCheck(
                "executable .venv/python now resolves to a different or changed file".to_string()
            ),
            RecipeCheck::NeedsCheck("executable .venv/python no longer resolves".to_string()),
        ]
    );
}

#[test]
fn an_unresolvable_executable_or_non_local_directory_is_never_grounded() {
    let root = TempDir::new().expect("project root");
    let missing_interpreter = command_in(&root, ".venv/python -m pytest");
    let mut remote = missing_interpreter.clone();
    remote.cwd = root.path().join("not-on-this-host");
    assert_eq!(
        (
            capture_conditions(&missing_interpreter, /*at_ms*/ 0),
            capture_conditions(&remote, /*at_ms*/ 0),
        ),
        (None, None)
    );
}
