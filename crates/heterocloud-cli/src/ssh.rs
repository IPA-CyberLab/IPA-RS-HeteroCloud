use std::path::Path;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use crate::{CliError, auth};

pub(crate) async fn connect(
    endpoint: Option<&str>,
    organization: Uuid,
    service: Uuid,
    identity: Option<&Path>,
    known_hosts_file: Option<&Path>,
    command: &[String],
    insecure: bool,
) -> Result<(), CliError> {
    let endpoint = auth::resolve_endpoint(endpoint, insecure)?;
    let exe = std::env::current_exe().map_err(|source| CliError::CommandStart {
        program: "heterocloud",
        source,
    })?;
    let exe = exe
        .to_str()
        .ok_or_else(|| CliError::Authentication("CLI path is not valid Unicode".into()))?;
    let proxy = format!(
        "{} --organization-id {organization} flash ssh-proxy {service}",
        shell_quote(exe)?
    );
    let host = endpoint
        .host_str()
        .ok_or(CliError::MissingEndpoint)?
        .replace(':', "-");
    let alias = format!(
        "flash-ssh-{}.{}",
        endpoint.port_or_known_default().unwrap_or(443),
        host
    );
    let mut ssh = tokio::process::Command::new("ssh");
    ssh.arg("-o")
        .arg(format!("ProxyCommand={proxy}"))
        .arg("-o")
        .arg(format!("HostKeyAlias={alias}"))
        .arg("-l")
        .arg(format!("{organization}+{service}"));
    if let Some(identity) = identity {
        ssh.arg("-i").arg(identity);
        ssh.arg("-o").arg("IdentitiesOnly=yes");
    }
    if let Some(path) = known_hosts_file {
        ssh.arg("-o")
            .arg(format!("UserKnownHostsFile={}", path.display()));
    }
    ssh.arg(format!("flash-{service}"));
    ssh.args(command)
        .env("HETEROCLOUD_ENDPOINT", endpoint.as_str());
    if insecure {
        ssh.env("HETEROCLOUD_ALLOW_INSECURE_HTTP", "true");
    }
    let status = ssh
        .status()
        .await
        .map_err(|source| CliError::CommandStart {
            program: "ssh",
            source,
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::SshExit(
            status
                .code()
                .and_then(|n| u8::try_from(n).ok())
                .unwrap_or(255),
        ))
    }
}

#[cfg(not(windows))]
fn shell_quote(value: &str) -> Result<String, CliError> {
    if value.contains(['\0', '\n', '\r']) {
        return Err(CliError::Authentication(
            "CLI path contains a control character".into(),
        ));
    }
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}

#[cfg(windows)]
fn shell_quote(value: &str) -> Result<String, CliError> {
    if value.contains(['\0', '\n', '\r', '"', '%', '!', '^']) {
        return Err(CliError::Authentication(
            "CLI path contains a character unsupported by Windows SSH ProxyCommand".into(),
        ));
    }
    Ok(format!("\"{value}\""))
}

pub(crate) async fn proxy(
    endpoint: Option<&str>,
    organization: Uuid,
    service: Uuid,
    insecure: bool,
) -> Result<(), CliError> {
    let endpoint = auth::resolve_endpoint(endpoint, insecure)?;
    let mut url = endpoint
        .join(&format!("api/v1/flash-ssh/{organization}/{service}"))
        .map_err(|error| CliError::InvalidApiEndpoint(error.to_string()))?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme)
        .map_err(|_| CliError::InvalidApiEndpoint("invalid WebSocket scheme".into()))?;
    let _result = rustls::crypto::ring::default_provider().install_default();
    let (socket, _) = tokio_tungstenite::connect_async(url.as_str())
        .await
        .map_err(|_| CliError::Authentication("Flash SSH gateway could not be reached".into()))?;
    let (mut sender, mut receiver) = socket.split();
    let (mut input, mut output) = (tokio::io::stdin(), tokio::io::stdout());
    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
        tokio::select! {
            read = input.read(&mut buffer) => {
                let count = read.map_err(|source| CliError::CommandInput {program:"ssh proxy",source})?;
                if count == 0 { let _result = sender.send(Message::Close(None)).await; break; }
                sender.send(Message::Binary(buffer[..count].to_vec().into())).await.map_err(|_| CliError::Authentication("Flash SSH connection closed".into()))?;
            }
            message = receiver.next() => match message {
                Some(Ok(Message::Binary(data))) => {
                    output.write_all(&data).await.map_err(|source| CliError::CommandInput {program:"ssh proxy",source})?;
                    output.flush().await.map_err(|source| CliError::CommandInput {program:"ssh proxy",source})?;
                }
                Some(Ok(Message::Ping(data))) => if sender.send(Message::Pong(data)).await.is_err() { break },
                Some(Ok(Message::Pong(_))) => {},
                Some(Ok(Message::Close(_))) | None => break,
                _ => return Err(CliError::Authentication("Invalid Flash SSH transport".into())),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(windows))]
    #[test]
    fn quotes_proxy_executable_without_shell_expansion() -> Result<(), CliError> {
        assert_eq!(
            shell_quote("/path/my'$(touch nope)")?,
            "'/path/my'\\''$(touch nope)'"
        );
        assert!(shell_quote("bad\npath").is_err());
        Ok(())
    }
}
