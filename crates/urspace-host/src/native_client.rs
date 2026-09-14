//! Native Urspace client sessions for managed devices.

use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::{Endpoint, EndpointAddr, SecretKey, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use rand::Rng as _;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use urspace_protocol::{
    ALPN, ClientAuthV4, ClientProofV4, Header, INVITE_VERSION, RequestMethod,
    SESSION_PROOF_VERSION, ServerAuthV4, ServerHelloV4, SessionGrantPayload, SessionProofPayload,
    SessionProofPurpose, SiteRequest, SiteResponseHead, SocketMessage, read_frame,
    session_grant_hash, sign_session_proof, verify_invite_url, verify_session_grant, write_frame,
};
use zeroize::{Zeroize as _, Zeroizing};

const SESSION_FILE_VERSION: u8 = 1;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_REQUEST_BODY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSession {
    version: u8,
    session_grant: String,
    session_secret: String,
    local_origin_label: String,
    local_port: u16,
}

#[derive(Debug)]
pub struct NativeResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
}

pub struct NativeSocket {
    pub send: iroh::endpoint::SendStream,
    pub recv: iroh::endpoint::RecvStream,
}

pub struct NativeSiteClient {
    endpoint: Endpoint,
    endpoint_addr: EndpointAddr,
    connection: Mutex<iroh::endpoint::Connection>,
    session_grant: Mutex<String>,
    session_key: Zeroizing<[u8; 32]>,
    entry_path: String,
    site_id: String,
    local_origin_label: String,
    local_port: u16,
}

impl Drop for NativeSiteClient {
    fn drop(&mut self) {
        self.session_grant.get_mut().zeroize();
    }
}

impl NativeSiteClient {
    pub async fn enroll(
        invitation_url: &str,
        session_path: PathBuf,
        local_port: u16,
        now_unix: i64,
    ) -> Result<Self> {
        let mut invite = verify_invite_url(invitation_url, now_unix)
            .context("the enrollment invitation was rejected")?;
        if invite.version != INVITE_VERSION {
            bail!("native device enrollment requires a current Urspace invitation");
        }
        let endpoint_addr = ticket_addr(&invite.endpoint_ticket)?;
        let endpoint = bind_endpoint().await?;
        let session_key = SecretKey::generate();
        let authorization = ClientAuthV4::Admit {
            invite_id: invite.invite_id,
            capability: std::mem::take(&mut invite.capability),
            session_public_key: *session_key.public().as_bytes(),
        };
        let (connection, session_grant, grant) = authorize(
            &endpoint,
            &endpoint_addr,
            authorization,
            &session_key,
            None,
            now_unix,
        )
        .await?;
        let local_origin_label = random_local_origin_label();
        let session_key_bytes = Zeroizing::new(session_key.to_bytes());
        save_session(
            &session_path,
            &session_grant,
            &session_key_bytes,
            &local_origin_label,
            local_port,
        )?;
        Ok(Self::new(
            endpoint,
            endpoint_addr,
            connection,
            session_grant,
            session_key_bytes,
            grant,
            local_origin_label,
            local_port,
        ))
    }

