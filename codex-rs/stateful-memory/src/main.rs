//! Standalone project inspection. No engine login, home or native attachment.

mod arguments;
mod read;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use codex_project_store::ProjectStoreLocation;
use codex_project_store::ProjectStoreOpenError;
use codex_project_store::initialize_local_store;
use codex_project_store::inspect_store;
use codex_utils_absolute_path::AbsolutePathBuf;
use serde_json::Value;
use serde_json::json;

const MAX_REPLY_BYTES: usize = 4 * 1024;
const MAX_READ_REPLY_BYTES: usize = 8 * 1024;

#[derive(Parser)]
#[command(version, about = "Inspect project metadata and attributed content")]
struct Cli {
    #[arg(long)]
    project: PathBuf,
    /// Emit compact JSON (the default is indented JSON).
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Operation,
}

#[derive(Subcommand)]
enum Operation {
    Capabilities,
    Root,
    /// Read bounded attributed historical text from installed project domains.
    Read {
        #[arg(long)]
        project_uuid: String,
        #[arg(long)]
        store_uuid: String,
        #[arg(long)]
        entry: String,
    },
    /// Initialize metadata only after qualifying this host's storage.
    Init {
        #[arg(long, value_enum)]
        storage: StorageQualification,
    },
}

enum BootstrapOperation {
    Capabilities,
    Root,
    Init,
}

#[derive(Clone, Copy, ValueEnum)]
enum StorageQualification {
    LocalUnsynchronized,
}

#[derive(Clone, Copy)]
enum OutputStyle {
    Compact,
    Indented,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match arguments::bounded(std::env::args_os()) {
        Ok(args) => args,
        Err(()) => {
            return emit_reply(
                json!({"ok":false,"error":{"code":"invalidArguments","message":"Arguments exceed the UTF-8 count or byte limits."}}),
                OutputStyle::Compact,
                MAX_REPLY_BYTES,
            );
        }
    };
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                return if error.print().is_ok() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                };
            }
            return emit_reply(
                json!({"ok": false, "error": {"code": "invalidArguments", "message": "Use --help for required project and operation arguments."}}),
                OutputStyle::Compact,
                MAX_REPLY_BYTES,
            );
        }
    };
    let style = if cli.json {
        OutputStyle::Compact
    } else {
        OutputStyle::Indented
    };
    let max_reply_bytes = if matches!(&cli.command, Operation::Read { .. }) {
        MAX_READ_REPLY_BYTES
    } else {
        MAX_REPLY_BYTES
    };
    let reply = match execute(cli).await {
        Ok(result) => json!({"ok": true, "result": result}),
        Err((code, message)) => json!({"ok": false, "error": {"code": code, "message": message}}),
    };
    emit_reply(reply, style, max_reply_bytes)
}

async fn execute(cli: Cli) -> Result<Value, (&'static str, String)> {
    let operation = match cli.command {
        Operation::Read {
            project_uuid,
            store_uuid,
            entry,
        } => return read::execute(&cli.project, &project_uuid, &store_uuid, &entry).await,
        Operation::Capabilities => BootstrapOperation::Capabilities,
        Operation::Root => BootstrapOperation::Root,
        Operation::Init {
            storage: StorageQualification::LocalUnsynchronized,
        } => BootstrapOperation::Init,
    };
    let root = AbsolutePathBuf::relative_to_current_dir(&cli.project)
        .map_err(|error| ("invalidProject", error.to_string()))?;
    let location = ProjectStoreLocation::select_project(&root)
        .map_err(|error| ("invalidProject", error.to_string()))?;
    let project_root = location.project_root().as_path().to_str().ok_or((
        "unsupportedPathEncoding",
        "The project path must be representable as UTF-8.".to_string(),
    ))?;
    let database_path = location.database_path().as_path().to_str().ok_or((
        "unsupportedPathEncoding",
        "The store path must be representable as UTF-8.".to_string(),
    ))?;
    let identity = match operation {
        BootstrapOperation::Capabilities => {
            return Ok(json!({
                "protocolVersion": 1, "coverage": "bootstrapOnly",
                "operations": ["capabilities", "root", "init", "read"],
                "contentRead": {"projection":"attributedContent","verification":"notEvaluated","maxReplyBytes":MAX_READ_REPLY_BYTES,"requiresInstalledDomains":["project-intelligence","stateful-runtime"]},
                "maxReplyBytes": MAX_REPLY_BYTES, "knowledgeAvailable": false,
                "nativeAttached": false
            }));
        }
        BootstrapOperation::Root => inspect_store(&location).await.map_err(open_error)?,
        BootstrapOperation::Init => initialize_local_store(&location)
            .await
            .map_err(open_error)?,
    };
    Ok(json!({
        "projectUuid": identity.project_uuid.to_string(), "storeUuid": identity.store_uuid.to_string(),
        "projectRoot": project_root, "databasePath": database_path,
        "coverage": "bootstrapOnly", "knowledgeAvailable": false, "nativeAttached": false,
        "snapshotRevision": null, "eligibilityGeneration": null
    }))
}

fn open_error(error: ProjectStoreOpenError) -> (&'static str, String) {
    let code = match &error {
        ProjectStoreOpenError::Io(error) => {
            if error.kind() == std::io::ErrorKind::NotFound {
                "storeUnavailable"
            } else {
                "filesystemError"
            }
        }
        ProjectStoreOpenError::Resolve(_) => "invalidProject",
        ProjectStoreOpenError::Sql(_) => "storeUnavailable",
        ProjectStoreOpenError::Registry(_) => "unsupportedStore",
        ProjectStoreOpenError::UnsupportedFilesystem => "unsupportedFilesystem",
        ProjectStoreOpenError::LocationChanged => "locationChanged",
        ProjectStoreOpenError::IgnorePolicy => "ignorePolicyConflict",
    };
    (code, error.to_string())
}

fn emit_reply(reply: Value, style: OutputStyle, max_reply_bytes: usize) -> ExitCode {
    let succeeded = reply["ok"] == true;
    let encoded = match encode_reply(&reply, style, max_reply_bytes) {
        Some(encoded) => encoded,
        None => {
            let fallback: &[u8] = if max_reply_bytes == MAX_READ_REPLY_BYTES {
                b"{\"ok\":false,\"error\":{\"code\":\"responseTooLarge\",\"message\":\"Reply cannot fit the 8192-byte ASCII envelope.\"}}\n"
            } else {
                b"{\"ok\":false,\"error\":{\"code\":\"responseTooLarge\",\"message\":\"Reply cannot fit the 4096-byte UTF-8 envelope.\"}}\n"
            };
            let _ = std::io::stdout().write_all(fallback);
            return ExitCode::FAILURE;
        }
    };
    if std::io::stdout().write_all(&encoded).is_ok() && succeeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn encode_reply(reply: &Value, style: OutputStyle, max_reply_bytes: usize) -> Option<Vec<u8>> {
    let text = match style {
        OutputStyle::Compact => serde_json::to_string(reply),
        OutputStyle::Indented => serde_json::to_string_pretty(reply),
    }
    .ok()?;
    let mut bytes = Vec::new();
    for character in text.chars() {
        if character.is_ascii() {
            bytes.push(character as u8);
        } else {
            for unit in character.encode_utf16(&mut [0; 2]) {
                bytes.extend_from_slice(format!("\\u{unit:04x}").as_bytes());
            }
        }
        if bytes.len() >= max_reply_bytes {
            return None;
        }
    }
    bytes.push(b'\n');
    Some(bytes)
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
