use clap::ValueEnum;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum StatefulModeCliArg {
    Autonomous,
    Collaborative,
    Socratic,
}