    pub async fn resume(session_path: PathBuf, now_unix: i64) -> Result<Self> {
        let mut stored = load_session(&session_path)?;
        let secret = decode_secret(&stored.session_secret);
        stored.session_secret.zeroize();
        let secret = secret?;
        let session_key = SecretKey::from_bytes(&secret);
        let prior_grant = verify_session_grant(&stored.session_grant, now_unix)
            .context("saved device enrollment is invalid")?;
        if prior_grant.session_public_key != *session_key.public().as_bytes() {
            bail!("saved device key does not match its signed enrollment");
        }
        let endpoint_addr = ticket_addr(&prior_grant.endpoint_ticket)?;
        let endpoint = bind_endpoint().await?;
        let authorization = ClientAuthV4::Resume {
            session_grant: stored.session_grant.clone(),
        };
        let (connection, session_grant, grant) = authorize(
            &endpoint,
            &endpoint_addr,
            authorization,
            &session_key,
            Some((&stored.session_grant, &prior_grant)),
            now_unix,
        )
        .await?;
        stored.session_grant.zeroize();
        Ok(Self::new(
            endpoint,
            endpoint_addr,
            connection,
            session_grant,
            secret,
            grant,
            stored.local_origin_label,
            stored.local_port,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        endpoint: Endpoint,
        endpoint_addr: EndpointAddr,
        connection: iroh::endpoint::Connection,
        session_grant: String,
        session_key: Zeroizing<[u8; 32]>,
        grant: SessionGrantPayload,
        local_origin_label: String,
        local_port: u16,
    ) -> Self {
        Self {
            endpoint,
            endpoint_addr,
            connection: Mutex::new(connection),
            session_grant: Mutex::new(session_grant),
            session_key,
            entry_path: grant.entry_path,
            site_id: grant.site_id,
            local_origin_label,
            local_port,
        }
    }

    pub fn entry_path(&self) -> &str {
        &self.entry_path
    }

    pub fn site_id(&self) -> &str {
        &self.site_id
    }

    pub fn local_origin_label(&self) -> &str {
        &self.local_origin_label
    }

    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    pub async fn fetch(
        &self,
        method: RequestMethod,
        path: String,
        headers: Vec<Header>,
        body: Vec<u8>,
    ) -> Result<NativeResponse> {
        if body.len() > MAX_REQUEST_BODY_BYTES {
            bail!("request body exceeds the native client limit");
        }
        let (mut send, mut recv) = self.open_stream().await?;
        write_frame(
            &mut send,
            &SiteRequest {
                method,
                path,
                headers,
                body_length: body.len() as u64,
            },
        )
        .await
        .context("send private site request")?;
        tokio::io::AsyncWriteExt::write_all(&mut send, &body)
            .await
            .context("send private site request body")?;
        send.finish().context("finish private site request")?;
        let head: SiteResponseHead = read_frame(&mut recv)
            .await
            .context("read private site response")?;
        let body = recv
            .read_to_end(MAX_RESPONSE_BODY_BYTES)
            .await
            .context("read private site response body")?;
        Ok(NativeResponse {
            status: head.status,
            content_type: head.content_type,
            headers: head.headers,
            body,
        })
    }

    pub async fn open_socket(&self, path: String) -> Result<NativeSocket> {
        let (mut send, mut recv) = self.open_stream().await?;
        write_frame(
            &mut send,
            &SiteRequest {
                method: RequestMethod::WebSocket,
                path,
                headers: Vec::new(),
                body_length: 0,
            },
        )
        .await
        .context("send private WebSocket request")?;
        let head: SiteResponseHead = read_frame(&mut recv)
            .await
            .context("read private WebSocket response")?;
        if head.status != 101 {
            bail!(
                "private site rejected WebSocket with status {}",
                head.status
            );
        }
        Ok(NativeSocket { send, recv })
    }

    async fn open_stream(
        &self,
    ) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream)> {
        let connection = self.connection.lock().await.clone();
        if let Ok(stream) = connection.open_bi().await {
            return Ok(stream);
        }

        let mut current = self.connection.lock().await;
        if current.stable_id() != connection.stable_id()
            && let Ok(stream) = current.open_bi().await
        {
            return Ok(stream);
        }
        let mut session_grant = self.session_grant.lock().await;
        let prior = verify_session_grant(&session_grant, 0)
            .context("saved device enrollment is invalid")?;
        let session_key = SecretKey::from_bytes(&self.session_key);
        let (replacement, rotated_grant, _) = authorize(
            &self.endpoint,
            &self.endpoint_addr,
            ClientAuthV4::Resume {
                session_grant: session_grant.clone(),
            },
            &session_key,
            Some((&session_grant, &prior)),
            0,
        )
        .await?;
        session_grant.zeroize();
        *session_grant = rotated_grant;
        *current = replacement;
        current
            .open_bi()
            .await
            .context("open private site stream after reconnect")
    }
}

async fn bind_endpoint() -> Result<Endpoint> {
    #[cfg(not(test))]
    let endpoint = Endpoint::builder(presets::N0)
        .bind()
        .await
        .context("start native Iroh client")?;
    #[cfg(test)]
    let endpoint = Endpoint::builder(presets::Minimal)
        .bind()
        .await
        .context("start native Iroh client")?;
    #[cfg(not(test))]
    tokio::time::timeout(Duration::from_secs(30), endpoint.online())
        .await
        .context("timed out bringing the native Iroh client online")?;
    Ok(endpoint)
}

fn ticket_addr(raw: &str) -> Result<EndpointAddr> {
    Ok(EndpointTicket::from_str(raw)
        .context("parse private site route")?
        .endpoint_addr()
        .clone())
}

async fn authorize(
    endpoint: &Endpoint,
    endpoint_addr: &EndpointAddr,
    mut authorization: ClientAuthV4,
    session_key: &SecretKey,
    resume: Option<(&str, &SessionGrantPayload)>,
    now_unix: i64,
) -> Result<(iroh::endpoint::Connection, String, SessionGrantPayload)> {
    let connection = tokio::time::timeout(
        CONNECT_TIMEOUT,
        endpoint.connect(endpoint_addr.clone(), ALPN),
    )
    .await
    .context("timed out reaching the private site")?
    .context("reach the private site")?;
    let handshake = async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .context("open device authorization stream")?;
        let sent = write_frame(&mut send, &authorization).await;
        if let ClientAuthV4::Admit { capability, .. } = &mut authorization {
            capability.zeroize();
        }
        sent.context("send device authorization")?;
        let challenge = match read_frame(&mut recv)
            .await
            .context("read device authorization challenge")?
        {
            ServerAuthV4::Challenge(challenge) => challenge,
            ServerAuthV4::Denied { code } => bail!("site denied device authorization: {code:?}"),
        };
        let (purpose, grant_hash, expected_invite) = match resume {
            Some((encoded, grant)) => {
                if challenge.session_id != grant.session_id {
                    bail!("site returned a mismatched device session");
                }
                (
                    SessionProofPurpose::Resume,
                    session_grant_hash(encoded),
                    grant.invite_id,
                )
            }
            None => {
                let ClientAuthV4::Admit { invite_id, .. } = &authorization else {
                    bail!("invalid native authorization state");
                };
                (SessionProofPurpose::Admit, [0_u8; 32], *invite_id)
            }
        };
        let proof = SessionProofPayload {
            version: SESSION_PROOF_VERSION,
            purpose,
            host_id: *endpoint_addr.id.as_bytes(),
            site_id: endpoint_addr.id.to_z32(),
            alpn: ALPN.to_vec(),
            session_id: challenge.session_id,
            session_grant_hash: grant_hash,
            endpoint_id: *endpoint.id().as_bytes(),
            challenge_id: challenge.challenge_id,
            nonce: challenge.nonce,
            expires_at_unix: challenge.expires_at_unix,
        };
        write_frame(
            &mut send,
            &ClientProofV4 {
                signature: sign_session_proof(session_key, &proof)
                    .context("sign device authorization proof")?,
            },
        )
        .await
        .context("send device authorization proof")?;
        send.finish().context("finish device authorization")?;
        let session_grant = match read_frame(&mut recv)
            .await
            .context("read device authorization result")?
        {
            ServerHelloV4::Granted { session_grant } => session_grant,
            ServerHelloV4::Denied { code } => bail!("site denied device authorization: {code:?}"),
        };
        let grant = verify_session_grant(&session_grant, now_unix)
            .context("site returned an invalid device grant")?;
        if grant.host_id != *endpoint_addr.id.as_bytes()
            || grant.session_id != challenge.session_id
            || grant.invite_id != expected_invite
            || grant.session_public_key != *session_key.public().as_bytes()
        {
            bail!("site returned a mismatched device grant");
        }
        Ok((session_grant, grant))
    };
    let outcome = tokio::time::timeout(AUTHORIZATION_TIMEOUT, handshake).await;
    if let ClientAuthV4::Admit { capability, .. } = &mut authorization {
        capability.zeroize();
    }
    match outcome {
        Ok(Ok((session_grant, grant))) => Ok((connection, session_grant, grant)),
        Ok(Err(error)) => {
            connection.close(0_u8.into(), b"authorization failed");
            Err(error)
        }
        Err(_) => {
            connection.close(0_u8.into(), b"authorization timed out");
            bail!("site device authorization timed out")
        }
    }
}

