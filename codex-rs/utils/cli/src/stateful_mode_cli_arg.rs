use clap::ValueEnum;

/// How `--stateful` uses the current project. `ask` (also a bare `--stateful`) starts no run;
/// the other values start a run for the prompt's goal in that workflow mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum StatefulModeCliArg {
    /// Answer with the project's memory (rules, decisions, root findings); starts no run.
    Ask,
    /// Start a run that keeps working within its budget until the goal ends.
    Autonomous,
    /// Start a run that stays open across your turns.
    Collaborative,
    /// Start a run that questions and plans first and executes only after you resume it.
    Socratic,
}
