#![cfg_attr(not(target_family = "wasm"), allow(dead_code))]

use std::{
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, TransportAddr, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use urspace_protocol::{
    ClientAuthV4, ClientHello, ClientProofV4, Header, INVITE_VERSION, RequestMethod,
    SESSION_PROOF_VERSION, ServerAuthV4, ServerHello, ServerHelloV4, SessionGrantPayload,
    SessionProofPayload, SessionProofPurpose, SiteRequest, SiteResponseHead, SocketMessage,
    alpn_for_invite, read_frame, session_grant_hash, session_grant_origin, sign_session_proof,
    verify_invite_url, verify_invite_url_for_resume, verify_session_grant, write_frame,
};
use uuid::Uuid;
use wasm_bindgen::{JsError, JsValue, prelude::wasm_bindgen};
use zeroize::{Zeroize, Zeroizing};

const MAX_BOOTSTRAP_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const SITE_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const SITE_AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize)]
struct BrowserSiteResponse {
    status: u16,
    content_type: Option<String>,
    headers: Vec<Header>,
    body: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct BrowserHeader {
    name: String,
    value: String,
}

#[wasm_bindgen]
pub struct SiteClient {
    endpoint: Endpoint,
    endpoint_addr: EndpointAddr,
    connection: Mutex<iroh::endpoint::Connection>,
    authorization: SessionAuthorization,
    closed: AtomicBool,
    entry_path: String,
    site_id: String,
}

enum SessionAuthorization {
    Legacy {
        invite_id: Uuid,
        invite_version: u8,
        capability: Zeroizing<[u8; 32]>,
    },
    Grant {
        session_grant: String,
        session_key: Zeroizing<[u8; 32]>,
    },
}

#[wasm_bindgen]
impl SiteClient {
    #[wasm_bindgen(js_name = connect)]
    pub async fn connect(mut invitation_url: String, now_unix: f64) -> Result<Self, JsError> {
        if !now_unix.is_finite() || now_unix < 0.0 || now_unix > i64::MAX as f64 {
            invitation_url.zeroize();
            return Err(JsError::new("browser clock is outside the supported range"));
        }
        let now_unix = now_unix.floor() as i64;
        Self::establish_invite(invitation_url, now_unix).await
    }

    #[wasm_bindgen(js_name = resume)]
    pub async fn resume(
        mut resume_credential: String,
        mut session_secret: Vec<u8>,
        now_unix: f64,
        mut expected_origin: String,
    ) -> Result<Self, JsError> {
        if !now_unix.is_finite() || now_unix < 0.0 || now_unix > i64::MAX as f64 {
            resume_credential.zeroize();
            session_secret.zeroize();
            expected_origin.zeroize();
            return Err(JsError::new("browser clock is outside the supported range"));
        }
        let key_bytes: [u8; 32] = match session_secret.as_slice().try_into() {
            Ok(bytes) => bytes,
            Err(_) => {
                resume_credential.zeroize();
                session_secret.zeroize();
                expected_origin.zeroize();
                return Err(JsError::new("browser resume identity is invalid"));
            }
        };
        session_secret.zeroize();
        let key_bytes = Zeroizing::new(key_bytes);
        let now_unix = now_unix.floor() as i64;
        if resume_credential.starts_with("usg1.") {
            let result =
                Self::establish_grant(resume_credential, key_bytes, now_unix, &expected_origin)
                    .await;
            expected_origin.zeroize();
            result
        } else {
            expected_origin.zeroize();
            let endpoint_secret = iroh::SecretKey::from_bytes(&key_bytes);
            Self::establish_legacy_resume(resume_credential, endpoint_secret).await
        }
    }

    #[wasm_bindgen(js_name = exportResumeKey)]
    pub fn export_resume_key(&self) -> Vec<u8> {
        match &self.authorization {
            SessionAuthorization::Legacy { .. } => self.endpoint.secret_key().to_bytes().to_vec(),
            SessionAuthorization::Grant { session_key, .. } => session_key.to_vec(),
        }
    }

