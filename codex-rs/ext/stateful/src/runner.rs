//! What a command actually ran: the shell source behind a `bash -lc` or `pwsh -Command` argv,
//! and whether it invokes a test or check runner (not merely mentions one). Recognition is
//! deliberately conservative: separators inside quotes do not split commands, and a program
//! whose arguments are not understood is not a runner.

const SHELLS: &[&str] = &["bash", "sh", "zsh", "dash", "pwsh", "powershell", "cmd"];
const SCRIPT_FLAGS: &[&str] = &["-c", "-lc", "-command", "/c"];

/// How a command was given to the host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommandForm {
    /// A shell ran this exact script source.
    Script { shell: String, source: String },
    /// A program ran with these exact arguments; any one-line rendering is display only.
    Argv(Vec<String>),
}

impl CommandForm {
    pub(crate) fn of(argv: &[String]) -> Self {
        if argv.len() >= 3
            && SHELLS.contains(&executable_name(&argv[0]).as_str())
            && SCRIPT_FLAGS.contains(&argv[argv.len() - 2].to_ascii_lowercase().as_str())
        {
            return Self::Script {
                shell: executable_name(&argv[0]),
                source: argv[argv.len() - 1].clone(),
            };
        }
        Self::Argv(argv.to_vec())
    }

    /// The text shown for this command: the exact script, or the argv joined for display.
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Script { source, .. } => source.clone(),
            Self::Argv(argv) => argv.join(" "),
        }
    }

    /// Whether the shown text is exactly what ran, so it can be replayed as shown.
    pub(crate) fn replayable(&self) -> bool {
        matches!(self, Self::Script { .. })
    }

    pub(crate) fn shell(&self) -> Option<&str> {
        match self {
            Self::Script { shell, .. } => Some(shell),
            Self::Argv(_) => None,
        }
    }

    /// The test or check runner this command invokes, if it clearly invokes one.
    pub(crate) fn runner(&self) -> Option<&'static str> {
        match self {
            Self::Script { source, .. } => invoked_runner(source),
            Self::Argv(argv) => words_runner(argv.iter().map(String::as_str)),
        }
    }
}

/// The test or check runner a shell source invokes, judged from the program of each simple
/// command (after environment assignments and wrappers such as `uv run`), never from a mention.
/// Says nothing about what passed; a compound command's exit code is its last command's.
pub(crate) fn invoked_runner(source: &str) -> Option<&'static str> {
    split_commands(source)
        .iter()
        .find_map(|segment| words_runner(shell_words(segment).iter().map(String::as_str)))
}

/// Simple commands of a shell source, split on `;`, `&`, `|` and newlines outside quotes.
fn split_commands(source: &str) -> Vec<String> {
    let mut segments = vec![String::new()];
    let mut quote = None;
    for character in source.chars() {
        match (quote, character) {
            (Some(open), character) if character == open => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, ';' | '&' | '|' | '\n') => {
                segments.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(segment) = segments.last_mut() {
            segment.push(character);
        }
    }
    segments
}

/// Words of one simple command, with quotes removed (a quoted word stays one word).
fn shell_words(segment: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for character in segment.chars() {
        match (quote, character) {
            (Some(open), character) if character == open => quote = None,
            (Some(_), character) => word.push(character),
            (None, '\'' | '"') => quote = Some(character),
            (None, character) if character.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (None, character) => word.push(character),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

fn words_runner<'a>(words: impl Iterator<Item = &'a str>) -> Option<&'static str> {
    let mut words = words
        .skip_while(|word| {
            // Environment assignments (`TMPDIR=/x`, `$env:TMP='C:\t'`) precede the program.
            (word.contains('=') && !word.starts_with('-')) || word.starts_with("$env:")
        })
        .peekable();
    let mut program = executable_name(words.next()?.trim_start_matches('&'));
    // Wrappers that run the next word as the program.
    loop {
        match (program.as_str(), words.peek().copied()) {
            ("uv" | "poetry" | "pipenv" | "hatch" | "rye", Some("run"))
            | ("pnpm" | "npm", Some("exec")) => {
                words.next();
                program = executable_name(words.next()?);
            }
            ("npx" | "bunx", Some(_)) => {
                program = executable_name(words.next()?);
            }
            _ => break,
        }
    }
    match program.as_str() {
        "pytest" | "py.test" => Some("pytest"),
        "tox" => Some("tox"),
        "jest" => Some("jest"),
        "vitest" => Some("vitest"),
        "rspec" => Some("rspec"),
        "phpunit" => Some("phpunit"),
        "ctest" => Some("ctest"),
        "tsc" => Some("tsc"),
        "python" | "python3" | "py" => {
            // Interpreter options come first; `-m` names the module that runs. A script or any
            // other argument before `-m` means a script runs, not a test module.
            for word in words.by_ref() {
                match word {
                    "-m" => break,
                    option if option.starts_with('-') && option != "-c" => continue,
                    _ => return None,
                }
            }
            match words.next()?.to_ascii_lowercase().as_str() {
                "pytest" => Some("pytest"),
                "unittest" => Some("unittest"),
                "tox" => Some("tox"),
                _ => None,
            }
        }
        "cargo" => match words.next()?.to_ascii_lowercase().as_str() {
            "test" => Some("cargo test"),
            "nextest" => Some("cargo nextest"),
            "check" => Some("cargo check"),
            "clippy" => Some("cargo clippy"),
            _ => None,
        },
        "just" | "make" | "go" | "dotnet" | "mvn" | "gradle" | "gradlew" | "yarn" | "pnpm"
        | "npm" => match (words.next()?, words.next()) {
            ("test", _) | ("run", Some("test")) => Some("test task"),
            _ => None,
        },
        _ => None,
    }
}

/// The program name without its directory or Windows executable extension, lower-cased.
fn executable_name(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    let name = name.to_ascii_lowercase();
    for extension in [".exe", ".cmd", ".bat", ".ps1"] {
        if let Some(stem) = name.strip_suffix(extension) {
            return stem.to_string();
        }
    }
    name
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
