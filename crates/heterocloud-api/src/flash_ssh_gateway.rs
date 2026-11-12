use super::*;
use futures_util::SinkExt;
use russh::{
    Channel, ChannelId, ChannelMsg, MethodKind, MethodSet,
    keys::HashAlg,
    server::{self, VerifiedPublicKey},
};
use std::{
    path::Path as FilePath,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
    sync::OwnedSemaphorePermit,
};
use tokio_tungstenite::tungstenite::Message as ProviderMessage;

const SSH_MAX_CONNECTIONS: usize = 64;
const SSH_MAX_CHANNELS: usize = 8;
const SSH_REVALIDATE_SECONDS: u64 = 30;
const SFTP_COMMAND: &str = "for p in /usr/lib/openssh/sftp-server /usr/lib/ssh/sftp-server /usr/libexec/openssh/sftp-server /usr/libexec/sftp-server; do if [ -x \"$p\" ]; then exec \"$p\"; fi; done; exec sftp-server";

pub struct FlashSshGateway {
    config: Arc<server::Config>,
    permits: Arc<Semaphore>,
    pub fingerprint: String,
    pub advertised_tcp: Option<(String, u16)>,
}

impl FlashSshGateway {
    pub fn from_file(path: &FilePath) -> Result<Self, Box<dyn std::error::Error>> {
        let key = russh::keys::load_secret_key(path, None)?;
        if key.public_key().algorithm().as_str() != "ssh-ed25519" {
            return Err("Flash SSH host key must be Ed25519".into());
        }
        let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
        let config = server::Config {
            keys: vec![key],
            methods: MethodSet::from(&[MethodKind::PublicKey][..]),
            max_auth_attempts: 5,
            auth_rejection_time: Duration::from_millis(300),
            auth_rejection_time_initial: Some(Duration::ZERO),
            inactivity_timeout: Some(Duration::from_secs(3600)),
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            ..Default::default()
        };
        Ok(Self {
            config: Arc::new(config),
            permits: Arc::new(Semaphore::new(SSH_MAX_CONNECTIONS)),
            fingerprint,
            advertised_tcp: None,
        })
    }

    pub fn set_advertised_tcp(
        &mut self,
        host: &str,
        port: u16,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if host.is_empty()
            || host.len() > 253
            || !host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b':'))
            || port == 0
        {
            return Err("Invalid advertised Flash SSH host or port".into());
        }
        self.advertised_tcp = Some((host.to_owned(), port));
        Ok(())
    }

    pub fn spawn_listener(state: Arc<AppState>, listener: TcpListener) {
        tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    break;
                };
                let Some(gateway) = &state.ssh_gateway else {
                    continue;
                };
                let Ok(permit) = Arc::clone(&gateway.permits).try_acquire_owned() else {
                    continue;
                };
                let state = Arc::clone(&state);
                tokio::spawn(async move { serve_stream(state, stream, None, permit).await });
            }
        });
    }
}

