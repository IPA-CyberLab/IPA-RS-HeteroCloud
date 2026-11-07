use std::{
    env,
    error::Error,
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};
use futures_util::StreamExt;
use heterocloud_api::{
    config::RuntimeConfig,
    flash_provider::FlashProviderProxy,
    flow_access::FlowAccessSigner,
    routes::{AppState, FlashSshGateway},
};
use heterocloud_domain::{OrganizationId, ProjectId, ServiceInstanceId, UserId};
use heterocloud_provider::ProviderSigner;
use heterocloud_store::{BootstrapAdmin, Store};
use russh::keys::{HashAlg, PublicKey};
use secrecy::SecretString;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use url::Url;

type TestResult = Result<(), Box<dyn Error>>;

async fn containers(headers: HeaderMap) -> impl IntoResponse {
    if !headers
        .get("authorization")
        .is_some_and(|h| h.as_bytes().starts_with(b"Bearer "))
    {
        return (StatusCode::UNAUTHORIZED, Json(json!({}))).into_response();
    }
    Json(json!({"items":[{"name":"flash-test-pod", "phase":"Running", "ready":true}]}))
        .into_response()
}

async fn mock_exec(
    State(count): State<Arc<AtomicUsize>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    if !headers
        .get("authorization")
        .is_some_and(|h| h.as_bytes().starts_with(b"Bearer "))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    upgrade
        .on_upgrade(move |mut socket| async move {
            let Some(Ok(Message::Text(start))) = socket.next().await else {
                return;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&start) else {
                return;
            };
            if value["type"] != "start"
                || !["printf expected", "hold-open"]
                    .contains(&value["command"].as_str().unwrap_or(""))
            {
                return;
            }
            count.fetch_add(1, Ordering::SeqCst);
            let _ = socket
                .send(Message::Text(json!({"type":"started"}).to_string().into()))
                .await;
            if value["command"] == "hold-open" {
                while socket.next().await.is_some() {}
                return;
            }
            let mut output = vec![1];
            output.extend_from_slice(b"expected\n");
            let _ = socket.send(Message::Binary(output.into())).await;
            let _ = socket
                .send(Message::Text(
                    json!({"type":"exit","code":42}).to_string().into(),
                ))
                .await;
        })
        .into_response()
}

