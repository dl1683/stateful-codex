#![cfg(windows)]

use codex_shell_command::powershell::prefix_powershell_script_with_utf8;
use pretty_assertions::assert_eq;
use std::process::Command;

#[test]
fn constrained_language_preserves_file_decoding_when_console_setup_is_refused() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let json = r#"{"blockId":"M-01","source":"मराठी २००[?]","reviewRequired":true}"#;
    let utf8 = directory.path().join("source.json");
    let utf16 = directory.path().join("explicit-encoding.json");
    let csv = directory.path().join("source.csv");
    std::fs::write(&utf8, json)?;
    std::fs::write(
        &utf16,
        json.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )?;
    std::fs::write(&csv, "label,value\r\nमराठी,२००[?]\r\n")?;
    let utf8 = utf8.to_string_lossy().replace('\'', "''");
    let utf16 = utf16.to_string_lossy().replace('\'', "''");
    let csv = csv.to_string_lossy().replace('\'', "''");
    let powershell = std::path::PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| anyhow::anyhow!("SystemRoot is required on Windows"))?,
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    for (script, expected) in [
        (
            format!("Get-Content -LiteralPath '{utf8}' -Raw"),
            format!("{json}\r\n"),
        ),
        (
            format!(
                "Select-String -LiteralPath '{utf8}' -Pattern 'blockId' | ForEach-Object {{ $_.Line }}"
            ),
            format!("{json}\r\n"),
        ),
        (
            format!(
                "$row = Import-Csv -LiteralPath '{csv}'; Write-Output $row.label; Write-Output $row.value"
            ),
            "मराठी\r\n२००[?]\r\n".to_string(),
        ),
        (
            format!("Get-Content -LiteralPath '{utf16}' -Encoding Unicode -Raw"),
            format!("{json}\r\n"),
        ),
    ] {
        let command = prefix_powershell_script_with_utf8(&[
            powershell.to_string_lossy().into_owned(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            script,
        ]);
        let script = &command[3];
        // Qualify file decoding independently of console transport: establish the
        // console before CLM, then let the production prefix hit its refused setter.
        let invocation = format!(
            "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $ErrorActionPreference='Stop'; $ExecutionContext.SessionState.LanguageMode='ConstrainedLanguage'; {script}"
        );
        let output = Command::new(&command[0])
            .args(["-NoProfile", "-Command", &invocation])
            .output()?;
        assert_eq!(
            (
                output.status.success(),
                String::from_utf8(output.stdout)?,
                String::from_utf8(output.stderr)?
            ),
            (true, expected, String::new())
        );
    }
    Ok(())
}