pub(super) async fn websocket(
    State(state): State<Arc<AppState>>,
    Path((org, service)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    // Non-browser clients need no HTTP token: the SSH signature and IAM are
    // verified inside the tunnel. Browsers must still be same-origin.
    if headers.contains_key(header::ORIGIN) {
        require_same_origin(&state.config, &headers)?;
    }
    let gateway = state.ssh_gateway.as_ref().ok_or(ApiError::NotFound)?;
    let permit = Arc::clone(&gateway.permits)
        .try_acquire_owned()
        .map_err(|_| ApiError::TooManyRequests)?;
    Ok(upgrade.max_message_size(64 * 1024).on_upgrade(move |socket| async move {
        let (ssh_stream, mut ws_stream) = tokio::io::duplex(64 * 1024);
        let ssh_state = Arc::clone(&state);
        let server = tokio::spawn(async move { serve_stream(ssh_state, ssh_stream, Some((org, service)), permit).await });
        let (mut sender, mut receiver) = socket.split();
        let mut bytes = vec![0_u8; 16 * 1024];
        loop {
            tokio::select! {
                read = ws_stream.read(&mut bytes) => {
                    let count = read.unwrap_or(0);
                    if count == 0 || sender.send(axum::extract::ws::Message::Binary(bytes[..count].to_vec().into())).await.is_err() { break; }
                }
                message = receiver.next() => match message {
                    Some(Ok(axum::extract::ws::Message::Binary(input))) => { if ws_stream.write_all(&input).await.is_err() { break; } }
                    Some(Ok(axum::extract::ws::Message::Ping(payload))) => { if sender.send(axum::extract::ws::Message::Pong(payload)).await.is_err() { break; } }
                    Some(Ok(axum::extract::ws::Message::Pong(_))) => {},
                    _ => break,
                }
            }
        }
        let _result = ws_stream.shutdown().await;
        server.abort();
    }).into_response())
}

#[derive(Clone)]
struct Identity {
    org: Uuid,
    service: Uuid,
    user: UserId,
    key_id: Uuid,
    fingerprint: String,
}

fn ssh_username(value: &str) -> Option<(Uuid, Uuid)> {
    let (org, service) = value.split_once('+')?;
    if org.len() != 36 || service.len() != 36 {
        return None;
    }
    Some((Uuid::parse_str(org).ok()?, Uuid::parse_str(service).ok()?))
}

async fn check_identity(
    state: &AppState,
    identity: &Identity,
    audit: bool,
) -> Result<(ServiceInstance, PrincipalId), ApiError> {
    let org = OrganizationId(identity.org);
    let key = state
        .store
        .authorized_flash_ssh_key(
            org,
            ServiceInstanceId(identity.service),
            &identity.fingerprint,
        )
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::Forbidden)?;
    if key.id != identity.key_id || key.user_id != identity.user.0 {
        return Err(ApiError::Forbidden);
    }
    let instance = flash_service(state, identity.org, identity.service).await?;
    // Fail closed for legacy, disabled, or malformed settings. This check is
    // shared by TCP, HTTPS tunnelling, new channels and periodic revalidation.
    if instance
        .spec
        .get("ssh_enabled")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return Err(ApiError::Forbidden);
    }
    let context = state
        .store
        .authorization_context(identity.user, org)
        .await
        .map_err(ApiError::from_store)?
        .ok_or(ApiError::Forbidden)?;
    let resource = flash_service_resource(identity.org, identity.service);
    let evaluation = evaluate_account_permissions(
        state,
        &context,
        org,
        "flash:ExecInstance",
        &resource,
        false,
        Utc::now(),
    )
    .await?;
    if audit {
        state
            .store
            .append_audit(&AuditEvent {
                organization_id: Some(org),
                principal_id: Some(context.principal_id),
                user_id: Some(identity.user),
                request_id: &Uuid::now_v7().to_string(),
                source_ip: None,
                action: "flash:ExecInstance",
                resource: &resource,
                decision: if evaluation.decision == Decision::Allow {
                    "allow"
                } else {
                    "deny"
                },
                reason: evaluation.reason,
                metadata: json!({"actor":"ssh_public_key","key_id":identity.key_id}),
            })
            .await
            .map_err(ApiError::from_store)?;
    }
    if evaluation.decision != Decision::Allow {
        return Err(ApiError::Forbidden);
    }
    Ok((instance, context.principal_id))
}

struct Handler {
    state: Arc<AppState>,
    expected: Option<(Uuid, Uuid)>,
    identity: Option<Identity>,
    verified: Arc<AtomicBool>,
    channel_permits: Arc<Semaphore>,
    pod: Option<String>,
}

