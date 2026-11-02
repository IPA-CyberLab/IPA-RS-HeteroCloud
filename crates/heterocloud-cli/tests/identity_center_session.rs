use std::{
    error::Error,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const MANAGEMENT: &str = "00000000-0000-0000-0000-000000000001";
const TARGET: &str = "00000000-0000-0000-0000-000000000002";
const ASSIGNMENT: &str = "00000000-0000-0000-0000-000000000003";
const ROLE_TOKEN: &str = "hcr_test-only-role-session";
const LOGIN_TOKEN: &str = "hcu_0123456789abcdef_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcd";

fn serve(mut stream: TcpStream) -> Result<String, Box<dyn Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buffer = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err("request ended before headers".into());
        }
        buffer.extend_from_slice(&chunk[..n]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if buffer.len() > 32_768 {
            return Err("oversized request".into());
        }
    }
    let request = String::from_utf8_lossy(&buffer);
    let request_line = request.lines().next().ok_or("missing request")?;
    let authorization = request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then_some(value.trim())
        })
        .ok_or("missing authorization")?;
    let body = if request_line.starts_with(&format!("POST /api/v1/organizations/{MANAGEMENT}/identity-center/my-assignments/{ASSIGNMENT}/session ")) {
        if authorization != format!("Bearer {LOGIN_TOKEN}") { return Err("wrong login token".into()); }
        serde_json::json!({"access_token":ROLE_TOKEN,"session":{"account_id":TARGET,"expires_at":"2099-01-01T00:00:00Z"}})
    } else if request_line.starts_with("GET /api/v1/auth/identity ") {
        if authorization != format!("Bearer {ROLE_TOKEN}") { return Err("wrong role token".into()); }
        serde_json::json!({"type":"role_session","organization_id":TARGET})
    } else { return Err("unexpected request".into()); };
    let data = serde_json::to_vec(&body)?;
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        data.len()
    )?;
    stream.write_all(&data)?;
    Ok(request_line.to_owned())
}

#[test]
fn saved_login_can_issue_and_use_a_private_identity_center_session() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let endpoint = format!("http://{}/", listener.local_addr()?);
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut count = 0;
        while count < 2 && Instant::now() < deadline {
            match listener.accept() {
                Ok((stream, _)) => {
                    let result = serve(stream).map_err(|error| error.to_string());
                    let failed = result.is_err();
                    let _ = sender.send(result);
                    if failed {
                        return;
                    }
                    count += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                    return;
                }
            }
        }
    });
    let temp = tempfile::tempdir()?;
    let credentials = temp.path().join("credentials.json");
    let profile = serde_json::json!({"access_token":LOGIN_TOKEN,"organization_id":MANAGEMENT,"user_email":"test@example.test","user_display_name":"Test","organization_name":"Test","organization_slug":"test","expires_at":"2099-01-01T00:00:00Z"});
    fs::write(
        &credentials,
        serde_json::to_vec(
            &serde_json::json!({"version":1,"active_endpoint":endpoint,"profiles":{endpoint.clone():profile}}),
        )?,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&credentials, fs::Permissions::from_mode(0o600))?;
    }
    let role_file = temp.path().join("role.token");
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_heterocloud"));
        command
            .env("HETEROCLOUD_CREDENTIALS_FILE", &credentials)
            .env("HETEROCLOUD_ENDPOINT", &endpoint)
            .env("HETEROCLOUD_ORGANIZATION_ID", MANAGEMENT)
            .env("HETEROCLOUD_ALLOW_INSECURE_HTTP", "1")
            .env("HETEROCLOUD_NO_UPDATE_CHECK", "1")
            .env_remove("HETEROCLOUD_API_KEY")
            .env_remove("HETEROCLOUD_API_KEY_FILE")
            .env_remove("HETEROCLOUD_WORKLOAD_TOKEN_FILE");
        command
    };
    let issued = command()
        .args([
            "iam",
            "identity-center",
            "start-session",
            ASSIGNMENT,
            "--output-file",
        ])
        .arg(&role_file)
        .output()?;
    assert!(
        issued.status.success(),
        "{}",
        String::from_utf8_lossy(&issued.stderr)
    );
    assert!(!String::from_utf8_lossy(&issued.stdout).contains(ROLE_TOKEN));
    assert!(!String::from_utf8_lossy(&issued.stderr).contains(ROLE_TOKEN));
    assert_eq!(fs::read_to_string(&role_file)?, ROLE_TOKEN);
    let metadata: serde_json::Value = serde_json::from_slice(&issued.stdout)?;
    assert_eq!(metadata["account_id"], TARGET);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&role_file)?.permissions().mode() & 0o777,
            0o600
        );
    }
    let refused = command()
        .args([
            "iam",
            "identity-center",
            "start-session",
            ASSIGNMENT,
            "--output-file",
        ])
        .arg(&role_file)
        .output()?;
    assert!(
        !refused.status.success(),
        "existing token file must not be overwritten"
    );
    assert_eq!(fs::read_to_string(&role_file)?, ROLE_TOKEN);
    let whoami = command()
        .env("HETEROCLOUD_ORGANIZATION_ID", TARGET)
        .env("HETEROCLOUD_API_KEY_FILE", &role_file)
        .args(["iam", "whoami"])
        .output()?;
    assert!(
        whoami.status.success(),
        "{}",
        String::from_utf8_lossy(&whoami.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&whoami.stdout)?["type"],
        "role_session"
    );
    for _ in 0..2 {
        receiver
            .recv_timeout(Duration::from_secs(5))?
            .map_err(std::io::Error::other)?;
    }
    Ok(())
}