async fn mock_forward(headers: HeaderMap, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    if !headers
        .get("authorization")
        .is_some_and(|h| h.as_bytes().starts_with(b"Bearer "))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    upgrade
        .on_upgrade(|mut socket| async move {
            let _ = socket
                .send(Message::Text(json!({"type":"started"}).to_string().into()))
                .await;
            while let Some(Ok(Message::Binary(bytes))) = socket.next().await {
                if socket.send(Message::Binary(bytes)).await.is_err() {
                    break;
                }
            }
        })
        .into_response()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_openssh_requires_assigned_key_and_preserves_output_and_exit_status() -> TestResult {
    let Ok(database_url) = env::var("HETEROCLOUD_TEST_DATABASE_URL") else {
        return Ok(());
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let store = Store::connect(&database_url, 8).await?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(store.pool())
        .await?;
    if !database.starts_with("heterocloud_test_") {
        return Err("a disposable heterocloud_test_ database is required".into());
    }
    sqlx::query("DROP SCHEMA public CASCADE")
        .execute(store.pool())
        .await?;
    sqlx::query("CREATE SCHEMA public")
        .execute(store.pool())
        .await?;
    store.migrate().await?;
    let owner = store
        .bootstrap_admin(BootstrapAdmin {
            email: "ssh@example.test",
            display_name: "SSH owner",
            password_hash: "test-only",
            organization_slug: "ssh-test",
            organization_name: "SSH Test",
        })
        .await?;
    let membership = owner.memberships.first().ok_or("missing membership")?;
    let org = OrganizationId(membership.organization_id.0);
    let project = store.create_project(org, "ssh-test", "SSH Test").await?;
    let instance = store
        .create_service_instance(
            org,
            ProjectId(project.id.0),
            membership.principal_id,
            "flash",
            "ssh-workspace",
            json!({
                "region":"test", "image":"example/workspace:v1", "replicas":1, "cpu_millis":100,
                "memory_mib":256, "ephemeral_storage_gib":1, "ports":[],
                "exposure":{"type":"internal","traffic_mode":"forwarded"},
                "env":{}, "command":[], "args":[], "metadata":{}
            }),
        )
        .await?;

    let tmp = tempfile::tempdir()?;
    let host_key = tmp.path().join("host-key");
    let user_key = tmp.path().join("user-key");
    let member_key = tmp.path().join("member-key");
    for path in [&host_key, &user_key, &member_key] {
        let status = tokio::process::Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", ""])
            .arg("-f")
            .arg(path)
            .status()
            .await?;
        if !status.success() {
            return Err("ssh-keygen failed".into());
        }
    }
    let public_key = fs::read_to_string(user_key.with_extension("pub"))?;
    let fingerprint = PublicKey::from_openssh(&public_key)?
        .fingerprint(HashAlg::Sha256)
        .to_string();
    let key = store
        .create_flash_ssh_key(
            org,
            owner.user.id,
            "test",
            public_key.trim(),
            &fingerprint,
            None,
        )
        .await?;

    let count = Arc::new(AtomicUsize::new(0));
    let mock = Router::new()
        .route(
            "/internal/v1/service-instances/{id}/containers",
            get(containers),
        )
        .route(
            "/internal/v1/service-instances/{id}/ssh-exec",
            get(mock_exec),
        )
        .route(
            "/internal/v1/service-instances/{id}/ssh-forward",
            get(mock_forward),
        )
        .with_state(Arc::clone(&count));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let provider_address = listener.local_addr()?;
    let provider_task = tokio::spawn(async move {
        let _ = axum::serve(listener, mock).await;
    });
    let signer = ProviderSigner::from_ed25519_pem("test","flash","ssh-test",b"-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEICKoEEWPLg2OazcyTWzBEw/mMPPXatNOUcEUWDHo2y0Y\n-----END PRIVATE KEY-----\n")?;
    let proxy = FlashProviderProxy::new(
        Url::parse(&format!("http://{provider_address}/"))?,
        signer,
        reqwest::Client::builder().no_proxy().build()?,
    );
    let gateway = Arc::new(FlashSshGateway::from_file(&host_key)?);
    let state = Arc::new(AppState {
        store: store.clone(),
        config: RuntimeConfig {
            public_origin: Url::parse("http://cloud.example.test")?,
            secret_manager_origin: None,
            allowed_origins: vec!["http://cloud.example.test".into()],
            trusted_proxy_networks: vec![],
            secure_cookie: false,
            session_ttl: Duration::from_secs(3600),
            cli_token_ttl: Duration::from_secs(3600),
            csrf_key: SecretString::from("test-csrf-key-at-least-32-bytes"),
            flow_access_signer: FlowAccessSigner::new(
                "test",
                "test",
                SecretString::from("test-flow-signing-key-at-least-32-bytes"),
            )?,
            flow_public_endpoints: vec![Url::parse("https://flow.example.test")?],
            flow_internal_endpoint: Url::parse("http://flow.example.test")?,
            oidc: None,
            owner_origin: None,
            owner_email: None,
            owner_console_mode: false,
            owner_allowed_networks: vec![],
        },
        flow_client: reqwest::Client::builder().no_proxy().build()?,
        flash_provider: Some(Arc::new(proxy)),
        vpc_provider: None,
        syouyu_provider: None,
        registry: None,
        registration_limiter: Arc::new(Semaphore::new(2)),
        workload_identity: None,
        ssh_gateway: Some(gateway),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let ssh_address = listener.local_addr()?;
    let host_public = fs::read_to_string(host_key.with_extension("pub"))?;
    let known_hosts = tmp.path().join("known_hosts");
    fs::write(
        &known_hosts,
        format!(
            "[{0}]:{1} {2}",
            ssh_address.ip(),
            ssh_address.port(),
            host_public
        ),
    )?;
    FlashSshGateway::spawn_listener(Arc::clone(&state), listener);
    let ssh = |command: &str, identity: &std::path::Path| {
        let mut cmd = tokio::process::Command::new("ssh");
        cmd.args([
            "-F",
            "/dev/null",
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "SendEnv=HETEROCLOUD_SSH_TEST_ENV",
        ])
        .arg("-o")
        .arg(format!("UserKnownHostsFile={}", known_hosts.display()))
        .arg("-i")
        .arg(identity)
        .arg("-p")
        .arg(ssh_address.port().to_string())
        .arg("-l")
        .arg(format!("{}+{}", org, instance.id))
        .arg("127.0.0.1")
        .arg(command)
        .env("HETEROCLOUD_SSH_TEST_ENV", "ignored");
        cmd
    };
    let denied = tokio::time::timeout(
        Duration::from_secs(20),
        ssh("printf expected", &user_key).output(),
    )
    .await??;
    assert!(!denied.status.success(), "an unassigned key must be denied");
    assert_eq!(count.load(Ordering::SeqCst), 0);
    store
        .set_flash_service_ssh_key(org, ServiceInstanceId(instance.id.0), key.id, true)
        .await?;
    let allowed = tokio::time::timeout(
        Duration::from_secs(20),
        ssh("printf expected", &user_key).output(),
    )
    .await??;
    assert_eq!(
        allowed.status.code(),
        Some(42),
        "{}",
        String::from_utf8_lossy(&allowed.stderr)
    );
    assert_eq!(allowed.stdout, b"expected\n");
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let member_user = uuid::Uuid::now_v7();
    let member_principal = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO users(id,email,display_name,password_hash,status) VALUES($1,'ssh-member@example.test','Member','test-only','active')")
        .bind(member_user).execute(store.pool()).await?;
    sqlx::query("INSERT INTO principals(id,organization_id,kind,name,user_id) VALUES($1,$2,'user','ssh-member',$3)")
        .bind(member_principal).bind(org.0).bind(member_user).execute(store.pool()).await?;
    sqlx::query("INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES($1,$2,$3,'member')")
        .bind(org.0).bind(member_user).bind(member_principal).execute(store.pool()).await?;
    let member_public = fs::read_to_string(member_key.with_extension("pub"))?;
    let member_fingerprint = PublicKey::from_openssh(&member_public)?
        .fingerprint(HashAlg::Sha256)
        .to_string();
    let member = store
        .create_flash_ssh_key(
            org,
            UserId(member_user),
            "member",
            member_public.trim(),
            &member_fingerprint,
            None,
        )
        .await?;
    store
        .set_flash_service_ssh_key(org, ServiceInstanceId(instance.id.0), member.id, true)
        .await?;
    let missing_permission = tokio::time::timeout(
        Duration::from_secs(20),
        ssh("printf expected", &member_key).output(),
    )
    .await??;
    assert!(
        !missing_permission.status.success(),
        "a selected key must not bypass IAM"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let local = TcpListener::bind("127.0.0.1:0").await?;
    let local_port = local.local_addr()?.port();
    drop(local);
    let mut forwarding = tokio::process::Command::new("ssh");
    forwarding
        .args([
            "-F",
            "/dev/null",
            "-N",
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ExitOnForwardFailure=yes",
        ])
        .arg("-o")
        .arg(format!("UserKnownHostsFile={}", known_hosts.display()))
        .arg("-i")
        .arg(&user_key)
        .arg("-p")
        .arg(ssh_address.port().to_string())
        .arg("-L")
        .arg(format!("127.0.0.1:{local_port}:127.0.0.1:1234"))
        .arg("-l")
        .arg(format!("{}+{}", org, instance.id))
        .arg("127.0.0.1")
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut forwarding = forwarding.spawn()?;
    let mut forwarded = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(stream) = TcpStream::connect(("127.0.0.1", local_port)).await {
                break stream;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;
    forwarded.write_all(b"forwarded").await?;
    let mut response = [0_u8; 9];
    tokio::time::timeout(Duration::from_secs(10), forwarded.read_exact(&mut response)).await??;
    assert_eq!(&response, b"forwarded");
    drop(forwarded);
    forwarding.kill().await?;
    let mut hold = ssh("hold-open", &user_key);
    hold.kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut hold = hold.spawn()?;
    tokio::time::timeout(Duration::from_secs(15), async {
        while count.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await?;
    store
        .set_flash_service_ssh_key(org, ServiceInstanceId(instance.id.0), key.id, false)
        .await?;
    let stopped = tokio::time::timeout(Duration::from_secs(40), hold.wait()).await??;
    assert!(
        !stopped.success(),
        "revoking a key must close an existing session"
    );
    store
        .set_flash_service_ssh_key(org, ServiceInstanceId(instance.id.0), key.id, true)
        .await?;
    if let Ok(cli_path) = env::var("HETEROCLOUD_TEST_CLI") {
        let api_listener = TcpListener::bind("127.0.0.1:0").await?;
        let api_address = api_listener.local_addr()?;
        let app = heterocloud_api::app(Arc::clone(&state), None);
        let api_task = tokio::spawn(async move {
            let _ = axum::serve(api_listener, app).await;
        });
        let hosts = format!(
            "[{0}]:{1} {2}flash-ssh-{3}.{0} {2}",
            ssh_address.ip(),
            ssh_address.port(),
            host_public,
            api_address.port()
        );
        fs::write(&known_hosts, hosts)?;
        let output = tokio::time::timeout(
            Duration::from_secs(30),
            tokio::process::Command::new(cli_path)
                .arg("--endpoint")
                .arg(format!("http://{api_address}"))
                .arg("--organization-id")
                .arg(org.to_string())
                .arg("--allow-insecure-http")
                .arg("flash")
                .arg("ssh")
                .arg(instance.id.to_string())
                .arg("-i")
                .arg(&user_key)
                .arg("--known-hosts-file")
                .arg(&known_hosts)
                .args(["--", "printf expected"])
                .output(),
        )
        .await??;
        assert_eq!(
            output.status.code(),
            Some(42),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"expected\n");
        assert_eq!(count.load(Ordering::SeqCst), 3);
        api_task.abort();
    }
    store
        .set_flash_service_ssh_key(org, ServiceInstanceId(instance.id.0), key.id, false)
        .await?;
    let revoked = tokio::time::timeout(
        Duration::from_secs(20),
        ssh("printf expected", &user_key).output(),
    )
    .await??;
    assert!(!revoked.status.success(), "a detached key must be denied");
    assert_eq!(
        count.load(Ordering::SeqCst),
        if env::var("HETEROCLOUD_TEST_CLI").is_ok() {
            3
        } else {
            2
        }
    );
    provider_task.abort();
    Ok(())
}
