//! Versioned wire types and self-authenticating invitation URLs for Urspace.

use std::str::FromStr;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh_base::{PublicKey, SecretKey, Signature};
use iroh_tickets::endpoint::EndpointTicket;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

pub const ALPN: &[u8] = b"urspace-site/4";
pub const TUNNEL_ALPN: &[u8] = b"urspace-tunnel/1";
pub const INVITE_VERSION: u8 = 4;
pub const TUNNEL_VERSION: u8 = 1;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const BOOTSTRAP_PATH: &str = "/.urspace/open/";

pub const LEGACY_V3_ALPN: &[u8] = b"urspace-site/3";
pub const LEGACY_V3_INVITE_VERSION: u8 = 3;

// Kept only so v0.1 invitation URLs continue to open during the v0.2 migration.
pub const LEGACY_V2_ALPN: &[u8] = b"medousa-site/2";
pub const LEGACY_V2_INVITE_VERSION: u8 = 2;
pub const LEGACY_V2_BOOTSTRAP_PATH: &str = "/.medousa/open/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvitePayload {
    pub version: u8,
    pub bootstrap_origin: String,
    pub endpoint_ticket: String,
    pub site_id: String,
    pub invite_id: Uuid,
    pub capability: [u8; 32],
    pub expires_at_unix: i64,
    pub entry_path: String,
    pub max_sessions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteGrant {
    pub bootstrap_origin: String,
    pub endpoint_ticket: String,
    pub invite_id: Uuid,
    pub capability: [u8; 32],
    pub expires_at_unix: i64,
    pub entry_path: String,
    pub max_sessions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SignedInvite {
    payload: InvitePayload,
    signer: [u8; 32],
    signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientHello {
    pub version: u8,
    pub invite_id: Uuid,
    pub capability: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerHello {
    Granted { expires_at_unix: i64 },
    Denied { code: DenialCode },
}

pub const SESSION_GRANT_VERSION: u8 = 1;
pub const SESSION_PROOF_VERSION: u8 = 1;
const SESSION_GRANT_PREFIX: &str = "usg1.";
const SESSION_GRANT_DOMAIN: &[u8] = b"urspace-session-grant-v1\0";
const SESSION_PROOF_DOMAIN: &[u8] = b"urspace-session-proof-v1\0";
const MAX_SESSION_GRANT_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGrantPayload {
    pub version: u8,
    pub host_id: [u8; 32],
    pub site_id: String,
    pub bootstrap_origin: String,
    pub endpoint_ticket: String,
    pub invite_id: Uuid,
    pub session_id: Uuid,
    pub session_public_key: [u8; 32],
    pub issued_at_unix: i64,
    pub expires_at_unix: i64,
    pub authorization_epoch: u64,
    pub entry_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionGrantIssue {
    pub bootstrap_origin: String,
    pub endpoint_ticket: String,
    pub invite_id: Uuid,
    pub session_id: Uuid,
    pub session_public_key: [u8; 32],
    pub issued_at_unix: i64,
    pub expires_at_unix: i64,
    pub authorization_epoch: u64,
    pub entry_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SignedSessionGrant {
    payload: SessionGrantPayload,
    signer: [u8; 32],
    signature: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionProofPurpose {
    Admit,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionProofPayload {
    pub version: u8,
    pub purpose: SessionProofPurpose,
    pub host_id: [u8; 32],
    pub site_id: String,
    pub alpn: Vec<u8>,
    pub session_id: Uuid,
    pub session_grant_hash: [u8; 32],
    pub endpoint_id: [u8; 32],
    pub challenge_id: Uuid,
    pub nonce: [u8; 32],
    pub expires_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientAuthV4 {
    Admit {
        invite_id: Uuid,
        capability: [u8; 32],
        session_public_key: [u8; 32],
    },
    Resume {
        session_grant: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionChallengeV4 {
    pub challenge_id: Uuid,
    pub session_id: Uuid,
    pub nonce: [u8; 32],
    pub expires_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerAuthV4 {
    Challenge(SessionChallengeV4),
    Denied { code: DenialCode },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientProofV4 {
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerHelloV4 {
    Granted { session_grant: String },
    Denied { code: DenialCode },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelOpenV1 {
    pub version: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TunnelStatusV1 {
    Ready,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DenialCode {
    Invalid,
    Expired,
    Revoked,
    SessionLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteRequest {
    pub method: RequestMethod,
    pub path: String,
    pub headers: Vec<Header>,
    pub body_length: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    WebSocket,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteResponseHead {
    pub status: u16,
    pub content_type: Option<String>,
    pub content_length: u64,
    pub headers: Vec<Header>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SocketMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close { code: Option<u16>, reason: String },
}

#[derive(Debug, Error)]
pub enum InviteError {
    #[error("invalid bootstrap URL")]
    InvalidBootstrapUrl,
    #[error("invitation fragment is missing")]
    MissingFragment,
    #[error("unsupported invitation version")]
    UnsupportedVersion,
    #[error("invitation encoding is invalid")]
    InvalidEncoding,
    #[error("invitation signature is invalid")]
    InvalidSignature,
    #[error("site identity does not match invitation signer")]
    SiteIdentityMismatch,
    #[error("URL origin does not match the invitation site identity")]
    OriginMismatch,
    #[error("endpoint ticket does not match invitation signer")]
    EndpointMismatch,
    #[error("invitation has expired")]
    Expired,
    #[error("entry path must be an absolute site path")]
    InvalidEntryPath,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionGrantError {
    #[error("session grant encoding is invalid")]
    InvalidEncoding,
    #[error("session grant version is unsupported")]
    UnsupportedVersion,
    #[error("session grant signature is invalid")]
    InvalidSignature,
    #[error("session grant signer does not match its host")]
    HostIdentityMismatch,
    #[error("session grant endpoint does not match its host")]
    EndpointMismatch,
    #[error("session grant site origin is invalid")]
    InvalidOrigin,
    #[error("session grant entry path is invalid")]
    InvalidEntryPath,
    #[error("session grant lifetime is invalid")]
    InvalidLifetime,
    #[error("session grant has expired")]
    Expired,
    #[error("session proof is invalid")]
    InvalidProof,
}

pub fn sign_session_grant(
    identity: &SecretKey,
    issue: SessionGrantIssue,
) -> Result<String, SessionGrantError> {
    if issue.issued_at_unix < 0 || issue.expires_at_unix <= issue.issued_at_unix {
        return Err(SessionGrantError::InvalidLifetime);
    }
    validate_entry_path(&issue.entry_path).map_err(|_| SessionGrantError::InvalidEntryPath)?;
    let bootstrap_origin = normalize_bootstrap_origin(&issue.bootstrap_origin)
        .map_err(|_| SessionGrantError::InvalidOrigin)?;
    let payload = SessionGrantPayload {
        version: SESSION_GRANT_VERSION,
        host_id: *identity.public().as_bytes(),
        site_id: identity.public().to_z32(),
        bootstrap_origin,
        endpoint_ticket: issue.endpoint_ticket,
        invite_id: issue.invite_id,
        session_id: issue.session_id,
        session_public_key: issue.session_public_key,
        issued_at_unix: issue.issued_at_unix,
        expires_at_unix: issue.expires_at_unix,
        authorization_epoch: issue.authorization_epoch,
        entry_path: issue.entry_path,
    };
    validate_session_grant_payload(&payload, None)?;
    let payload_bytes =
        postcard::to_allocvec(&payload).map_err(|_| SessionGrantError::InvalidEncoding)?;
    let signed = SignedSessionGrant {
        payload,
        signer: *identity.public().as_bytes(),
        signature: identity
            .sign(&domain_message(SESSION_GRANT_DOMAIN, &payload_bytes))
            .to_bytes()
            .to_vec(),
    };
    let encoded = postcard::to_allocvec(&signed).map_err(|_| SessionGrantError::InvalidEncoding)?;
    if encoded.len() > MAX_SESSION_GRANT_BYTES {
        return Err(SessionGrantError::InvalidEncoding);
    }
    Ok(format!(
        "{SESSION_GRANT_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(encoded)
    ))
}

pub fn verify_session_grant(
    encoded: &str,
    now_unix: i64,
) -> Result<SessionGrantPayload, SessionGrantError> {
    let value = encoded
        .strip_prefix(SESSION_GRANT_PREFIX)
        .ok_or(SessionGrantError::InvalidEncoding)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| SessionGrantError::InvalidEncoding)?;
    if bytes.is_empty() || bytes.len() > MAX_SESSION_GRANT_BYTES {
        return Err(SessionGrantError::InvalidEncoding);
    }
    let signed: SignedSessionGrant =
        postcard::from_bytes(&bytes).map_err(|_| SessionGrantError::InvalidEncoding)?;
    let canonical =
        postcard::to_allocvec(&signed).map_err(|_| SessionGrantError::InvalidEncoding)?;
    if canonical != bytes {
        return Err(SessionGrantError::InvalidEncoding);
    }
    validate_session_grant_payload(&signed.payload, Some(now_unix))?;
    if signed.signer != signed.payload.host_id {
        return Err(SessionGrantError::HostIdentityMismatch);
    }
    let public =
        PublicKey::from_bytes(&signed.signer).map_err(|_| SessionGrantError::InvalidSignature)?;
    let signature_bytes: [u8; Signature::LENGTH] = signed
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| SessionGrantError::InvalidSignature)?;
    let signature = Signature::from_bytes(&signature_bytes);
    let payload_bytes =
        postcard::to_allocvec(&signed.payload).map_err(|_| SessionGrantError::InvalidEncoding)?;
    public
        .verify(
            &domain_message(SESSION_GRANT_DOMAIN, &payload_bytes),
            &signature,
        )
        .map_err(|_| SessionGrantError::InvalidSignature)?;
    Ok(signed.payload)
}

pub fn session_grant_hash(encoded: &str) -> [u8; 32] {
    *blake3::hash(encoded.as_bytes()).as_bytes()
}

pub fn session_grant_origin(payload: &SessionGrantPayload) -> Result<String, SessionGrantError> {
    validate_session_grant_payload(payload, None)?;
    let mut url =
        Url::parse(&payload.bootstrap_origin).map_err(|_| SessionGrantError::InvalidOrigin)?;
    let host = format!(
        "{}.{}",
        payload.site_id,
        url.host_str().ok_or(SessionGrantError::InvalidOrigin)?
    );
    url.set_host(Some(&host))
        .map_err(|_| SessionGrantError::InvalidOrigin)?;
    Ok(url.origin().ascii_serialization())
}

pub fn sign_session_proof(
    session_key: &SecretKey,
    proof: &SessionProofPayload,
) -> Result<Vec<u8>, SessionGrantError> {
    validate_session_proof_payload(proof, None)?;
    let bytes = postcard::to_allocvec(proof).map_err(|_| SessionGrantError::InvalidProof)?;
    Ok(session_key
        .sign(&domain_message(SESSION_PROOF_DOMAIN, &bytes))
        .to_bytes()
        .to_vec())
}

pub fn verify_session_proof(
    session_public_key: &[u8; 32],
    proof: &SessionProofPayload,
    signature: &[u8],
    now_unix: i64,
) -> Result<(), SessionGrantError> {
    validate_session_proof_payload(proof, Some(now_unix))?;
    let public =
        PublicKey::from_bytes(session_public_key).map_err(|_| SessionGrantError::InvalidProof)?;
    let signature_bytes: [u8; Signature::LENGTH] = signature
        .try_into()
        .map_err(|_| SessionGrantError::InvalidProof)?;
    let signature = Signature::from_bytes(&signature_bytes);
    let bytes = postcard::to_allocvec(proof).map_err(|_| SessionGrantError::InvalidProof)?;
    public
        .verify(&domain_message(SESSION_PROOF_DOMAIN, &bytes), &signature)
        .map_err(|_| SessionGrantError::InvalidProof)
}

fn validate_session_grant_payload(
    payload: &SessionGrantPayload,
    now_unix: Option<i64>,
) -> Result<(), SessionGrantError> {
    if payload.version != SESSION_GRANT_VERSION {
        return Err(SessionGrantError::UnsupportedVersion);
    }
    if payload.issued_at_unix < 0 || payload.expires_at_unix <= payload.issued_at_unix {
        return Err(SessionGrantError::InvalidLifetime);
    }
    if now_unix.is_some_and(|now| payload.expires_at_unix <= now) {
        return Err(SessionGrantError::Expired);
    }
    validate_entry_path(&payload.entry_path).map_err(|_| SessionGrantError::InvalidEntryPath)?;
    if normalize_bootstrap_origin(&payload.bootstrap_origin)
        .map_err(|_| SessionGrantError::InvalidOrigin)?
        != payload.bootstrap_origin
    {
        return Err(SessionGrantError::InvalidOrigin);
    }
    let public =
        PublicKey::from_bytes(&payload.host_id).map_err(|_| SessionGrantError::InvalidSignature)?;
    if public.to_z32() != payload.site_id {
        return Err(SessionGrantError::HostIdentityMismatch);
    }
    let ticket = EndpointTicket::from_str(&payload.endpoint_ticket)
        .map_err(|_| SessionGrantError::EndpointMismatch)?;
    if ticket.endpoint_addr().id != public {
        return Err(SessionGrantError::EndpointMismatch);
    }
    PublicKey::from_bytes(&payload.session_public_key)
        .map_err(|_| SessionGrantError::InvalidProof)?;
    Ok(())
}

fn validate_session_proof_payload(
    proof: &SessionProofPayload,
    now_unix: Option<i64>,
) -> Result<(), SessionGrantError> {
    if proof.version != SESSION_PROOF_VERSION
        || !matches!(proof.alpn.as_slice(), ALPN | TUNNEL_ALPN)
    {
        return Err(SessionGrantError::InvalidProof);
    }
    if proof.expires_at_unix < 0 || now_unix.is_some_and(|now| proof.expires_at_unix <= now) {
        return Err(SessionGrantError::InvalidProof);
    }
    let host =
        PublicKey::from_bytes(&proof.host_id).map_err(|_| SessionGrantError::InvalidProof)?;
    if host.to_z32() != proof.site_id {
        return Err(SessionGrantError::InvalidProof);
    }
    PublicKey::from_bytes(&proof.endpoint_id).map_err(|_| SessionGrantError::InvalidProof)?;
    Ok(())
}

fn domain_message(domain: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(domain.len() + payload.len());
    message.extend_from_slice(domain);
    message.extend_from_slice(payload);
    message
}

pub fn sign_invite(identity: &SecretKey, grant: InviteGrant) -> Result<String, InviteError> {
    sign_invite_version(identity, grant, INVITE_VERSION)
}

fn sign_invite_version(
    identity: &SecretKey,
    grant: InviteGrant,
    version: u8,
) -> Result<String, InviteError> {
    wire_profile(version)?;
    validate_entry_path(&grant.entry_path)?;
    let bootstrap_origin = normalize_bootstrap_origin(&grant.bootstrap_origin)?;
    let payload = InvitePayload {
        version,
        bootstrap_origin,
        endpoint_ticket: grant.endpoint_ticket,
        site_id: identity.public().to_z32(),
        invite_id: grant.invite_id,
        capability: grant.capability,
        expires_at_unix: grant.expires_at_unix,
        entry_path: grant.entry_path,
        max_sessions: grant.max_sessions,
    };
    let bytes = postcard::to_allocvec(&payload).map_err(|_| InviteError::InvalidEncoding)?;
    let signed = SignedInvite {
        payload,
        signer: *identity.public().as_bytes(),
        signature: identity.sign(&bytes).to_bytes().to_vec(),
    };
    let encoded = postcard::to_allocvec(&signed).map_err(|_| InviteError::InvalidEncoding)?;
    Ok(URL_SAFE_NO_PAD.encode(encoded))
}

pub fn invite_url(encoded: &str) -> Result<Url, InviteError> {
    let signed = decode_signed(encoded)?;
    let wire = wire_profile(signed.payload.version)?;
    let mut url = Url::parse(&signed.payload.bootstrap_origin)
        .map_err(|_| InviteError::InvalidBootstrapUrl)?;
    let host = format!(
        "{}.{}",
        signed.payload.site_id,
        url.host_str().expect("checked host")
    );
    url.set_host(Some(&host))
        .map_err(|_| InviteError::InvalidBootstrapUrl)?;
    url.set_path(wire.bootstrap_path);
    url.set_query(None);
    url.set_fragment(Some(&format!("{}={encoded}", wire.fragment_prefix)));
    Ok(url)
}

pub fn verify_invite_url(raw: &str, now_unix: i64) -> Result<InvitePayload, InviteError> {
    verify_invite_url_inner(raw, Some(now_unix))
}

/// Verifies a legacy v2/v3 invitation without enforcing its admission expiry.
///
/// The host remains the authorization boundary: it accepts this invitation only when the
/// connecting endpoint identity was admitted before expiry and has not been kicked or revoked.
pub fn verify_invite_url_for_resume(raw: &str) -> Result<InvitePayload, InviteError> {
    verify_invite_url_inner(raw, None)
}

fn verify_invite_url_inner(raw: &str, now_unix: Option<i64>) -> Result<InvitePayload, InviteError> {
    let url = Url::parse(raw).map_err(|_| InviteError::InvalidBootstrapUrl)?;
    let fragment = url.fragment().ok_or(InviteError::MissingFragment)?;
    let (fragment_version, encoded) = if let Some(encoded) = fragment.strip_prefix("u4=") {
        (INVITE_VERSION, encoded)
    } else if let Some(encoded) = fragment.strip_prefix("u3=") {
        (LEGACY_V3_INVITE_VERSION, encoded)
    } else if let Some(encoded) = fragment.strip_prefix("m2=") {
        (LEGACY_V2_INVITE_VERSION, encoded)
    } else {
        return Err(InviteError::MissingFragment);
    };
    let signed = decode_signed(encoded)?;
    if signed.payload.version != fragment_version {
        return Err(InviteError::UnsupportedVersion);
    }
    verify_signed(&signed, now_unix)?;

    let expected = invite_url(encoded)?;
    if url.scheme() != expected.scheme()
        || url.host_str() != expected.host_str()
        || url.port_or_known_default() != expected.port_or_known_default()
        || url.path() != expected.path()
        || url.query().is_some()
    {
        return Err(InviteError::OriginMismatch);
    }
    Ok(signed.payload)
}

pub fn verify_encoded_invite(encoded: &str, now_unix: i64) -> Result<InvitePayload, InviteError> {
    let signed = decode_signed(encoded)?;
    verify_signed(&signed, Some(now_unix))?;
    Ok(signed.payload)
}

fn decode_signed(encoded: &str) -> Result<SignedInvite, InviteError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| InviteError::InvalidEncoding)?;
    postcard::from_bytes(&bytes).map_err(|_| InviteError::InvalidEncoding)
}

fn verify_signed(signed: &SignedInvite, now_unix: Option<i64>) -> Result<(), InviteError> {
    wire_profile(signed.payload.version)?;
    validate_entry_path(&signed.payload.entry_path)?;
    if normalize_bootstrap_origin(&signed.payload.bootstrap_origin)?
        != signed.payload.bootstrap_origin
    {
        return Err(InviteError::InvalidBootstrapUrl);
    }
    if now_unix.is_some_and(|now| signed.payload.expires_at_unix <= now) {
        return Err(InviteError::Expired);
    }

    let public =
        PublicKey::from_bytes(&signed.signer).map_err(|_| InviteError::InvalidSignature)?;
    if public.to_z32() != signed.payload.site_id {
        return Err(InviteError::SiteIdentityMismatch);
    }
    let signature_bytes: [u8; Signature::LENGTH] = signed
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| InviteError::InvalidSignature)?;
    let signature = Signature::from_bytes(&signature_bytes);
    let bytes = postcard::to_allocvec(&signed.payload).map_err(|_| InviteError::InvalidEncoding)?;
    public
        .verify(&bytes, &signature)
        .map_err(|_| InviteError::InvalidSignature)?;

    let ticket = EndpointTicket::from_str(&signed.payload.endpoint_ticket)
        .map_err(|_| InviteError::EndpointMismatch)?;
    if ticket.endpoint_addr().id != public {
        return Err(InviteError::EndpointMismatch);
    }
    Ok(())
}

pub fn alpn_for_invite(version: u8) -> Result<&'static [u8], InviteError> {
    Ok(wire_profile(version)?.alpn)
}

#[derive(Debug, Clone, Copy)]
struct WireProfile {
    alpn: &'static [u8],
    bootstrap_path: &'static str,
    fragment_prefix: &'static str,
}

fn wire_profile(version: u8) -> Result<WireProfile, InviteError> {
    match version {
        INVITE_VERSION => Ok(WireProfile {
            alpn: ALPN,
            bootstrap_path: BOOTSTRAP_PATH,
            fragment_prefix: "u4",
        }),
        LEGACY_V3_INVITE_VERSION => Ok(WireProfile {
            alpn: LEGACY_V3_ALPN,
            bootstrap_path: BOOTSTRAP_PATH,
            fragment_prefix: "u3",
        }),
        LEGACY_V2_INVITE_VERSION => Ok(WireProfile {
            alpn: LEGACY_V2_ALPN,
            bootstrap_path: LEGACY_V2_BOOTSTRAP_PATH,
            fragment_prefix: "m2",
        }),
        _ => Err(InviteError::UnsupportedVersion),
    }
}

fn normalize_bootstrap_origin(raw: &str) -> Result<String, InviteError> {
    let url = Url::parse(raw).map_err(|_| InviteError::InvalidBootstrapUrl)?;
    let host = url.host_str().ok_or(InviteError::InvalidBootstrapUrl)?;
    let secure_scheme = url.scheme() == "https"
        || (url.scheme() == "http" && (host == "localhost" || host.ends_with(".localhost")));
    if !secure_scheme
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(InviteError::InvalidBootstrapUrl);
    }
    Ok(url.origin().ascii_serialization())
}

fn validate_entry_path(path: &str) -> Result<(), InviteError> {
    if !path.starts_with('/') || path.contains('\\') || path.split('/').any(|part| part == "..") {
        return Err(InviteError::InvalidEntryPath);
    }
    Ok(())
}

pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
    T: Serialize,
{
    use tokio::io::AsyncWriteExt as _;
    let bytes = postcard::to_allocvec(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "protocol frame exceeds limit",
        ));
    }
    writer.write_u32(bytes.len() as u32).await?;
    writer.write_all(&bytes).await
}

pub async fn read_frame<R, T>(reader: &mut R) -> std::io::Result<T>
where
    R: tokio::io::AsyncRead + Unpin,
    T: DeserializeOwned,
{
    use tokio::io::AsyncReadExt as _;
    let len = reader.read_u32().await? as usize;
    if len > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "protocol frame exceeds limit",
        ));
    }
    let mut bytes = vec![0_u8; len];
    reader.read_exact(&mut bytes).await?;
    postcard::from_bytes(&bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh_base::EndpointAddr;

    fn fixture(expiry: i64) -> (SecretKey, String, [u8; 32]) {
        let identity = SecretKey::generate();
        let ticket = EndpointTicket::new(EndpointAddr::new(identity.public())).to_string();
        let capability = [7_u8; 32];
        let encoded = sign_invite(
            &identity,
            InviteGrant {
                bootstrap_origin: "https://sites.example".into(),
                endpoint_ticket: ticket,
                invite_id: Uuid::nil(),
                capability,
                expires_at_unix: expiry,
                entry_path: "/index.html".into(),
                max_sessions: 2,
            },
        )
        .unwrap();
        (identity, encoded, capability)
    }

    #[test]
    fn invitation_round_trips_and_binds_url_origin() {
        let (identity, encoded, capability) = fixture(2_000);
        let url = invite_url(&encoded).unwrap();
        assert_eq!(url.path(), BOOTSTRAP_PATH);
        assert!(url.fragment().unwrap().starts_with("u4="));
        let payload = verify_invite_url(url.as_str(), 1_000).unwrap();
        assert_eq!(payload.site_id, identity.public().to_z32());
        assert_eq!(payload.capability, capability);
    }

    #[test]
    fn tampering_is_rejected() {
        let (_, encoded, _) = fixture(2_000);
        let mut bytes = URL_SAFE_NO_PAD.decode(encoded).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 1;
        let tampered = URL_SAFE_NO_PAD.encode(bytes);
        assert!(verify_encoded_invite(&tampered, 1_000).is_err());
    }

    #[test]
    fn wrong_origin_and_expiry_are_rejected() {
        let (_, encoded, _) = fixture(2_000);
        let wrong = format!("https://attacker.sites.example/.urspace/open/#u4={encoded}");
        assert!(matches!(
            verify_invite_url(&wrong, 1_000),
            Err(InviteError::OriginMismatch)
        ));
        assert!(matches!(
            verify_encoded_invite(&encoded, 2_000),
            Err(InviteError::Expired)
        ));
        assert_eq!(
            verify_invite_url_for_resume(invite_url(&encoded).unwrap().as_str())
                .unwrap()
                .expires_at_unix,
            2_000
        );
    }

    #[test]
    fn bootstrap_origin_is_signed_and_cannot_be_rewrapped() {
        let (_, encoded, _) = fixture(2_000);
        let payload = verify_encoded_invite(&encoded, 1_000).unwrap();
        assert_eq!(payload.bootstrap_origin, "https://sites.example");

        let attacker = format!(
            "https://{}.attacker.example/.urspace/open/#u4={encoded}",
            payload.site_id
        );
        assert!(matches!(
            verify_invite_url(&attacker, 1_000),
            Err(InviteError::OriginMismatch)
        ));
    }

    #[test]
    fn local_http_bootstrap_is_allowed_but_remote_http_is_not() {
        assert_eq!(
            normalize_bootstrap_origin("http://localhost:8080/").unwrap(),
            "http://localhost:8080"
        );
        assert!(matches!(
            normalize_bootstrap_origin("http://sites.example"),
            Err(InviteError::InvalidBootstrapUrl)
        ));
    }

    #[test]
    fn legacy_v2_invites_keep_their_original_url_and_alpn() {
        let identity = SecretKey::generate();
        let encoded = sign_invite_version(
            &identity,
            InviteGrant {
                bootstrap_origin: "https://sites.example".into(),
                endpoint_ticket: EndpointTicket::new(EndpointAddr::new(identity.public()))
                    .to_string(),
                invite_id: Uuid::nil(),
                capability: [3_u8; 32],
                expires_at_unix: 2_000,
                entry_path: "/".into(),
                max_sessions: 1,
            },
            LEGACY_V2_INVITE_VERSION,
        )
        .unwrap();
        let url = invite_url(&encoded).unwrap();
        assert_eq!(url.path(), LEGACY_V2_BOOTSTRAP_PATH);
        assert!(url.fragment().unwrap().starts_with("m2="));
        let payload = verify_invite_url(url.as_str(), 1_000).unwrap();
        assert_eq!(payload.version, LEGACY_V2_INVITE_VERSION);
        assert_eq!(alpn_for_invite(payload.version).unwrap(), LEGACY_V2_ALPN);

        let mismatched = url.as_str().replace("#m2=", "#u3=");
        assert!(matches!(
            verify_invite_url(&mismatched, 1_000),
            Err(InviteError::UnsupportedVersion)
        ));
    }

    #[test]
    fn legacy_v3_invites_keep_their_original_url_and_alpn() {
        let identity = SecretKey::generate();
        let encoded = sign_invite_version(
            &identity,
            InviteGrant {
                bootstrap_origin: "https://sites.example".into(),
                endpoint_ticket: EndpointTicket::new(EndpointAddr::new(identity.public()))
                    .to_string(),
                invite_id: Uuid::nil(),
                capability: [4_u8; 32],
                expires_at_unix: 2_000,
                entry_path: "/".into(),
                max_sessions: 1,
            },
            LEGACY_V3_INVITE_VERSION,
        )
        .unwrap();
        let url = invite_url(&encoded).unwrap();
        assert!(url.fragment().unwrap().starts_with("u3="));
        assert_eq!(
            alpn_for_invite(LEGACY_V3_INVITE_VERSION).unwrap(),
            LEGACY_V3_ALPN
        );
        assert_eq!(
            verify_invite_url(url.as_str(), 1_000).unwrap().version,
            LEGACY_V3_INVITE_VERSION
        );
    }

    fn grant_fixture() -> (SecretKey, SecretKey, String) {
        let host = SecretKey::generate();
        let session = SecretKey::generate();
        let grant = sign_session_grant(
            &host,
            SessionGrantIssue {
                bootstrap_origin: "https://sites.example".into(),
                endpoint_ticket: EndpointTicket::new(EndpointAddr::new(host.public())).to_string(),
                invite_id: Uuid::from_u128(1),
                session_id: Uuid::from_u128(2),
                session_public_key: *session.public().as_bytes(),
                issued_at_unix: 1_000,
                expires_at_unix: 2_000,
                authorization_epoch: 7,
                entry_path: "/app".into(),
            },
        )
        .unwrap();
        (host, session, grant)
    }

    #[test]
    fn session_grant_round_trips_without_a_bearer_capability() {
        let (host, session, grant) = grant_fixture();
        let payload = verify_session_grant(&grant, 1_500).unwrap();
        assert_eq!(payload.host_id, *host.public().as_bytes());
        assert_eq!(payload.session_public_key, *session.public().as_bytes());
        assert_eq!(payload.session_id, Uuid::from_u128(2));
        assert_eq!(payload.authorization_epoch, 7);
        assert!(!grant.contains("sites.example"));
        assert!(matches!(
            verify_session_grant(&grant, 2_000),
            Err(SessionGrantError::Expired)
        ));
        assert_eq!(
            session_grant_origin(&payload).unwrap(),
            format!("https://{}.sites.example", host.public().to_z32())
        );
    }

    #[test]
    fn tampered_session_grants_are_rejected() {
        let (_, _, grant) = grant_fixture();
        let encoded = grant.strip_prefix(SESSION_GRANT_PREFIX).unwrap();
        let mut bytes = URL_SAFE_NO_PAD.decode(encoded).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 1;
        let tampered = format!("{SESSION_GRANT_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
        assert!(verify_session_grant(&tampered, 1_500).is_err());
    }

    #[test]
    fn session_proofs_bind_grant_endpoint_and_host_nonce() {
        let (host, session, grant) = grant_fixture();
        let endpoint = SecretKey::generate();
        let proof = SessionProofPayload {
            version: SESSION_PROOF_VERSION,
            purpose: SessionProofPurpose::Resume,
            host_id: *host.public().as_bytes(),
            site_id: host.public().to_z32(),
            alpn: ALPN.to_vec(),
            session_id: Uuid::from_u128(2),
            session_grant_hash: session_grant_hash(&grant),
            endpoint_id: *endpoint.public().as_bytes(),
            challenge_id: Uuid::from_u128(3),
            nonce: [9_u8; 32],
            expires_at_unix: 1_600,
        };
        let signature = sign_session_proof(&session, &proof).unwrap();
        verify_session_proof(session.public().as_bytes(), &proof, &signature, 1_500).unwrap();

        let wrong_session = SecretKey::generate();
        assert!(
            verify_session_proof(wrong_session.public().as_bytes(), &proof, &signature, 1_500)
                .is_err()
        );

        let mut wrong_endpoint = proof.clone();
        wrong_endpoint.endpoint_id = *SecretKey::generate().public().as_bytes();
        assert!(
            verify_session_proof(
                session.public().as_bytes(),
                &wrong_endpoint,
                &signature,
                1_500
            )
            .is_err()
        );

        let mut wrong_nonce = proof;
        wrong_nonce.nonce[0] ^= 1;
        assert!(
            verify_session_proof(session.public().as_bytes(), &wrong_nonce, &signature, 1_500)
                .is_err()
        );
    }

    #[test]
    fn session_proofs_reject_a_substituted_site_identity() {
        let (host, session, grant) = grant_fixture();
        let endpoint = SecretKey::generate();
        let mut proof = SessionProofPayload {
            version: SESSION_PROOF_VERSION,
            purpose: SessionProofPurpose::Resume,
            host_id: *host.public().as_bytes(),
            site_id: host.public().to_z32(),
            alpn: ALPN.to_vec(),
            session_id: Uuid::from_u128(2),
            session_grant_hash: session_grant_hash(&grant),
            endpoint_id: *endpoint.public().as_bytes(),
            challenge_id: Uuid::from_u128(3),
            nonce: [9_u8; 32],
            expires_at_unix: 1_600,
        };
        proof.site_id = SecretKey::generate().public().to_z32();

        assert!(matches!(
            sign_session_proof(&session, &proof),
            Err(SessionGrantError::InvalidProof)
        ));
    }

    #[test]
    fn tunnel_proofs_are_versioned_and_bound_to_the_tunnel_alpn() {
        let (host, session, grant) = grant_fixture();
        let endpoint = SecretKey::generate();
        let mut proof = SessionProofPayload {
            version: SESSION_PROOF_VERSION,
            purpose: SessionProofPurpose::Resume,
            host_id: *host.public().as_bytes(),
            site_id: host.public().to_z32(),
            alpn: TUNNEL_ALPN.to_vec(),
            session_id: Uuid::from_u128(2),
            session_grant_hash: session_grant_hash(&grant),
            endpoint_id: *endpoint.public().as_bytes(),
            challenge_id: Uuid::from_u128(4),
            nonce: [10_u8; 32],
            expires_at_unix: 1_600,
        };
        let signature = sign_session_proof(&session, &proof).unwrap();
        verify_session_proof(session.public().as_bytes(), &proof, &signature, 1_500).unwrap();

        proof.alpn = ALPN.to_vec();
        assert!(
            verify_session_proof(session.public().as_bytes(), &proof, &signature, 1_500).is_err()
        );
        proof.alpn = b"urspace-tunnel/2".to_vec();
        assert!(matches!(
            sign_session_proof(&session, &proof),
            Err(SessionGrantError::InvalidProof)
        ));
    }
}
