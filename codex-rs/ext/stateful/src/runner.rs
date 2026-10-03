//! What a command actually ran: the shell source behind a `bash -lc` or `pwsh -Command` argv,
//! and whether it invokes a test or check runner (not merely mentions one).

/// The command as its author wrote it, and the shell that ran it when the argv wraps a shell
/// script. Other argv is rendered with whitespace-bearing arguments quoted.
pub(crate) fn command_source(argv: &[String]) -> (String, Option<String>) {
    const SCRIPT_FLAGS: &[&str] = &["-c", "-lc", "-command", "/c"];
    if argv.len() >= 3 && SCRIPT_FLAGS.contains(&argv[argv.len() - 2].to_ascii_lowercase().as_str())
    {
        return (
            argv[argv.len() - 1].clone(),
            Some(executable_name(&argv[0])),
        );
    }
    let rendered = argv
        .iter()
        .map(|argument| {
            if argument.chars().any(char::is_whitespace) || argument.is_empty() {
                format!("\"{}\"", argument.replace('"', "\\\""))
            } else {
                argument.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    (rendered, None)
}

/// The test or check runner a shell source invokes, judged from the executable of each simple
/// command (after environment assignments and wrappers such as `uv run`), never from a mere
/// mention. Says nothing about what passed; a compound command's exit code is its last
/// command's.
pub(crate) fn invoked_runner(source: &str) -> Option<&'static str> {
    source.split(['\n', ';', '&', '|']).find_map(segment_runner)
}

fn segment_runner(segment: &str) -> Option<&'static str> {
    let mut words = segment
        .split_whitespace()
        .map(|word| word.trim_matches(['"', '\'']))
        .filter(|word| !word.is_empty())
        .skip_while(|word| {
            // Environment assignments (`TMPDIR=/x`, `$env:TMP='C:\t'`) precede the program.
            (word.contains('=') && !word.starts_with('-')) || word.starts_with("$env:")
        })
        .peekable();
    let mut program = executable_name(words.next()?);
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
    let next = words.next().map(str::to_ascii_lowercase);
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
            let mut rest = std::iter::once(next?).chain(words.map(str::to_ascii_lowercase));
            rest.position(|word| word == "-m")?;
            match rest.next()?.as_str() {
                "pytest" => Some("pytest"),
                "unittest" => Some("unittest"),
                "tox" => Some("tox"),
                _ => None,
            }
        }
        "cargo" => match next?.as_str() {
            "test" => Some("cargo test"),
            "nextest" => Some("cargo nextest"),
            "check" => Some("cargo check"),
            "clippy" => Some("cargo clippy"),
            _ => None,
        },
        "just" | "make" | "go" | "dotnet" | "mvn" | "gradle" | "gradlew" | "yarn" | "pnpm"
        | "npm" => match (next?.as_str(), words.next()) {
            ("test", _) | ("run", Some("test")) => Some("test task"),
            _ => None,
        },
        _ => None,
    }
}

/// The program name without its directory or Windows executable extension, lower-cased.
fn executable_name(word: &str) -> String {
    let word = word.trim_matches(['"', '\'']).trim_start_matches('&');
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
