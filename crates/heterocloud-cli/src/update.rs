use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use clap::Args;
use flate2::read::GzDecoder;
use reqwest::{Client, redirect::Policy};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::{NamedTempFile, TempPath};
use thiserror::Error;

use crate::ApiOutputFormat;

const RELEASES: &str =
    "https://api.github.com/repos/IPA-CyberLab/IPA-RS-HeteroCloud/releases?per_page=10";
const DOWNLOADS: &str = "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/download";
const MAX_ARCHIVE: u64 = 96 * 1024 * 1024;
const MAX_BINARY: u64 = 128 * 1024 * 1024;
const CHECK_INTERVAL: u64 = 6 * 60 * 60;

#[derive(Debug, Args)]
pub struct UpdateArgs {
    /// Check the release without changing the installed CLI.
    #[arg(long)]
    pub check: bool,
    /// Reinstall the current version when it is already the latest; never downgrade.
    #[arg(long, conflicts_with = "check")]
    pub force: bool,
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("public release request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid public release metadata: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid ZIP archive: {0}")]
    Zip(#[from] zip::result::ZipError),
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug)]
struct Candidate {
    version: Version,
    archive: String,
    download: String,
    checksum: String,
}

#[derive(Serialize, Deserialize)]
struct CheckCache {
    checked_at: u64,
    latest_version: Option<String>,
}

fn platform(
    os: &str,
    arch: &str,
) -> Result<(&'static str, &'static str, &'static str), UpdateError> {
    let os = match os {
        "linux" => "linux",
        "macos" => "macos",
        "windows" => "windows",
        _ => {
            return Err(UpdateError::Invalid(
                "automatic CLI updates do not support this OS",
            ));
        }
    };
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => {
            return Err(UpdateError::Invalid(
                "automatic CLI updates do not support this CPU",
            ));
        }
    };
    Ok((os, arch, if os == "windows" { "zip" } else { "tar.gz" }))
}

fn select_release(releases: Vec<Release>, os: &str, arch: &str) -> Result<Candidate, UpdateError> {
    let (os, arch, suffix) = platform(os, arch)?;
    releases
        .into_iter()
        .filter_map(|release| {
            if release.draft || release.prerelease {
                return None;
            }
            let version = Version::parse(
                release
                    .tag_name
                    .strip_prefix('v')
                    .unwrap_or(&release.tag_name),
            )
            .ok()?;
            if !version.pre.is_empty() || !version.build.is_empty() {
                return None;
            }
            let archive = format!("heterocloud-{}-{os}-{arch}.{suffix}", release.tag_name);
            let download = format!("{DOWNLOADS}/{}/{archive}", release.tag_name);
            let checksum = format!("{download}.sha256");
            if !release
                .assets
                .iter()
                .any(|a| a.name == archive && a.browser_download_url == download)
                || !release.assets.iter().any(|a| {
                    a.name == format!("{archive}.sha256") && a.browser_download_url == checksum
                })
            {
                return None;
            }
            Some(Candidate {
                version,
                archive,
                download,
                checksum,
            })
        })
        .max_by(|a, b| a.version.cmp(&b.version))
        .ok_or(UpdateError::Invalid(
            "no complete stable CLI release exists for this platform",
        ))
}

fn client(timeout: Duration) -> Result<Client, UpdateError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Ok(Client::builder()
        .https_only(true)
        .timeout(timeout)
        .user_agent(concat!(
            "HeteroCloud-CLI-Updater/",
            env!("CARGO_PKG_VERSION")
        ))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.error("too many release redirects");
            }
            if attempt.url().scheme() == "https"
                && matches!(
                    attempt.url().host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.error("untrusted release redirect")
            }
        }))
        .build()?)
}

async fn bounded(client: &Client, url: &str, limit: usize) -> Result<Vec<u8>, UpdateError> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    let mut result = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if result.len().saturating_add(chunk.len()) > limit {
            return Err(UpdateError::Invalid(
                "release response exceeds its size limit",
            ));
        }
        result.extend_from_slice(&chunk);
    }
    Ok(result)
}