fn random_local_origin_label() -> String {
    let bytes: [u8; 16] = rand::rng().random();
    blake3::hash(&bytes).to_hex()[..32].to_owned()
}

fn decode_secret(encoded: &str) -> Result<Zeroizing<[u8; 32]>> {
    let mut bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .context("decode saved device key")?;
    let key = bytes
        .as_slice()
        .try_into()
        .map(Zeroizing::new)
        .map_err(|_| anyhow::anyhow!("saved device key has invalid length"));
    bytes.zeroize();
    key
}

fn load_session(path: &Path) -> Result<StoredSession> {
    let mut bytes = std::fs::read(path)
        .with_context(|| format!("read saved Urspace device {}", path.display()))?;
    if bytes.len() > 32 * 1024 {
        bytes.zeroize();
        bail!("saved Urspace device is too large");
    }
    let parsed = serde_json::from_slice(&bytes).context("parse saved Urspace device");
    bytes.zeroize();
    let mut stored: StoredSession = parsed?;
    if stored.version != SESSION_FILE_VERSION
        || !stored.local_origin_label.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
        || stored.local_origin_label.len() != 32
        || stored.local_port == 0
    {
        stored.session_secret.zeroize();
        bail!("saved Urspace device is invalid");
    }
    Ok(stored)
}

