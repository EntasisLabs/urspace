use std::io::Write as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

const CONTROL_VERSION: u8 = 1;
const MAX_CONTROL_MESSAGE_BYTES: usize = 64 * 1024;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum ControlAction {
    Status,
    Invite,
    Sessions,
    Kick { session_id: String },
    KickAll,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionView {
    pub session_id: String,
    pub endpoint_id: String,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlResponse {
    pub ok: bool,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<SessionView>,
}

impl ControlResponse {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            share_url: None,
            site_id: None,
            source: None,
            sessions: Vec::new(),
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            share_url: None,
            site_id: None,
            source: None,
            sessions: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlEndpoint {
    version: u8,
    address: SocketAddr,
    token: String,
    pid: u32,
    site_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlRequest {
    token: String,
    command: ControlAction,
}

pub struct ControlServer {
    listener: TcpListener,
    token: [u8; 32],
    token_text: String,
    endpoint_path: PathBuf,
}

impl ControlServer {
    pub async fn ensure_available(name: &str) -> Result<()> {
        clear_stale_control(&control_path(name)?).await
    }

    pub async fn bind(name: &str, site_id: &str) -> Result<Self> {
        let endpoint_path = control_path(name)?;
        Self::bind_at_path(endpoint_path, site_id).await
    }

    async fn bind_at_path(endpoint_path: PathBuf, site_id: &str) -> Result<Self> {
        clear_stale_control(&endpoint_path).await?;

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("bind local service control")?;
        let token: [u8; 32] = rand::rng().random();
        let token_text = URL_SAFE_NO_PAD.encode(token);
        let endpoint = ControlEndpoint {
            version: CONTROL_VERSION,
            address: listener.local_addr()?,
            token: token_text.clone(),
            pid: std::process::id(),
            site_id: site_id.to_owned(),
        };
        let encoded = serde_json::to_vec(&endpoint).context("encode service control endpoint")?;
        write_private_new(&endpoint_path, &encoded)?;
        Ok(Self {
            listener,
            token,
            token_text,
            endpoint_path,
        })
    }

    pub async fn accept(&self) -> Result<(TcpStream, ControlAction)> {
        loop {
            let (mut stream, peer) = self.listener.accept().await?;
            if !peer.ip().is_loopback() {
                continue;
            }
            let request = tokio::time::timeout(CONTROL_TIMEOUT, read_request(&mut stream))
                .await
                .context("service control request timed out")
                .and_then(|request| request);
            match request {
                Ok(request) if token_matches(&self.token, &request.token) => {
                    return Ok((stream, request.command));
                }
                Ok(_) => {
                    let _ = send_response(
                        &mut stream,
                        &ControlResponse::failure("service control authentication failed"),
                    )
                    .await;
                }
                Err(error) => {
                    let _ = send_response(
                        &mut stream,
                        &ControlResponse::failure(format!(
                            "invalid service control request: {error}"
                        )),
                    )
                    .await;
                }
            }
        }
    }
}

async fn clear_stale_control(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if request_at_path(path, ControlAction::Status).await.is_ok()
        || endpoint_is_listening(path).await
    {
        // A listener that has not answered yet may still be completing startup.
        // Never remove its token file and let a second process claim the name.
        bail!("an Urspace service is already starting or running with this name");
    }
    std::fs::remove_file(path)
        .with_context(|| format!("remove stale service control {}", path.display()))?;
    Ok(())
}

async fn endpoint_is_listening(path: &Path) -> bool {
    let Ok(encoded) = std::fs::read(path) else {
        return false;
    };
    let Ok(endpoint) = serde_json::from_slice::<ControlEndpoint>(&encoded) else {
        return false;
    };
    endpoint.version == CONTROL_VERSION
        && endpoint.address.ip().is_loopback()
        && TcpStream::connect(endpoint.address).await.is_ok()
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        let owns_file = std::fs::read(&self.endpoint_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ControlEndpoint>(&bytes).ok())
            .is_some_and(|endpoint| endpoint.token == self.token_text);
        if owns_file {
            let _ = std::fs::remove_file(&self.endpoint_path);
        }
    }
}

pub async fn request(name: &str, action: ControlAction) -> Result<ControlResponse> {
    request_at_path(&control_path(name)?, action).await
}

pub async fn send_response(stream: &mut TcpStream, response: &ControlResponse) -> Result<()> {
    let encoded = serde_json::to_vec(response).context("encode service control response")?;
    if encoded.len() > MAX_CONTROL_MESSAGE_BYTES {
        bail!("service control response is too large");
    }
    stream.write_all(&encoded).await?;
    stream.shutdown().await?;
    Ok(())
}

pub fn service_directory(name: &str) -> Result<PathBuf> {
    Ok(data_directory()?.join("services").join(name))
}

pub fn data_directory() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("URSPACE_DATA_DIR") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            bail!("URSPACE_DATA_DIR must be an absolute path");
        }
        return Ok(path);
    }
    Ok(dirs::data_local_dir()
        .context("local data directory is unavailable")?
        .join("urspace"))
}