async fn latest(client: &Client) -> Result<Candidate, UpdateError> {
    let releases = serde_json::from_slice(&bounded(client, RELEASES, 1024 * 1024).await?)?;
    select_release(releases, std::env::consts::OS, std::env::consts::ARCH)
}

fn expected_checksum(bytes: &[u8], archive: &str) -> Result<String, UpdateError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| UpdateError::Invalid("invalid checksum encoding"))?;
    let mut words = text.split_whitespace();
    let hash = words
        .next()
        .ok_or(UpdateError::Invalid("missing release checksum"))?;
    let filename = words
        .next()
        .ok_or(UpdateError::Invalid("missing checksum filename"))?;
    if hash.len() != 64
        || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        || filename.trim_start_matches('*') != archive
        || words.next().is_some()
    {
        return Err(UpdateError::Invalid(
            "checksum does not identify the selected release archive",
        ));
    }
    Ok(hash.to_ascii_lowercase())
}

async fn download(
    client: &Client,
    candidate: &Candidate,
    file: &mut File,
) -> Result<(), UpdateError> {
    let expected = expected_checksum(
        &bounded(client, &candidate.checksum, 4096).await?,
        &candidate.archive,
    )?;
    let mut response = client
        .get(&candidate.download)
        .send()
        .await?
        .error_for_status()?;
    let mut size = 0u64;
    let mut hash = Sha256::new();
    while let Some(chunk) = response.chunk().await? {
        size = size.saturating_add(chunk.len() as u64);
        if size > MAX_ARCHIVE {
            return Err(UpdateError::Invalid(
                "release archive exceeds its size limit",
            ));
        }
        hash.update(&chunk);
        file.write_all(&chunk)?;
    }
    if format!("{:x}", hash.finalize()) != expected {
        return Err(UpdateError::Invalid(
            "release checksum mismatch; installed CLI was not changed",
        ));
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(())
}

fn copy_binary(reader: impl Read, destination: &mut File) -> Result<(), UpdateError> {
    let size = io::copy(&mut reader.take(MAX_BINARY + 1), destination)?;
    if size == 0 || size > MAX_BINARY {
        return Err(UpdateError::Invalid("invalid executable size"));
    }
    Ok(())
}

fn extract(archive: &mut File, output: &mut File, windows: bool) -> Result<(), UpdateError> {
    if windows {
        let mut zip = zip::ZipArchive::new(archive)?;
        if zip.len() != 1 {
            return Err(UpdateError::Invalid(
                "release ZIP must contain only the CLI executable",
            ));
        }
        let mut entry = zip.by_index(0)?;
        if entry.name() != "heterocloud.exe"
            || entry.is_dir()
            || entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170000, 0 | 0o100000))
        {
            return Err(UpdateError::Invalid(
                "unsafe executable entry in release ZIP",
            ));
        }
        copy_binary(&mut entry, output)?;
    } else {
        let mut tar = tar::Archive::new(GzDecoder::new(archive));
        let mut count = 0;
        for entry in tar.entries()? {
            let mut entry = entry?;
            if entry.path()? != Path::new("heterocloud")
                || !entry.header().entry_type().is_file()
                || count != 0
            {
                return Err(UpdateError::Invalid(
                    "release TAR must contain only a regular CLI executable",
                ));
            }
            copy_binary(&mut entry, output)?;
            count += 1;
        }
        if count != 1 {
            return Err(UpdateError::Invalid("release archive has no executable"));
        }
    }
    output.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        output.set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn install(staged: TempPath, destination: &Path) -> Result<(), UpdateError> {
    #[cfg(windows)]
    {
        // Windows cannot overwrite an executing image. Keep one previous binary
        // and restore it if replacement fails; config and credentials live elsewhere.
        let backup = destination.with_extension("previous.exe");
        fs::rename(destination, &backup)?;
        match staged.persist(destination) {
            Ok(()) => return Ok(()),
            Err(error) => {
                fs::rename(&backup, destination)?;
                return Err(error.error.into());
            }
        }
    }
    #[cfg(not(windows))]
    {
        staged.persist(destination).map_err(|error| error.error)?;
        Ok(())
    }
}