    #[wasm_bindgen(getter, js_name = resumeCredential)]
    pub fn resume_credential(&self) -> String {
        match &self.authorization {
            SessionAuthorization::Legacy { .. } => String::new(),
            SessionAuthorization::Grant { session_grant, .. } => session_grant.clone(),
        }
    }

    #[wasm_bindgen(getter, js_name = entryPath)]
    pub fn entry_path(&self) -> String {
        self.entry_path.clone()
    }

    #[wasm_bindgen(getter, js_name = siteId)]
    pub fn site_id(&self) -> String {
        self.site_id.clone()
    }

    pub async fn fetch(
        &self,
        method: String,
        path: String,
        headers: JsValue,
        body: Vec<u8>,
    ) -> Result<JsValue, JsError> {
        let method = parse_method(&method)?;
        let headers: Vec<BrowserHeader> = serde_wasm_bindgen::from_value(headers)
            .map_err(|error| JsError::new(&format!("invalid request headers: {error}")))?;
        let (mut send, mut recv) = self.open_stream("request").await?;
        write_frame(
            &mut send,
            &SiteRequest {
                method,
                path,
                headers: headers
                    .into_iter()
                    .map(|header| Header {
                        name: header.name,
                        value: header.value,
                    })
                    .collect(),
                body_length: body.len() as u64,
            },
        )
        .await
        .map_err(|error| JsError::new(&format!("request failed: {error}")))?;
        tokio::io::AsyncWriteExt::write_all(&mut send, &body)
            .await
            .map_err(|error| JsError::new(&format!("request body failed: {error}")))?;
        send.finish()
            .map_err(|error| JsError::new(&format!("request failed: {error}")))?;
        let head: SiteResponseHead = read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("response failed: {error}")))?;
        let body = recv
            .read_to_end(MAX_BOOTSTRAP_RESPONSE_BYTES)
            .await
            .map_err(|error| JsError::new(&format!("response body failed: {error}")))?;
        serde_wasm_bindgen::to_value(&BrowserSiteResponse {
            status: head.status,
            content_type: head.content_type,
            headers: head.headers,
            body,
        })
        .map_err(|error| JsError::new(&format!("response conversion failed: {error}")))
    }

    #[wasm_bindgen(js_name = openSocket)]
    pub async fn open_socket(&self, path: String) -> Result<SiteSocket, JsError> {
        let (mut send, mut recv) = self.open_stream("socket").await?;
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
        .map_err(|error| JsError::new(&format!("socket request failed: {error}")))?;
        let head: SiteResponseHead = read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("socket response failed: {error}")))?;
        if head.status != 101 {
            return Err(JsError::new(&format!(
                "site returned socket status {}",
                head.status
            )));
        }
        Ok(SiteSocket {
            send: Mutex::new(send),
            recv: Mutex::new(recv),
        })
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(connection) = self.connection.try_lock() {
            connection.close(0_u8.into(), b"browser closed");
        }
    }
}

impl SiteClient {
    async fn establish_invite(mut invitation_url: String, now_unix: i64) -> Result<Self, JsError> {
        let mut invite = match verify_invite_url(&invitation_url, now_unix) {
            Ok(invite) => invite,
            Err(error) => {
                invitation_url.zeroize();
                return Err(JsError::new(&format!("invitation rejected: {error}")));
            }
        };
        invitation_url.zeroize();

        if invite.version != INVITE_VERSION {
            return Self::establish_legacy(invite, iroh::SecretKey::generate()).await;
        }

        let capability = Zeroizing::new(std::mem::take(&mut invite.capability));
        let endpoint_addr = endpoint_addr_from_ticket(&invite.endpoint_ticket)?;
        let endpoint = bind_browser_endpoint(&endpoint_addr, iroh::SecretKey::generate()).await?;
        let session_secret = iroh::SecretKey::generate();
        let auth = ClientAuthV4::Admit {
            invite_id: invite.invite_id,
            capability: *capability,
            session_public_key: *session_secret.public().as_bytes(),
        };
        let authorized = connect_and_authorize_v4(
            &endpoint,
            &endpoint_addr,
            auth,
            &session_secret,
            None,
            now_unix,
        )
        .await;
        let (connection, session_grant, grant) = match authorized {
            Ok(authorized) => authorized,
            Err(error) => {
                endpoint.close().await;
                return Err(error);
            }
        };

        Ok(Self {
            endpoint,
            endpoint_addr,
            connection: Mutex::new(connection),
            authorization: SessionAuthorization::Grant {
                session_grant,
                session_key: Zeroizing::new(session_secret.to_bytes()),
            },
            closed: AtomicBool::new(false),
            entry_path: grant.entry_path,
            site_id: grant.site_id,
        })
    }