async fn serve_stream<S>(
    state: Arc<AppState>,
    stream: S,
    expected: Option<(Uuid, Uuid)>,
    _permit: OwnedSemaphorePermit,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Some(gateway) = state.ssh_gateway.as_ref() else {
        return;
    };
    let verified = Arc::new(AtomicBool::new(false));
    let handler = Handler {
        state: Arc::clone(&state),
        expected,
        identity: None,
        verified: Arc::clone(&verified),
        channel_permits: Arc::new(Semaphore::new(SSH_MAX_CHANNELS)),
        pod: None,
    };
    let running = tokio::time::timeout(
        Duration::from_secs(15),
        server::run_stream(Arc::clone(&gateway.config), stream, handler),
    )
    .await;
    let Ok(Ok(running)) = running else { return };
    let handle = running.handle();
    let guard = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        if !verified.load(Ordering::Acquire) {
            let _result = handle
                .disconnect(
                    russh::Disconnect::ByApplication,
                    "Authentication timed out".into(),
                    "en".into(),
                )
                .await;
        }
    });
    let _result = running.await;
    guard.abort();
}

impl Handler {
    async fn target(&mut self) -> Option<(Identity, FlashProviderContext, String)> {
        let identity = self.identity.clone()?;
        let (instance, principal) = check_identity(&self.state, &identity, false).await.ok()?;
        let context = flash_provider_context(&instance, principal);
        let provider = self.state.flash_provider.as_ref()?;
        let pod = if let Some(pod) = &self.pod {
            pod.clone()
        } else {
            let mut items = provider.list_containers(context).await.ok()?.items;
            items.retain(|item| item.ready && item.phase == "Running");
            items.sort_by(|a, b| a.name.cmp(&b.name));
            let pod = items.into_iter().next()?.name;
            self.pod = Some(pod.clone());
            pod
        };
        Some((identity, context, pod))
    }
}

impl server::Handler for Handler {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        verified: VerifiedPublicKey<'_>,
    ) -> Result<server::Auth, Self::Error> {
        self.identity = None;
        if verified.algorithm.as_str() == "ssh-rsa" {
            return Ok(server::Auth::reject());
        }
        let Some((org, service)) = ssh_username(user) else {
            return Ok(server::Auth::reject());
        };
        if self
            .expected
            .is_some_and(|expected| expected != (org, service))
        {
            return Ok(server::Auth::reject());
        }
        let fingerprint = verified.public_key.fingerprint(HashAlg::Sha256).to_string();
        let key = self
            .state
            .store
            .authorized_flash_ssh_key(
                OrganizationId(org),
                ServiceInstanceId(service),
                &fingerprint,
            )
            .await;
        let Ok(Some(key)) = key else {
            return Ok(server::Auth::reject());
        };
        let identity = Identity {
            org,
            service,
            user: UserId(key.user_id),
            key_id: key.id,
            fingerprint,
        };
        if check_identity(&self.state, &identity, true).await.is_err() {
            return Ok(server::Auth::reject());
        }
        self.identity = Some(identity);
        self.verified.store(true, Ordering::Release);
        Ok(server::Auth::Accept)
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        _name: &str,
        _value: &str,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        // Reply while processing this specific request. The channel event
        // does not preserve want-reply reliably; responding to it later can
        // accidentally reject a subsequent exec request.
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        let Ok(permit) = Arc::clone(&self.channel_permits).try_acquire_owned() else {
            return Ok(());
        };
        let Some((identity, context, pod)) = self.target().await else {
            return Ok(());
        };
        reply.accept().await;
        let state = Arc::clone(&self.state);
        let handle = session.handle();
        tokio::spawn(async move {
            session_channel(state, identity, context, pod, channel, handle, permit).await
        });
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _origin: &str,
        _origin_port: u32,
        reply: server::ChannelOpenHandle,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if !matches!(host, "localhost" | "127.0.0.1" | "::1") || port == 0 || port > u16::MAX.into()
        {
            return Ok(());
        }
        let Ok(permit) = Arc::clone(&self.channel_permits).try_acquire_owned() else {
            return Ok(());
        };
        let Some((identity, context, pod)) = self.target().await else {
            return Ok(());
        };
        let Some(provider) = self.state.flash_provider.as_ref() else {
            return Ok(());
        };
        let Ok(mut socket) = provider
            .connect_ssh_forward(context, &pod, port as u16)
            .await
        else {
            return Ok(());
        };
        if !provider_started(&mut socket).await {
            return Ok(());
        }
        reply.accept().await;
        let state = Arc::clone(&self.state);
        let handle = session.handle();
        tokio::spawn(async move {
            forward_channel(state, identity, channel, handle, socket, permit).await
        });
        Ok(())
    }
}

