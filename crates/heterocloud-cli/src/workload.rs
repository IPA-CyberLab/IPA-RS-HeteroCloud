//! Automatic credentials from a rotating workload identity. No login or key file.
use crate::CliError;
use reqwest::{Client, Url};
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

pub(crate) struct WorkloadCredentials {
    file: PathBuf,
    endpoint: Url,
    organization_id: Uuid,
    cache: Mutex<Option<CachedCredential>>,
}
struct CachedCredential {
    token: String,
    refresh_at: Instant,
}
#[derive(Deserialize)]
struct Exchange {
    access_token: String,
    expires_in: u64,
    token_type: String,
    organization_id: Uuid,
}

impl WorkloadCredentials {
    pub fn from_environment(origin: &Url, organization_id: Uuid) -> Result<Self, CliError> {
        if origin.scheme() != "https" {
            return Err(error("workload identity requires an HTTPS endpoint"));
        }
        let file = std::env::var_os("HETEROCLOUD_WORKLOAD_TOKEN_FILE")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                CliError::Authentication("workload identity token file is missing".into())
            })?;
        Ok(Self {
            file: PathBuf::from(file),
            endpoint: origin
                .join("api/v1/auth/workload/token")
                .map_err(|e| CliError::InvalidApiEndpoint(e.to_string()))?,
            organization_id,
            cache: Mutex::new(None),
        })
    }
    pub async fn token(&self, http: &Client) -> Result<String, CliError> {
        let mut cache = self.cache.lock().await;
        if let Some(credential) = cache.as_ref().filter(|c| c.refresh_at > Instant::now()) {
            return Ok(credential.token.clone());
        }
        // Follow the projected-volume symlink on each refresh; do not pin its inode.
        let metadata = tokio::fs::metadata(&self.file)
            .await
            .map_err(|_| error("cannot read workload identity token"))?;
        if !metadata.is_file() || metadata.len() > 16384 {
            return Err(error("invalid workload identity file"));
        }
        let subject = tokio::fs::read_to_string(&self.file)
            .await
            .map_err(|_| error("cannot read workload identity token"))?;
        if subject.trim().is_empty() || subject.len() > 16384 {
            return Err(error("invalid workload identity token"));
        }
        let mut response = http
            .post(self.endpoint.clone())
            .form(&[
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:token-exchange",
                ),
                ("subject_token_type", "urn:ietf:params:oauth:token-type:jwt"),
                ("subject_token", subject.trim()),
            ])
            .send()
            .await
            .map_err(|_| error("workload token exchange could not reach HeteroCloud"))?;
        if !response.status().is_success() {
            return Err(error(
                "workload token exchange was rejected; check the attached task role and Pod identity",
            ));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| error("workload token response failed"))?
        {
            if chunk.len() > 32768_usize.saturating_sub(body.len()) {
                return Err(error("invalid workload token response"));
            }
            body.extend_from_slice(&chunk);
        }
        let exchanged: Exchange =
            serde_json::from_slice(&body).map_err(|_| error("invalid workload token response"))?;
        if !exchanged.access_token.starts_with("hcw_")
            || exchanged.access_token.len() > 256
            || exchanged.access_token.chars().any(char::is_whitespace)
            || exchanged.token_type != "Bearer"
            || !(1..=900).contains(&exchanged.expires_in)
            || exchanged.organization_id != self.organization_id
        {
            return Err(error(
                "workload token response does not match this organization",
            ));
        }
        let token = exchanged.access_token;
        *cache = Some(CachedCredential {
            token: token.clone(),
            refresh_at: Instant::now()
                + Duration::from_secs(exchanged.expires_in.saturating_sub(60).max(1)),
        });
        Ok(token)
    }
}
fn error(message: &str) -> CliError {
    CliError::Authentication(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex as StdMutex},
        thread,
    };

    type TestServer = (
        Url,
        Arc<StdMutex<Vec<String>>>,
        thread::JoinHandle<std::io::Result<()>>,
    );
    fn server(replies: Vec<serde_json::Value>) -> Result<TestServer, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let origin = Url::parse(&format!("http://{}/", listener.local_addr()?))?;
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let captured = requests.clone();
        let task = thread::spawn(move || {
            for reply in replies {
                let (mut stream, _) = listener.accept()?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut request = Vec::new();
                loop {
                    let mut bytes = [0; 1024];
                    let len = stream.read(&mut bytes)?;
                    if len == 0 {
                        return Err(std::io::Error::other("incomplete request"));
                    }
                    request.extend_from_slice(&bytes[..len]);
                    let text = String::from_utf8_lossy(&request);
                    if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                        let length = headers
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .and_then(|n| n.parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            break;
                        }
                    }
                }
                captured
                    .lock()
                    .map_err(|_| std::io::Error::other("lock"))?
                    .push(String::from_utf8_lossy(&request).into_owned());
                let body = reply.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )?;
            }
            Ok(())
        });
        Ok((origin, requests, task))
    }
    #[tokio::test]
    async fn caches_short_tokens_and_reads_the_rotated_identity_on_refresh()
    -> Result<(), Box<dyn std::error::Error>> {
        let org = Uuid::from_u128(1);
        let reply = |token: &str| json!({"access_token":token,"token_type":"Bearer","expires_in":900,"organization_id":org});
        let (origin, requests, server) = server(vec![reply("hcw_first"), reply("hcw_rotated")])?;
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("identity");
        std::fs::write(&file, "pod-bound-first")?;
        let credentials = WorkloadCredentials {
            file: file.clone(),
            endpoint: origin.join("api/v1/auth/workload/token")?,
            organization_id: org,
            cache: Mutex::new(None),
        };
        let http = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        assert_eq!(credentials.token(&http).await?, "hcw_first");
        assert_eq!(credentials.token(&http).await?, "hcw_first");
        let rotated = dir.path().join("rotated");
        std::fs::write(&rotated, "pod-bound-rotated")?;
        std::fs::rename(rotated, file)?;
        *credentials.cache.lock().await = None;
        assert_eq!(credentials.token(&http).await?, "hcw_rotated");
        server.join().map_err(|_| "server")??;
        let requests = requests.lock().map_err(|_| "lock")?;
        assert_eq!(requests.len(), 2);
        assert!(requests[0].contains("subject_token=pod-bound-first"));
        assert!(requests[1].contains("subject_token=pod-bound-rotated"));
        assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
        Ok(())
    }
    #[tokio::test]
    async fn rejects_a_response_for_another_organization_and_plaintext_endpoint()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(
            WorkloadCredentials::from_environment(
                &Url::parse("http://example.test/")?,
                Uuid::nil()
            )
            .is_err()
        );
        let (origin, _, server) = server(vec![
            json!({"access_token":"hcw_do-not-display","token_type":"Bearer","expires_in":900,"organization_id":Uuid::from_u128(2)}),
        ])?;
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("identity");
        std::fs::write(&file, "private-subject")?;
        let credentials = WorkloadCredentials {
            file,
            endpoint: origin.join("api/v1/auth/workload/token")?,
            organization_id: Uuid::from_u128(1),
            cache: Mutex::new(None),
        };
        let error = credentials
            .token(&Client::builder().no_proxy().build()?)
            .await
            .err()
            .ok_or("rejected")?
            .to_string();
        assert!(!error.contains("private-subject"));
        assert!(!error.contains("hcw_do-not-display"));
        server.join().map_err(|_| "server")??;
        Ok(())
    }
}