    async fn establish_legacy_resume(
        mut invitation_url: String,
        endpoint_secret: iroh::SecretKey,
    ) -> Result<Self, JsError> {
        let invite = match verify_invite_url_for_resume(&invitation_url) {
            Ok(invite) => invite,
            Err(error) => {
                invitation_url.zeroize();
                return Err(JsError::new(&format!("invitation rejected: {error}")));
            }
        };
        invitation_url.zeroize();
        if invite.version == INVITE_VERSION {
            return Err(JsError::new("a v4 session grant is required to reconnect"));
        }
        Self::establish_legacy(invite, endpoint_secret).await
    }

    async fn establish_legacy(
        mut invite: urspace_protocol::InvitePayload,
        endpoint_secret: iroh::SecretKey,
    ) -> Result<Self, JsError> {
        let capability = Zeroizing::new(std::mem::take(&mut invite.capability));
        let endpoint_addr = endpoint_addr_from_ticket(&invite.endpoint_ticket)?;
        let endpoint = bind_browser_endpoint(&endpoint_addr, endpoint_secret).await?;
        let connection = match connect_and_authorize_legacy(
            &endpoint,
            &endpoint_addr,
            invite.version,
            invite.invite_id,
            &capability,
        )
        .await
        {
            Ok(connection) => connection,
            Err(error) => {
                endpoint.close().await;
                return Err(error);
            }
        };
        Ok(Self {
            endpoint,
            endpoint_addr,
            connection: Mutex::new(connection),
            authorization: SessionAuthorization::Legacy {
                invite_id: invite.invite_id,
                invite_version: invite.version,
                capability,
            },
            closed: AtomicBool::new(false),
            entry_path: invite.entry_path,
            site_id: invite.site_id,
        })
    }

    async fn establish_grant(
        session_grant: String,
        session_key: Zeroizing<[u8; 32]>,
        now_unix: i64,
        expected_origin: &str,
    ) -> Result<Self, JsError> {
        let grant = verify_session_grant(&session_grant, now_unix)
            .map_err(|error| JsError::new(&format!("session grant rejected: {error}")))?;
        if session_grant_origin(&grant)
            .map_err(|error| JsError::new(&format!("session grant rejected: {error}")))?
            != expected_origin
        {
            return Err(JsError::new("session grant belongs to another site origin"));
        }
        let session_secret = iroh::SecretKey::from_bytes(&session_key);
        if grant.session_public_key != *session_secret.public().as_bytes() {
            return Err(JsError::new("browser resume identity is invalid"));
        }
        let endpoint_addr = endpoint_addr_from_ticket(&grant.endpoint_ticket)?;
        let endpoint = bind_browser_endpoint(&endpoint_addr, iroh::SecretKey::generate()).await?;
        let authorized = connect_and_authorize_v4(
            &endpoint,
            &endpoint_addr,
            ClientAuthV4::Resume {
                session_grant: session_grant.clone(),
            },
            &session_secret,
            Some((&session_grant, &grant)),
            now_unix,
        )
        .await;
        let (connection, rotated_grant, rotated) = match authorized {
            Ok(authorized) => authorized,
            Err(error) => {
                endpoint.close().await;
                return Err(error);
            }
        };
        Ok(Self {
            endpoint,
            endpoint_addr,
            connection: Mutex::new(connection),
            authorization: SessionAuthorization::Grant {
                session_grant: rotated_grant,
                session_key,
            },
            closed: AtomicBool::new(false),
            entry_path: rotated.entry_path,
            site_id: rotated.site_id,
        })
    }