async fn provider_started(socket: &mut crate::flash_provider::ProviderWebSocket) -> bool {
    matches!(tokio::time::timeout(Duration::from_secs(15), socket.next()).await,
        Ok(Some(Ok(ProviderMessage::Text(text)))) if serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("type").and_then(Value::as_str).map(str::to_owned)).as_deref() == Some("started"))
}

async fn session_channel(
    state: Arc<AppState>,
    identity: Identity,
    context: FlashProviderContext,
    pod: String,
    mut channel: Channel<server::Msg>,
    handle: server::Handle,
    _permit: OwnedSemaphorePermit,
) {
    let id = channel.id();
    let mut tty = false;
    let (mut cols, mut rows) = (80_u16, 24_u16);
    let command = loop {
        match tokio::time::timeout(Duration::from_secs(30), channel.wait()).await {
            Ok(Some(ChannelMsg::RequestPty {
                col_width,
                row_height,
                ..
            })) => {
                let (Ok(width), Ok(height)) = (u16::try_from(col_width), u16::try_from(row_height))
                else {
                    let _result = handle.channel_failure(id).await;
                    continue;
                };
                if width == 0 || height == 0 {
                    let _result = handle.channel_failure(id).await;
                    continue;
                }
                tty = true;
                cols = width;
                rows = height;
                let _result = handle.channel_success(id).await;
            }
            Ok(Some(ChannelMsg::Exec { command, .. })) => match String::from_utf8(command) {
                Ok(command)
                    if !command.is_empty()
                        && command.len() <= 16 * 1024
                        && !command.contains('\0') =>
                {
                    break Some(command);
                }
                _ => {
                    let _result = handle.channel_failure(id).await;
                    return;
                }
            },
            Ok(Some(ChannelMsg::RequestShell { .. })) => break None,
            Ok(Some(ChannelMsg::RequestSubsystem { name, .. })) if name == "sftp" && !tty => {
                break Some(SFTP_COMMAND.into());
            }
            Ok(Some(ChannelMsg::SetEnv { .. })) => {}
            Ok(Some(ChannelMsg::WindowChange {
                col_width,
                row_height,
                ..
            })) => {
                if let (Ok(width), Ok(height)) =
                    (u16::try_from(col_width), u16::try_from(row_height))
                    && width > 0
                    && height > 0
                {
                    cols = width;
                    rows = height;
                }
            }
            _ => {
                let _result = handle.channel_failure(id).await;
                return;
            }
        }
    };
    let Some(provider) = state.flash_provider.as_ref() else {
        let _result = handle.channel_failure(id).await;
        return;
    };
    let Ok(mut socket) = provider.connect_ssh_exec(context, &pod).await else {
        let _result = handle.channel_failure(id).await;
        return;
    };
    let start =
        json!({"type":"start","command":command,"tty":tty,"cols":cols,"rows":rows}).to_string();
    if socket
        .send(ProviderMessage::Text(start.into()))
        .await
        .is_err()
        || !provider_started(&mut socket).await
    {
        let _result = handle.channel_failure(id).await;
        return;
    }
    if handle.channel_success(id).await.is_err() {
        return;
    }
    let (mut sender, mut receiver) = socket.split();
    let mut recheck = tokio::time::interval(Duration::from_secs(SSH_REVALIDATE_SECONDS));
    recheck.tick().await;
    let mut exited = false;
    loop {
        tokio::select! {
            _ = recheck.tick() => if check_identity(&state, &identity, false).await.is_err() { break },
            item = channel.wait() => match item {
                Some(ChannelMsg::Data {data}) => if sender.send(ProviderMessage::Binary(data)).await.is_err() { break },
                Some(ChannelMsg::Eof) => if sender.send(ProviderMessage::Text(json!({"type":"eof"}).to_string().into())).await.is_err() { break },
                Some(ChannelMsg::WindowChange {col_width,row_height,..}) => {
                    if let (Ok(cols),Ok(rows)) = (u16::try_from(col_width),u16::try_from(row_height))
                        && cols > 0 && rows > 0 && sender.send(ProviderMessage::Text(json!({"type":"resize","cols":cols,"rows":rows}).to_string().into())).await.is_err() { break; }
                }
                Some(ChannelMsg::Close) | None => break,
                _ => {},
            },
            message = receiver.next() => match message {
                Some(Ok(ProviderMessage::Binary(data))) if !data.is_empty() => {
                    let result = match data[0] { 1 => handle.data(id, data.slice(1..)).await, 2 => handle.extended_data(id, 1, data.slice(1..)).await, _ => break };
                    if result.is_err() { break; }
                }
                Some(Ok(ProviderMessage::Text(text))) => {
                    if let Ok(value) = serde_json::from_str::<Value>(&text)
                        && value.get("type").and_then(Value::as_str) == Some("exit") {
                        let code = value.get("code").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(255);
                        let _result = handle.exit_status_request(id, code).await; exited = true; break;
                    }
                }
                Some(Ok(ProviderMessage::Ping(payload))) => { if sender.send(ProviderMessage::Pong(payload)).await.is_err() { break; } }
                Some(Ok(ProviderMessage::Pong(_))) => {},
                _ => break,
            }
        }
    }
    if !exited {
        let _result = handle.exit_status_request(id, 255).await;
    }
    let _result = handle.eof(id).await;
    let _result = handle.close(id).await;
}

