//! Versioned wire types and self-authenticating invitation URLs for Urspace.

use std::str::FromStr;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh_base::{PublicKey, SecretKey, Signature};
use iroh_tickets::endpoint::EndpointTicket;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

pub const ALPN: &[u8] = b"urspace-site/3";
pub const INVITE_VERSION: u8 = 3;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const BOOTSTRAP_PATH: &str = "/.urspace/open/";

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
    let url = Url::parse(raw).map_err(|_| InviteError::InvalidBootstrapUrl)?;
    let fragment = url.fragment().ok_or(InviteError::MissingFragment)?;
    let (fragment_version, encoded) = if let Some(encoded) = fragment.strip_prefix("u3=") {
        (INVITE_VERSION, encoded)
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
    verify_signed(&signed, now_unix)?;
    Ok(signed.payload)
}

fn decode_signed(encoded: &str) -> Result<SignedInvite, InviteError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| InviteError::InvalidEncoding)?;
    postcard::from_bytes(&bytes).map_err(|_| InviteError::InvalidEncoding)
}

fn verify_signed(signed: &SignedInvite, now_unix: i64) -> Result<(), InviteError> {
    wire_profile(signed.payload.version)?;
    validate_entry_path(&signed.payload.entry_path)?;
    if normalize_bootstrap_origin(&signed.payload.bootstrap_origin)?
        != signed.payload.bootstrap_origin
    {
        return Err(InviteError::InvalidBootstrapUrl);
    }
    if signed.payload.expires_at_unix <= now_unix {
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
        assert!(url.fragment().unwrap().starts_with("u3="));
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
        let wrong = format!("https://attacker.sites.example/.urspace/open/#u3={encoded}");
        assert!(matches!(
            verify_invite_url(&wrong, 1_000),
            Err(InviteError::OriginMismatch)
        ));
        assert!(matches!(
            verify_encoded_invite(&encoded, 2_000),
            Err(InviteError::Expired)
        ));
    }

    #[test]
    fn bootstrap_origin_is_signed_and_cannot_be_rewrapped() {
        let (_, encoded, _) = fixture(2_000);
        let payload = verify_encoded_invite(&encoded, 1_000).unwrap();
        assert_eq!(payload.bootstrap_origin, "https://sites.example");

        let attacker = format!(
            "https://{}.attacker.example/.urspace/open/#u3={encoded}",
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
}