    async fn open_stream(
        &self,
        kind: &str,
    ) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream), JsError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(JsError::new("browser session is closed"));
        }
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
        let replacement = match &self.authorization {
            SessionAuthorization::Legacy {
                invite_id,
                invite_version,
                capability,
            } => {
                connect_and_authorize_legacy(
                    &self.endpoint,
                    &self.endpoint_addr,
                    *invite_version,
                    *invite_id,
                    capability,
                )
                .await?
            }
            SessionAuthorization::Grant {
                session_grant,
                session_key,
            } => {
                let grant = verify_session_grant(session_grant, 0)
                    .map_err(|error| JsError::new(&format!("session grant rejected: {error}")))?;
                let session_secret = iroh::SecretKey::from_bytes(session_key);
                let (connection, _, _) = connect_and_authorize_v4(
                    &self.endpoint,
                    &self.endpoint_addr,
                    ClientAuthV4::Resume {
                        session_grant: session_grant.clone(),
                    },
                    &session_secret,
                    Some((session_grant, &grant)),
                    0,
                )
                .await?;
                connection
            }
        };
        if self.closed.load(Ordering::Acquire) {
            replacement.close(0_u8.into(), b"browser closed");
            return Err(JsError::new("browser session is closed"));
        }
        *current = replacement;
        current
            .open_bi()
            .await
            .map_err(|error| JsError::new(&format!("{kind} stream failed: {error}")))
    }
}

fn endpoint_addr_from_ticket(raw: &str) -> Result<EndpointAddr, JsError> {
    let ticket = EndpointTicket::from_str(raw)
        .map_err(|error| JsError::new(&format!("invalid endpoint ticket: {error}")))?;
    normalize_relay_hosts(ticket.endpoint_addr())
        .map_err(|error| JsError::new(&format!("invalid relay address: {error}")))
}

async fn bind_browser_endpoint(
    endpoint_addr: &EndpointAddr,
    endpoint_secret: iroh::SecretKey,
) -> Result<Endpoint, JsError> {
    let relay_urls: Vec<_> = endpoint_addr.relay_urls().cloned().collect();
    if relay_urls.is_empty() {
        return Err(JsError::new("invitation does not contain a browser relay"));
    }
    Endpoint::builder(presets::N0)
        .secret_key(endpoint_secret)
        .relay_mode(RelayMode::custom(relay_urls))
        .bind()
        .await
        .map_err(|error| JsError::new(&format!("could not start Iroh: {error}")))
}

async fn connect_transport(
    endpoint: &Endpoint,
    endpoint_addr: &EndpointAddr,
    alpn: &'static [u8],
) -> Result<iroh::endpoint::Connection, JsError> {
    match n0_future::time::timeout(
        SITE_CONNECT_TIMEOUT,
        endpoint.connect(endpoint_addr.clone(), alpn),
    )
    .await
    {
        Ok(Ok(connection)) => Ok(connection),
        Ok(Err(error)) => Err(JsError::new(&format!("could not reach site: {error}"))),
        Err(_) => Err(JsError::new(
            "Iroh relay connection timed out before the site could be reached",
        )),
    }
}