async fn forward_channel(
    state: Arc<AppState>,
    identity: Identity,
    mut channel: Channel<server::Msg>,
    handle: server::Handle,
    socket: crate::flash_provider::ProviderWebSocket,
    _permit: OwnedSemaphorePermit,
) {
    let id = channel.id();
    let (mut sender, mut receiver) = socket.split();
    let mut recheck = tokio::time::interval(Duration::from_secs(SSH_REVALIDATE_SECONDS));
    recheck.tick().await;
    loop {
        tokio::select! {
            _ = recheck.tick() => if check_identity(&state, &identity, false).await.is_err() { break },
            message = channel.wait() => match message {
                Some(ChannelMsg::Data {data}) => if sender.send(ProviderMessage::Binary(data)).await.is_err() { break },
                Some(ChannelMsg::Eof) => if sender.send(ProviderMessage::Text("eof".into())).await.is_err() { break },
                Some(ChannelMsg::Close) | None => break,
                _ => {},
            },
            message = receiver.next() => match message {
                Some(Ok(ProviderMessage::Binary(data))) => if handle.data(id, data).await.is_err() { break },
                Some(Ok(ProviderMessage::Ping(payload))) => if sender.send(ProviderMessage::Pong(payload)).await.is_err() { break },
                Some(Ok(ProviderMessage::Pong(_))) => {},
                _ => break,
            }
        }
    }
    let _result = handle.eof(id).await;
    let _result = handle.close(id).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn username_requires_explicit_organization_and_service() {
        let org = Uuid::now_v7();
        let service = Uuid::now_v7();
        assert_eq!(
            ssh_username(&format!("{org}+{service}")),
            Some((org, service))
        );
        for bad in [
            "root",
            "",
            "+",
            "root@host",
            "../../etc",
            "00000000-0000-0000-0000-000000000000+bad",
        ] {
            assert!(ssh_username(bad).is_none());
        }
    }
}