pub async fn execute(args: UpdateArgs, output: ApiOutputFormat) -> Result<(), UpdateError> {
    let candidate = latest(&client(Duration::from_secs(120))?).await?;
    let current = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| UpdateError::Invalid("invalid installed version"))?;
    let available = candidate.version > current;
    let destination = std::env::current_exe()?;
    let replace = !args.check && (available || (args.force && candidate.version == current));
    if replace {
        let parent = destination
            .parent()
            .ok_or(UpdateError::Invalid("installation directory is missing"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(parent.join(".heterocloud-update.lock"))?;
        lock.try_lock()
            .map_err(|_| UpdateError::Invalid("another CLI update is running"))?;
        let mut archive = tempfile::tempfile_in(parent)?;
        download(&client(Duration::from_secs(120))?, &candidate, &mut archive).await?;
        let mut staged = tempfile::Builder::new()
            .prefix(".heterocloud-update-")
            .suffix(if cfg!(windows) { ".exe" } else { ".bin" })
            .tempfile_in(parent)?;
        extract(&mut archive, staged.as_file_mut(), cfg!(windows))?;
        let staged = staged.into_temp_path(); // close the writable handle before executing on Windows
        let version = Command::new(&staged).arg("--version").output()?;
        if !version.status.success()
            || String::from_utf8_lossy(&version.stdout).trim()
                != format!("heterocloud {}", candidate.version)
        {
            return Err(UpdateError::Invalid(
                "downloaded executable does not report the selected release version",
            ));
        }
        install(staged, &destination)?;
    }
    let result = serde_json::json!({"current_version":current.to_string(),"latest_version":candidate.version.to_string(),
        "update_available":available,"updated":replace,"executable":destination});
    match output {
        ApiOutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
        ApiOutputFormat::Table => println!(
            "current\t{current}\nlatest\t{}\nupdated\t{replace}",
            candidate.version
        ),
    }
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

fn notice(latest: &str) -> Option<String> {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
    let latest = Version::parse(latest).ok()?;
    (latest>current && latest.pre.is_empty()).then(||format!("HeteroCloud CLI {latest} is available (installed: {current}). Run `heterocloud update`."))
}

pub async fn available_update_notice() -> Option<String> {
    if std::env::var_os("HETEROCLOUD_NO_UPDATE_CHECK").is_some_and(|v| v == "1") {
        return None;
    }
    let cache = crate::auth::credential_file_path()
        .ok()?
        .parent()?
        .join("update-check.json");
    let checked_at = now();
    if let Ok(metadata) = fs::symlink_metadata(&cache)
        && metadata.is_file()
        && metadata.len() <= 4096
        && let Ok(bytes) = fs::read(&cache)
        && let Ok(saved) = serde_json::from_slice::<CheckCache>(&bytes)
        && saved.checked_at <= checked_at
        && checked_at - saved.checked_at
            < if saved.latest_version.is_some() {
                CHECK_INTERVAL
            } else {
                300
            }
    {
        return saved.latest_version.as_deref().and_then(notice);
    }
    // Uses a separate unauthenticated client. Cloud credentials are never read
    // or sent to the public release host. Network errors do not fail commands.
    let candidate = latest(&client(Duration::from_millis(900)).ok()?).await.ok();
    let saved = CheckCache {
        checked_at,
        latest_version: candidate.map(|c| c.version.to_string()),
    };
    if let Some(parent) = cache.parent()
        && fs::create_dir_all(parent).is_ok()
        && let Ok(mut file) = NamedTempFile::new_in(parent)
        && serde_json::to_writer(file.as_file_mut(), &saved).is_ok()
    {
        let _ = file.persist(&cache);
    }
    saved.latest_version.as_deref().and_then(notice)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(version: &str, os: &str, arch: &str) -> Release {
        let suffix = if os == "windows" { "zip" } else { "tar.gz" };
        let archive = format!("heterocloud-v{version}-{os}-{arch}.{suffix}");
        Release {
            tag_name: format!("v{version}"),
            draft: false,
            prerelease: false,
            assets: [archive.clone(), format!("{archive}.sha256")]
                .into_iter()
                .map(|name| Asset {
                    browser_download_url: format!("{DOWNLOADS}/v{version}/{name}"),
                    name,
                })
                .collect(),
        }
    }

    #[test]
    fn uses_all_six_native_platform_assets_and_semantic_version_order() -> Result<(), UpdateError> {
        for os in ["linux", "macos", "windows"] {
            for (arch, asset_arch) in [("x86_64", "x64"), ("aarch64", "arm64")] {
                let mut incomplete = release("9.0.0", os, asset_arch);
                incomplete.assets.pop();
                let mut prerelease = release("10.0.0", os, asset_arch);
                prerelease.prerelease = true;
                let mut foreign = release("11.0.0", os, asset_arch);
                foreign.assets[0].browser_download_url = "https://example.invalid/cli".into();
                let selected = select_release(
                    vec![
                        release("0.1.9", os, asset_arch),
                        release("0.1.10", os, asset_arch),
                        incomplete,
                        prerelease,
                        foreign,
                    ],
                    os,
                    arch,
                )?;
                assert_eq!(selected.version, Version::new(0, 1, 10));
                assert!(selected.archive.contains(&format!("-{os}-{asset_arch}.")));
            }
        }
        assert!(platform("linux", "x86").is_err());
        Ok(())
    }

    #[test]
    fn checksum_must_identify_the_selected_archive() -> Result<(), UpdateError> {
        let hash = "a".repeat(64);
        assert_eq!(
            expected_checksum(format!("{hash}  cli.tar.gz\n").as_bytes(), "cli.tar.gz")?,
            hash
        );
        assert!(
            expected_checksum(format!("{hash}  other.tar.gz").as_bytes(), "cli.tar.gz").is_err()
        );
        assert!(expected_checksum(b"bad  cli.tar.gz", "cli.tar.gz").is_err());
        assert!(
            expected_checksum(
                format!("{hash} cli.tar.gz\n{hash} cli.tar.gz").as_bytes(),
                "cli.tar.gz"
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn archives_only_produce_the_single_regular_cli_file() -> Result<(), UpdateError> {
        use flate2::{Compression, write::GzEncoder};
        for (name, kind, allowed) in [
            ("heterocloud", tar::EntryType::Regular, true),
            ("other", tar::EntryType::Regular, false),
            ("heterocloud", tar::EntryType::Symlink, false),
        ] {
            let mut archive = tempfile::tempfile()?;
            {
                let mut tar =
                    tar::Builder::new(GzEncoder::new(&mut archive, Compression::default()));
                let mut header = tar::Header::new_gnu();
                header.set_mode(0o755);
                header.set_entry_type(kind);
                header.set_size(if kind.is_file() { 3 } else { 0 });
                if !kind.is_file() {
                    header.set_link_name("other")?;
                }
                header.set_cksum();
                tar.append_data(
                    &mut header,
                    name,
                    if kind.is_file() {
                        &b"cli"[..]
                    } else {
                        &b""[..]
                    },
                )?;
                tar.into_inner()?.finish()?;
            }
            archive.seek(SeekFrom::Start(0))?;
            let mut output = tempfile::tempfile()?;
            assert_eq!(extract(&mut archive, &mut output, false).is_ok(), allowed);
        }
        for (name, allowed) in [
            ("heterocloud.exe", true),
            ("other.exe", false),
            ("dir/heterocloud.exe", false),
        ] {
            let mut archive = tempfile::tempfile()?;
            {
                let mut zip = zip::ZipWriter::new(&mut archive);
                zip.start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )?;
                zip.write_all(b"cli")?;
                zip.finish()?;
            }
            archive.seek(SeekFrom::Start(0))?;
            assert_eq!(
                extract(&mut archive, &mut tempfile::tempfile()?, true).is_ok(),
                allowed
            );
        }
        Ok(())
    }

    #[test]
    fn notices_only_offer_a_newer_stable_version() -> Result<(), Box<dyn std::error::Error>> {
        let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
        assert!(notice(&current.to_string()).is_none());
        assert!(notice("0.0.0").is_none());
        assert!(notice("9999.0.0-dev").is_none());
        assert!(notice("bad\nmessage").is_none());
        assert!(notice(&format!("{}.0.0", current.major + 1)).is_some());
        Ok(())
    }

    #[tokio::test]
    async fn corrupted_download_never_reaches_installation()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::net::TcpListener;
        let server = TcpListener::bind("127.0.0.1:0")?;
        let address = server.local_addr()?;
        let thread = std::thread::spawn(move || -> io::Result<()> {
            for (body, expect_path) in [
                (format!("{}  cli.tar.gz\n", "a".repeat(64)), "/checksum"),
                ("corrupt".into(), "/archive"),
            ] {
                let (mut stream, _) = server.accept()?;
                let mut request = [0; 4096];
                let size = stream.read(&mut request)?;
                let request = String::from_utf8_lossy(&request[..size]);
                assert!(request.starts_with(&format!("GET {expect_path} ")));
                assert!(!request.to_ascii_lowercase().contains("authorization:"));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )?;
            }
            Ok(())
        });
        let _ = rustls::crypto::ring::default_provider().install_default();
        let candidate = Candidate {
            version: Version::new(1, 0, 0),
            archive: "cli.tar.gz".into(),
            download: format!("http://{address}/archive"),
            checksum: format!("http://{address}/checksum"),
        };
        let result = download(
            &Client::builder().no_proxy().build()?,
            &candidate,
            &mut tempfile::tempfile()?,
        )
        .await;
        assert!(matches!(
            result,
            Err(UpdateError::Invalid(
                "release checksum mismatch; installed CLI was not changed"
            ))
        ));
        thread.join().map_err(|_| "test HTTP server failed")??;
        Ok(())
    }

    #[test]
    fn running_image_replace_fixture() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var_os("HETEROCLOUD_UPDATE_TEST_CHILD").is_none() {
            return Ok(());
        }
        let current = std::env::current_exe()?;
        let parent = current.parent().ok_or("fixture parent")?;
        assert!(
            parent
                .file_name()
                .ok_or("fixture name")?
                .to_string_lossy()
                .starts_with("hc-update-selfreplace-")
        );
        let mut staged = NamedTempFile::new_in(parent)?;
        staged.write_all(b"replacement")?;
        staged.as_file_mut().sync_all()?;
        install(staged.into_temp_path(), &current)?;
        assert_eq!(fs::read(&current)?, b"replacement");
        Ok(())
    }

    #[test]
    fn replacing_a_running_image_works_on_the_native_os() -> Result<(), Box<dyn std::error::Error>>
    {
        let private = tempfile::Builder::new()
            .prefix("hc-update-selfreplace-")
            .tempdir()?;
        let binary = private.path().join(if cfg!(windows) {
            "fixture.exe"
        } else {
            "fixture"
        });
        fs::copy(std::env::current_exe()?, &binary)?;
        let result = Command::new(&binary)
            .args([
                "--exact",
                "update::tests::running_image_replace_fixture",
                "--nocapture",
            ])
            .env("HETEROCLOUD_UPDATE_TEST_CHILD", "1")
            .output()?;
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
        assert_eq!(fs::read(&binary)?, b"replacement");
        Ok(())
    }
}