fn save_session(
    path: &Path,
    session_grant: &str,
    session_key: &[u8; 32],
    local_origin_label: &str,
    local_port: u16,
) -> Result<()> {
    let mut stored = StoredSession {
        version: SESSION_FILE_VERSION,
        session_grant: session_grant.to_owned(),
        session_secret: URL_SAFE_NO_PAD.encode(session_key),
        local_origin_label: local_origin_label.to_owned(),
        local_port,
    };
    let encoded = serde_json::to_vec(&stored).context("encode saved Urspace device");
    stored.session_secret.zeroize();
    let mut encoded = encoded?;
    encoded.push(b'\n');
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create device directory {}", parent.display()))?;
    }
    if path.exists() {
        bail!("saved Urspace device already exists");
    }
    let temporary = path.with_extension(format!(
        "tmp-{}",
        URL_SAFE_NO_PAD.encode(rand::rng().random::<[u8; 8]>())
    ));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("create temporary device file {}", temporary.display()))?;
        std::io::Write::write_all(&mut file, &encoded)?;
        file.sync_all()?;
        std::fs::hard_link(&temporary, path)
            .with_context(|| format!("install saved Urspace device {}", path.display()))?;
        let _ = std::fs::remove_file(&temporary);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    })();
    encoded.zeroize();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub async fn send_socket_message(
    send: &mut iroh::endpoint::SendStream,
    message: &SocketMessage,
) -> Result<()> {
    write_frame(send, message)
        .await
        .context("send private WebSocket message")
}

