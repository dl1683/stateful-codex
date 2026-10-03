use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::RecipeCheck;
use super::check_observation;
use crate::recipe_capture::ExitStatus;
use crate::recipe_capture::ObservedCommand;
use crate::recipe_capture::observe_recipe;

fn observed_in(root: &TempDir, script: &str) -> crate::recipe_capture::RecipeObservation {
    observe_recipe(
        &ObservedCommand {
            turn_id: "turn-1".to_string(),
            call_id: "call-1".to_string(),
            script: script.to_string(),
            cwd: root.path().to_path_buf(),
        },
        /*observed_at_ms*/ 0,
        ExitStatus::Zero,
    )
}

/// A recipe stays current while its interpreter and manifests are unchanged, and needs a
/// check once the interpreter is deleted or a manifest changes.
#[test]
fn a_recipe_needs_a_check_when_its_environment_changes() {
    let root = TempDir::new().expect("project root");
    let interpreter = root.path().join(".venv").join("python");
    std::fs::create_dir_all(interpreter.parent().expect("venv dir")).expect("venv dir");
    std::fs::write(&interpreter, "").expect("interpreter");
    std::fs::write(root.path().join("pyproject.toml"), "[project]\nname='a'\n").expect("manifest");
    let observation = observed_in(&root, ".venv/python -m pytest tests -q");

    let fresh = check_observation(&observation);
    std::fs::write(root.path().join("pyproject.toml"), "[project]\nname='b'\n").expect("edit");
    let manifest_changed = check_observation(&observation);
    std::fs::write(root.path().join("pyproject.toml"), "[project]\nname='a'\n").expect("restore");
    std::fs::remove_file(&interpreter).expect("delete interpreter");
    let interpreter_gone = check_observation(&observation);

    assert_eq!(
        [fresh, manifest_changed, interpreter_gone],
        [
            RecipeCheck::Current,
            RecipeCheck::NeedsCheck("pyproject.toml changed since".to_string()),
            RecipeCheck::NeedsCheck("executable .venv/python no longer resolves".to_string()),
        ]
    );
}

#[test]
fn shell_words_and_environment_assignments_are_not_checked_as_executables() {
    let root = TempDir::new().expect("project root");
    assert_eq!(
        [
            check_observation(&observed_in(&root, "cd src && pytest")),
            check_observation(&observed_in(
                &root,
                "$env:PYTHONPATH='src'; python -m pytest"
            )),
        ],
        [RecipeCheck::Current, RecipeCheck::Current]
    );
}