async fn connect_and_authorize_v4(
    endpoint: &Endpoint,
    endpoint_addr: &EndpointAddr,
    auth: ClientAuthV4,
    session_secret: &iroh::SecretKey,
    resume: Option<(&str, &SessionGrantPayload)>,
    now_unix: i64,
) -> Result<(iroh::endpoint::Connection, String, SessionGrantPayload), JsError> {
    let connection = connect_transport(endpoint, endpoint_addr, urspace_protocol::ALPN).await?;
    let authorize = async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|error| JsError::new(&format!("authorization stream failed: {error}")))?;
        write_frame(&mut send, &auth)
            .await
            .map_err(|error| JsError::new(&format!("authorization request failed: {error}")))?;
        let challenge = match read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("authorization challenge failed: {error}")))?
        {
            ServerAuthV4::Challenge(challenge) => challenge,
            ServerAuthV4::Denied { code } => {
                return Err(JsError::new(&format!(
                    "site denied authorization: {code:?}"
                )));
            }
        };
        let (purpose, grant_hash, expected_invite) = match resume {
            Some((session_grant, grant)) => {
                if challenge.session_id != grant.session_id {
                    return Err(JsError::new("site returned the wrong browser session"));
                }
                (
                    SessionProofPurpose::Resume,
                    session_grant_hash(session_grant),
                    grant.invite_id,
                )
            }
            None => {
                let ClientAuthV4::Admit { invite_id, .. } = &auth else {
                    return Err(JsError::new("browser authorization state is invalid"));
                };
                (SessionProofPurpose::Admit, [0_u8; 32], *invite_id)
            }
        };
        let proof = SessionProofPayload {
            version: SESSION_PROOF_VERSION,
            purpose,
            host_id: *endpoint_addr.id.as_bytes(),
            site_id: endpoint_addr.id.to_z32(),
            alpn: urspace_protocol::ALPN.to_vec(),
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
                signature: sign_session_proof(session_secret, &proof).map_err(|error| {
                    JsError::new(&format!("could not sign session proof: {error}"))
                })?,
            },
        )
        .await
        .map_err(|error| JsError::new(&format!("authorization proof failed: {error}")))?;
        send.finish()
            .map_err(|error| JsError::new(&format!("authorization request failed: {error}")))?;
        let session_grant = match read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("authorization response failed: {error}")))?
        {
            ServerHelloV4::Granted { session_grant } => session_grant,
            ServerHelloV4::Denied { code } => {
                return Err(JsError::new(&format!(
                    "site denied authorization: {code:?}"
                )));
            }
        };
        let grant = verify_session_grant(&session_grant, now_unix)
            .map_err(|error| JsError::new(&format!("site returned an invalid grant: {error}")))?;
        if grant.host_id != *endpoint_addr.id.as_bytes()
            || grant.session_id != challenge.session_id
            || grant.invite_id != expected_invite
            || grant.session_public_key != *session_secret.public().as_bytes()
        {
            return Err(JsError::new("site returned a mismatched session grant"));
        }
        Ok((session_grant, grant))
    };
    let (session_grant, grant) =
        match n0_future::time::timeout(SITE_AUTHORIZATION_TIMEOUT, authorize).await {
            Ok(Ok(authorized)) => authorized,
            Ok(Err(error)) => {
                connection.close(0_u8.into(), b"authorization failed");
                return Err(error);
            }
            Err(_) => {
                connection.close(0_u8.into(), b"authorization timed out");
                return Err(JsError::new("site authorization timed out"));
            }
        };
    Ok((connection, session_grant, grant))
}

async fn connect_and_authorize_legacy(
    endpoint: &Endpoint,
    endpoint_addr: &EndpointAddr,
    invite_version: u8,
    invite_id: Uuid,
    capability: &[u8; 32],
) -> Result<iroh::endpoint::Connection, JsError> {
    let alpn = alpn_for_invite(invite_version)
        .map_err(|error| JsError::new(&format!("unsupported invitation: {error}")))?;
    let connection = match n0_future::time::timeout(
        SITE_CONNECT_TIMEOUT,
        endpoint.connect(endpoint_addr.clone(), alpn),
    )
    .await
    {
        Ok(Ok(connection)) => connection,
        Ok(Err(error)) => return Err(JsError::new(&format!("could not reach site: {error}"))),
        Err(_) => {
            return Err(JsError::new(
                "Iroh relay connection timed out before the site could be reached",
            ));
        }
    };

    let authorize = async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|error| JsError::new(&format!("authorization stream failed: {error}")))?;
        write_frame(
            &mut send,
            &ClientHello {
                version: invite_version,
                invite_id,
                capability: *capability,
            },
        )
        .await
        .map_err(|error| JsError::new(&format!("authorization request failed: {error}")))?;
        send.finish()
            .map_err(|error| JsError::new(&format!("authorization request failed: {error}")))?;
        read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("authorization response failed: {error}")))
    };
    let reply: ServerHello =
        match n0_future::time::timeout(SITE_AUTHORIZATION_TIMEOUT, authorize).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(error)) => {
                connection.close(0_u8.into(), b"authorization failed");
                return Err(error);
            }
            Err(_) => {
                connection.close(0_u8.into(), b"authorization timed out");
                return Err(JsError::new("site authorization timed out"));
            }
        };
    match reply {
        ServerHello::Granted { .. } => Ok(connection),
        ServerHello::Denied { code } => {
            connection.close(0_u8.into(), b"authorization denied");
            Err(JsError::new(&format!("site denied invitation: {code:?}")))
        }
    }
}