pub async fn receive_socket_message(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<SocketMessage> {
    read_frame(recv)
        .await
        .context("receive private WebSocket message")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapabilityRegistry, SessionGrantIssuer, SiteProtocol, StaticSite, unix_now};
    use iroh::protocol::Router;
    use urspace_protocol::{InviteGrant, invite_url, sign_invite};
    use uuid::Uuid;

    #[test]
    fn device_state_is_private_non_overwriting_and_strictly_versioned() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("device.json");
        save_session(
            &path,
            "usg1.first",
            &[7_u8; 32],
            "0123456789abcdef0123456789abcdef",
            43210,
        )
        .unwrap();
        assert!(
            save_session(
                &path,
                "usg1.second",
                &[8_u8; 32],
                "0123456789abcdef0123456789abcdef",
                43210,
            )
            .is_err()
        );
        let stored = load_session(&path).unwrap();
        assert_eq!(stored.session_grant, "usg1.first");
        assert_eq!(*decode_secret(&stored.session_secret).unwrap(), [7_u8; 32]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let malformed = StoredSession {
            version: 2,
            session_grant: "usg1.invalid".into(),
            session_secret: URL_SAFE_NO_PAD.encode([0_u8; 32]),
            local_origin_label: "0123456789abcdef0123456789abcdef".into(),
            local_port: 43210,
        };
        std::fs::write(&path, serde_json::to_vec(&malformed).unwrap()).unwrap();
        assert!(load_session(&path).is_err());
    }

    #[tokio::test]
    async fn native_device_enrolls_fetches_and_resumes_without_the_bootstrap() {
        let directory = tempfile::tempdir().unwrap();
        let site_root = directory.path().join("site");
        tokio::fs::create_dir(&site_root).await.unwrap();
        tokio::fs::write(site_root.join("index.html"), "native mesh works")
            .await
            .unwrap();
        let session_path = directory.path().join("device.json");

        let identity = SecretKey::generate();
        let server = Endpoint::builder(presets::Minimal)
            .secret_key(identity.clone())
            .bind()
            .await
            .unwrap();
        let server_addr = server.addr();
        let endpoint_ticket = EndpointTicket::new(server_addr.clone()).to_string();
        let invite_id = Uuid::new_v4();
        let capability = [71_u8; 32];
        let registry = CapabilityRegistry::default();
        registry
            .insert(invite_id, &capability, unix_now() + 60, 1)
            .unwrap();
        let protocol = SiteProtocol::new(
            registry,
            StaticSite::open(&site_root).await.unwrap(),
            SessionGrantIssuer::new(
                identity.clone(),
                "https://sites.example".into(),
                endpoint_ticket.clone(),
                "/index.html".into(),
            ),
        );
        let router = Router::builder(server).accept(ALPN, protocol).spawn();
        let encoded = sign_invite(
            &identity,
            InviteGrant {
                bootstrap_origin: "https://sites.example".into(),
                endpoint_ticket,
                invite_id,
                capability,
                expires_at_unix: unix_now() + 60,
                entry_path: "/index.html".into(),
                max_sessions: 1,
            },
        )
        .unwrap();
        let invitation = invite_url(&encoded).unwrap();

        let client =
            NativeSiteClient::enroll(invitation.as_str(), session_path.clone(), 43210, unix_now())
                .await
                .unwrap();
        let response = client
            .fetch(
                RequestMethod::Get,
                "/index.html".into(),
                Vec::new(),
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"native mesh works");
        assert_eq!(client.entry_path(), "/index.html");
        let origin_label = client.local_origin_label().to_owned();
        assert_eq!(client.local_port(), 43210);
        drop(client);

        let resumed = NativeSiteClient::resume(session_path, unix_now())
            .await
            .unwrap();
        assert_eq!(resumed.local_origin_label(), origin_label);
        assert_eq!(resumed.local_port(), 43210);
        let response = resumed
            .fetch(
                RequestMethod::Get,
                "/index.html".into(),
                Vec::new(),
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"native mesh works");

        drop(resumed);
        router.shutdown().await.unwrap();
    }
}
