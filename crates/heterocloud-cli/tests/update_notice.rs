use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn update_notice_preserves_command_stdout_and_exit_status() -> Result<(), Box<dyn std::error::Error>>
{
    let private = tempfile::tempdir()?;
    let credential_file = private.path().join("credentials.json");
    let args = [
        "dns",
        "records",
        "--domain",
        "cloud.example.com",
        "--public-ip",
        "1.1.1.1",
    ];
    let run = |disabled: &str| {
        Command::new(env!("CARGO_BIN_EXE_heterocloud"))
            .args(args)
            .env("HETEROCLOUD_CREDENTIALS_FILE", &credential_file)
            .env("HETEROCLOUD_NO_UPDATE_CHECK", disabled)
            .output()
    };
    let baseline = run("1")?;
    assert!(baseline.status.success());
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    fs::write(
        private.path().join("update-check.json"),
        serde_json::to_vec(&serde_json::json!({"checked_at":now,"latest_version":"9999.0.0"}))?,
    )?;
    let notified = run("0")?;
    assert!(notified.status.success());
    assert_eq!(notified.stdout, baseline.stdout);
    assert!(String::from_utf8(notified.stderr)?.contains("Run `heterocloud update`"));
    let failed = |disabled: &str| {
        Command::new(env!("CARGO_BIN_EXE_heterocloud"))
            .args(["auth", "status"])
            .env("HETEROCLOUD_CREDENTIALS_FILE", &credential_file)
            .env("HETEROCLOUD_NO_UPDATE_CHECK", disabled)
            .output()
    };
    let baseline_failure = failed("1")?;
    let notified_failure = failed("0")?;
    assert!(!baseline_failure.status.success());
    assert_eq!(
        notified_failure.status.code(),
        baseline_failure.status.code()
    );
    assert_eq!(notified_failure.stdout, baseline_failure.stdout);
    let failure_stderr = String::from_utf8(notified_failure.stderr)?;
    assert!(failure_stderr.starts_with(&String::from_utf8(baseline_failure.stderr)?));
    assert!(failure_stderr.contains("Run `heterocloud update`"));
    fs::write(
        private.path().join("update-check.json"),
        serde_json::to_vec(&serde_json::json!({"checked_at":now,"latest_version":null}))?,
    )?;
    let offline = run("0")?;
    assert!(offline.status.success());
    assert_eq!(offline.stdout, baseline.stdout);
    assert!(offline.stderr.is_empty());
    assert!(
        !credential_file.exists(),
        "update checking must not create or replace authentication credentials"
    );
    Ok(())
}