fn control_path(name: &str) -> Result<PathBuf> {
    Ok(service_directory(name)?.join("control.json"))
}

async fn request_at_path(path: &Path, action: ControlAction) -> Result<ControlResponse> {
    let encoded = std::fs::read(path)
        .with_context(|| format!("read service control endpoint {}", path.display()))?;
    if encoded.len() > MAX_CONTROL_MESSAGE_BYTES {
        bail!("service control endpoint is too large");
    }
    let endpoint: ControlEndpoint =
        serde_json::from_slice(&encoded).context("parse service control endpoint")?;
    if endpoint.version != CONTROL_VERSION || !endpoint.address.ip().is_loopback() {
        bail!("service control endpoint is invalid");
    }
    let request = ControlRequest {
        token: endpoint.token,
        command: action,
    };
    let encoded = serde_json::to_vec(&request).context("encode service control request")?;
    let response = tokio::time::timeout(CONTROL_TIMEOUT, async {
        let mut stream = TcpStream::connect(endpoint.address)
            .await
            .context("connect to Urspace service")?;
        stream.write_all(&encoded).await?;
        stream.shutdown().await?;
        let mut response = Vec::new();
        (&mut stream)
            .take((MAX_CONTROL_MESSAGE_BYTES + 1) as u64)
            .read_to_end(&mut response)
            .await?;
        if response.len() > MAX_CONTROL_MESSAGE_BYTES {
            bail!("service control response is too large");
        }
        serde_json::from_slice(&response).context("parse service control response")
    })
    .await
    .context("Urspace service control timed out")??;
    Ok(response)
}

async fn read_request(stream: &mut TcpStream) -> Result<ControlRequest> {
    let mut encoded = Vec::new();
    stream
        .take((MAX_CONTROL_MESSAGE_BYTES + 1) as u64)
        .read_to_end(&mut encoded)
        .await?;
    if encoded.len() > MAX_CONTROL_MESSAGE_BYTES {
        bail!("service control request is too large");
    }
    serde_json::from_slice(&encoded).context("parse service control request")
}

fn token_matches(expected: &[u8; 32], candidate: &str) -> bool {
    URL_SAFE_NO_PAD
        .decode(candidate)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .is_some_and(|candidate| expected.ct_eq(&candidate).into())
}

fn write_private_new(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create service directory {}", parent.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("create service control endpoint {}", path.display()))?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_tokens_require_the_exact_random_value() {
        let token = [7_u8; 32];
        assert!(token_matches(&token, &URL_SAFE_NO_PAD.encode(token)));
        assert!(!token_matches(&token, &URL_SAFE_NO_PAD.encode([8_u8; 32])));
        assert!(!token_matches(&token, "not-a-token"));
    }

    #[test]
    fn control_messages_do_not_accept_unknown_fields() {
        let request = format!(
            "{{\"token\":\"{}\",\"command\":{{\"action\":\"status\"}},\"extra\":true}}",
            URL_SAFE_NO_PAD.encode([9_u8; 32])
        );
        assert!(serde_json::from_str::<ControlRequest>(&request).is_err());
    }

    #[tokio::test]
    async fn authenticated_control_round_trip_stays_on_loopback() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.json");
        let server = ControlServer::bind_at_path(path.clone(), "site-id")
            .await
            .unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, action) = server.accept().await.unwrap();
            assert!(matches!(action, ControlAction::Status));
            send_response(&mut stream, &ControlResponse::success("running"))
                .await
                .unwrap();
        });
        let response = request_at_path(&path, ControlAction::Status).await.unwrap();
        assert!(response.ok);
        assert_eq!(response.message, "running");
        task.await.unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn a_starting_listener_is_not_removed_as_stale() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("control.json");
        let server = ControlServer::bind_at_path(path.clone(), "site-id")
            .await
            .unwrap();
        let error = ControlServer::bind_at_path(path.clone(), "site-id")
            .await
            .err()
            .expect("a second service must be rejected");
        assert!(error.to_string().contains("already starting or running"));
        assert!(path.exists());
        drop(server);
        assert!(!path.exists());
    }
}