fn normalize_relay_hosts(endpoint_addr: &EndpointAddr) -> Result<EndpointAddr, String> {
    let addrs = endpoint_addr
        .addrs
        .iter()
        .map(|addr| match addr {
            TransportAddr::Relay(relay_url) => {
                normalize_relay_host(relay_url).map(TransportAddr::Relay)
            }
            addr => Ok(addr.clone()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EndpointAddr::from_parts(endpoint_addr.id, addrs))
}

fn normalize_relay_host(relay_url: &RelayUrl) -> Result<RelayUrl, String> {
    let mut url: url::Url = relay_url.clone().into();
    let Some(host) = url.host_str().map(ToOwned::to_owned) else {
        return Ok(relay_url.clone());
    };
    let normalized = host.trim_end_matches('.');
    if normalized == host {
        return Ok(relay_url.clone());
    }
    if normalized.is_empty() {
        return Err("relay hostname is empty after normalization".to_string());
    }
    url.set_host(Some(normalized))
        .map_err(|_| "relay hostname could not be normalized".to_string())?;
    Ok(RelayUrl::from(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_terminal_dot_from_relay_hosts_for_webkit() {
        let relay: RelayUrl = "https://usw1-1.relay.n0.iroh.link./".parse().unwrap();
        let endpoint = EndpointAddr::new(iroh::SecretKey::from_bytes(&[7_u8; 32]).public())
            .with_relay_url(relay);

        let normalized = normalize_relay_hosts(&endpoint).unwrap();

        assert_eq!(
            normalized.relay_urls().next().unwrap().to_string(),
            "https://usw1-1.relay.n0.iroh.link/"
        );
    }

    #[test]
    fn leaves_already_normalized_relay_hosts_unchanged() {
        let relay: RelayUrl = "https://relay.example.com/".parse().unwrap();
        assert_eq!(normalize_relay_host(&relay).unwrap(), relay);
    }
}

#[wasm_bindgen]
pub struct SiteSocket {
    send: Mutex<iroh::endpoint::SendStream>,
    recv: Mutex<iroh::endpoint::RecvStream>,
}

#[wasm_bindgen]
impl SiteSocket {
    #[wasm_bindgen(js_name = sendText)]
    pub async fn send_text(&self, text: String) -> Result<(), JsError> {
        self.send_message(SocketMessage::Text(text)).await
    }

    #[wasm_bindgen(js_name = sendBinary)]
    pub async fn send_binary(&self, bytes: Vec<u8>) -> Result<(), JsError> {
        self.send_message(SocketMessage::Binary(bytes)).await
    }

    pub async fn receive(&self) -> Result<JsValue, JsError> {
        let message: SocketMessage = read_frame(&mut *self.recv.lock().await)
            .await
            .map_err(|error| JsError::new(&format!("socket receive failed: {error}")))?;
        serde_wasm_bindgen::to_value(&message)
            .map_err(|error| JsError::new(&format!("socket conversion failed: {error}")))
    }

    pub async fn close(&self, code: Option<u16>, reason: String) -> Result<(), JsError> {
        self.send_message(SocketMessage::Close { code, reason })
            .await
    }
}

impl SiteSocket {
    async fn send_message(&self, message: SocketMessage) -> Result<(), JsError> {
        write_frame(&mut *self.send.lock().await, &message)
            .await
            .map_err(|error| JsError::new(&format!("socket send failed: {error}")))
    }
}

fn parse_method(method: &str) -> Result<RequestMethod, JsError> {
    match method {
        "GET" => Ok(RequestMethod::Get),
        "HEAD" => Ok(RequestMethod::Head),
        "POST" => Ok(RequestMethod::Post),
        "PUT" => Ok(RequestMethod::Put),
        "PATCH" => Ok(RequestMethod::Patch),
        "DELETE" => Ok(RequestMethod::Delete),
        "OPTIONS" => Ok(RequestMethod::Options),
        _ => Err(JsError::new("unsupported request method")),
    }
}
