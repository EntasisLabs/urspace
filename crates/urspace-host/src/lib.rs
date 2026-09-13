use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use futures_util::{SinkExt as _, StreamExt as _};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{EndpointId, SecretKey};
use percent_encoding::percent_decode_str;
use rand::Rng as _;
use reqwest::redirect::Policy;
use subtle::ConstantTimeEq as _;
use tokio::sync::Semaphore;
use tokio_tungstenite::tungstenite::Message;
use url::Url;
use urspace_protocol::{
    ALPN, ClientAuthV4, ClientProofV4, DenialCode, Header, RequestMethod, ServerAuthV4,
    ServerHelloV4, SessionChallengeV4, SessionGrantIssue, SessionGrantPayload, SessionProofPayload,
    SessionProofPurpose, SiteRequest, SiteResponseHead, SocketMessage, read_frame,
    session_grant_hash, sign_session_grant, verify_session_grant, verify_session_proof,
    write_frame,
};
use uuid::Uuid;

const MAX_CONCURRENT_REQUESTS_PER_CONNECTION: usize = 64;
const MAX_REQUEST_BODY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;
const SESSION_CHALLENGE_TTL_SECONDS: i64 = 15;
const SESSION_GRANT_EXPIRY_UNIX: i64 = i64::MAX;

#[derive(Debug, Clone)]
struct InviteRecord {
    capability_hash: [u8; 32],
    expires_at_unix: i64,
    remaining_sessions: u32,
    admitted_sessions: HashSet<Uuid>,
    admissions_closed: bool,
    revoked: bool,
}

#[derive(Debug, Clone)]
struct SessionRecord {
    invite_id: Uuid,
    session_public_key: [u8; 32],
    authorization_epoch: u64,
    endpoint_id: EndpointId,
    kicked: bool,
}

#[derive(Debug, Clone)]
struct ActiveConnection {
    stable_id: usize,
    connection: Connection,
}

#[derive(Debug, Default)]
struct RegistryState {
    invites: HashMap<Uuid, InviteRecord>,
    sessions: HashMap<Uuid, SessionRecord>,
    active: HashMap<Uuid, ActiveConnection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub endpoint_id: EndpointId,
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizedSession {
    session_id: Uuid,
    invite_id: Uuid,
    endpoint_id: EndpointId,
}

impl AuthorizedSession {
    pub fn session_id(self) -> Uuid {
        self.session_id
    }
}

#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    inner: Arc<Mutex<RegistryState>>,
}

impl CapabilityRegistry {
    pub fn insert(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        expires_at_unix: i64,
        max_sessions: u32,
    ) {
        let record = InviteRecord {
            capability_hash: capability_hash(capability),
            expires_at_unix,
            remaining_sessions: max_sessions,
            admitted_sessions: HashSet::new(),
            admissions_closed: false,
            revoked: false,
        };
        self.inner
            .lock()
            .expect("capability registry poisoned")
            .invites
            .insert(invite_id, record);
    }

    pub fn can_admit(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        now_unix: i64,
    ) -> Result<(), DenialCode> {
        let candidate_hash = capability_hash(capability);
        let guard = self.inner.lock().expect("capability registry poisoned");
        let record = guard.invites.get(&invite_id).ok_or(DenialCode::Invalid)?;
        if record.capability_hash.ct_eq(&candidate_hash).unwrap_u8() != 1 {
            return Err(DenialCode::Invalid);
        }
        if record.revoked {
            return Err(DenialCode::Revoked);
        }
        if record.admissions_closed {
            return Err(DenialCode::Revoked);
        }
        if record.expires_at_unix <= now_unix {
            return Err(DenialCode::Expired);
        }
        if record.remaining_sessions == 0 {
            return Err(DenialCode::SessionLimit);
        }
        Ok(())
    }

    pub fn admit(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        session_id: Uuid,
        session_public_key: [u8; 32],
        endpoint_id: EndpointId,
        now_unix: i64,
    ) -> Result<AuthorizedSession, DenialCode> {
        self.can_admit_locked(
            invite_id,
            capability,
            session_id,
            session_public_key,
            endpoint_id,
            now_unix,
        )
    }

    fn can_admit_locked(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        session_id: Uuid,
        session_public_key: [u8; 32],
        endpoint_id: EndpointId,
        now_unix: i64,
    ) -> Result<AuthorizedSession, DenialCode> {
        let candidate_hash = capability_hash(capability);
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        if guard.sessions.contains_key(&session_id) {
            return Err(DenialCode::Invalid);
        }
        let record = guard
            .invites
            .get_mut(&invite_id)
            .ok_or(DenialCode::Invalid)?;
        if record.capability_hash.ct_eq(&candidate_hash).unwrap_u8() != 1 {
            return Err(DenialCode::Invalid);
        }
        if record.revoked || record.admissions_closed {
            return Err(DenialCode::Revoked);
        }
        if record.expires_at_unix <= now_unix {
            return Err(DenialCode::Expired);
        }
        if record.remaining_sessions == 0 {
            return Err(DenialCode::SessionLimit);
        }
        record.remaining_sessions -= 1;
        record.admitted_sessions.insert(session_id);
        guard.sessions.insert(
            session_id,
            SessionRecord {
                invite_id,
                session_public_key,
                authorization_epoch: 0,
                endpoint_id,
                kicked: false,
            },
        );
        Ok(AuthorizedSession {
            session_id,
            invite_id,
            endpoint_id,
        })
    }

    pub fn can_resume(
        &self,
        grant: &urspace_protocol::SessionGrantPayload,
    ) -> Result<(), DenialCode> {
        let guard = self.inner.lock().expect("capability registry poisoned");
        validate_grant_record(&guard, grant).map(|_| ())
    }

    pub fn resume(
        &self,
        grant: &urspace_protocol::SessionGrantPayload,
        endpoint_id: EndpointId,
    ) -> Result<AuthorizedSession, DenialCode> {
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        validate_grant_record(&guard, grant)?;
        let record = guard
            .sessions
            .get_mut(&grant.session_id)
            .ok_or(DenialCode::Invalid)?;
        record.endpoint_id = endpoint_id;
        Ok(AuthorizedSession {
            session_id: grant.session_id,
            invite_id: grant.invite_id,
            endpoint_id,
        })
    }

    pub fn session_is_active(&self, session: AuthorizedSession) -> bool {
        let guard = self.inner.lock().expect("capability registry poisoned");
        let Some(record) = guard.sessions.get(&session.session_id) else {
            return false;
        };
        let invite_active = guard
            .invites
            .get(&session.invite_id)
            .is_some_and(|invite| !invite.revoked);
        invite_active
            && !record.kicked
            && record.invite_id == session.invite_id
            && record.endpoint_id == session.endpoint_id
    }

    pub fn close_admissions(&self, invite_id: Uuid) -> bool {
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        let Some(record) = guard.invites.get_mut(&invite_id) else {
            return false;
        };
        record.admissions_closed = true;
        true
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        let guard = self.inner.lock().expect("capability registry poisoned");
        let mut sessions: Vec<_> = guard
            .sessions
            .iter()
            .filter_map(|(session_id, record)| {
                let invite_active = guard
                    .invites
                    .get(&record.invite_id)
                    .is_some_and(|invite| !invite.revoked);
                (invite_active && !record.kicked).then_some(SessionInfo {
                    session_id: *session_id,
                    endpoint_id: record.endpoint_id,
                    connected: guard.active.contains_key(session_id),
                })
            })
            .collect();
        sessions.sort_by_key(|session| session.session_id);
        sessions
    }

    pub fn kick(&self, session_id: Uuid) -> bool {
        let connection = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            let Some(record) = guard.sessions.get_mut(&session_id) else {
                return false;
            };
            if record.kicked {
                return false;
            }
            record.kicked = true;
            guard
                .active
                .remove(&session_id)
                .map(|active| active.connection)
        };
        if let Some(connection) = connection {
            connection.close(0_u8.into(), b"session kicked by host");
        }
        true
    }

    pub fn kick_all(&self) -> usize {
        let (count, connections) = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            let mut count = 0;
            for record in guard.sessions.values_mut() {
                if !record.kicked {
                    record.kicked = true;
                    count += 1;
                }
            }
            let connections = guard
                .active
                .drain()
                .map(|(_, active)| active.connection)
                .collect::<Vec<_>>();
            (count, connections)
        };
        for connection in connections {
            connection.close(0_u8.into(), b"session kicked by host");
        }
        count
    }

    fn session_connected(&self, session: AuthorizedSession, connection: Connection) {
        let stable_id = connection.stable_id();
        let replaced = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            let active = guard
                .sessions
                .get(&session.session_id)
                .is_some_and(|record| !record.kicked && record.endpoint_id == session.endpoint_id)
                && guard
                    .invites
                    .get(&session.invite_id)
                    .is_some_and(|invite| !invite.revoked);
            if !active {
                drop(guard);
                connection.close(0_u8.into(), b"browser session is no longer admitted");
                return;
            }
            guard.active.insert(
                session.session_id,
                ActiveConnection {
                    stable_id,
                    connection,
                },
            )
        };
        if let Some(replaced) = replaced
            && replaced.stable_id != stable_id
        {
            replaced
                .connection
                .close(0_u8.into(), b"browser session reconnected");
        }
    }

    fn session_disconnected(&self, session: AuthorizedSession, stable_id: usize) {
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        if guard
            .active
            .get(&session.session_id)
            .is_some_and(|active| active.stable_id == stable_id)
        {
            guard.active.remove(&session.session_id);
        }
    }

    pub fn revoke(&self, invite_id: Uuid) -> bool {
        let connections = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            let Some(record) = guard.invites.get_mut(&invite_id) else {
                return false;
            };
            record.revoked = true;
            let session_ids: Vec<_> = guard
                .sessions
                .iter()
                .filter_map(|(session_id, session)| {
                    (session.invite_id == invite_id).then_some(*session_id)
                })
                .collect();
            session_ids
                .into_iter()
                .filter_map(|session_id| guard.active.remove(&session_id))
                .map(|active| active.connection)
                .collect::<Vec<_>>()
        };
        for connection in connections {
            connection.close(0_u8.into(), b"invitation revoked by host");
        }
        true
    }
}

fn validate_grant_record<'a>(
    state: &'a RegistryState,
    grant: &urspace_protocol::SessionGrantPayload,
) -> Result<&'a SessionRecord, DenialCode> {
    let session = state
        .sessions
        .get(&grant.session_id)
        .ok_or(DenialCode::Invalid)?;
    let invite = state
        .invites
        .get(&session.invite_id)
        .ok_or(DenialCode::Invalid)?;
    if invite.revoked || session.kicked {
        return Err(DenialCode::Revoked);
    }
    if session.invite_id != grant.invite_id
        || session.session_public_key != grant.session_public_key
        || session.authorization_epoch != grant.authorization_epoch
    {
        return Err(DenialCode::Invalid);
    }
    Ok(session)
}

fn capability_hash(capability: &[u8; 32]) -> [u8; 32] {
    *blake3::hash(capability).as_bytes()
}

#[derive(Debug, Clone)]
pub struct StaticSite {
    root: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LoopbackSite {
    http_origin: Url,
    ws_origin: Url,
    client: reqwest::Client,
}

impl LoopbackSite {
    pub fn open(raw_origin: &str) -> Result<Self> {
        let mut http_origin = Url::parse(raw_origin).context("parse loopback upstream")?;
        let host = http_origin
            .host_str()
            .context("loopback upstream must have a host")?;
        if http_origin.scheme() != "http"
            || !matches!(host, "127.0.0.1" | "::1" | "[::1]" | "localhost")
            || !http_origin.username().is_empty()
            || http_origin.password().is_some()
            || http_origin.query().is_some()
            || http_origin.fragment().is_some()
            || !matches!(http_origin.path(), "" | "/")
        {
            anyhow::bail!("upstream must be an HTTP loopback origin without a path or credentials");
        }
        if host == "localhost" {
            http_origin
                .set_host(Some("127.0.0.1"))
                .map_err(|_| anyhow::anyhow!("could not normalize loopback upstream"))?;
        }
        http_origin.set_path("/");
        let mut ws_origin = http_origin.clone();
        ws_origin
            .set_scheme("ws")
            .map_err(|_| anyhow::anyhow!("could not derive WebSocket origin"))?;
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .build()
            .context("build loopback HTTP client")?;
        Ok(Self {
            http_origin,
            ws_origin,
            client,
        })
    }

    pub fn origin(&self) -> &str {
        self.http_origin.as_str()
    }
}

#[derive(Debug, Clone)]
enum SiteSource {
    Static(StaticSite),
    Loopback(LoopbackSite),
}

impl StaticSite {
    pub async fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = tokio::fs::canonicalize(root.as_ref())
            .await
            .with_context(|| format!("resolve site root {}", root.as_ref().display()))?;
        if !tokio::fs::metadata(&root).await?.is_dir() {
            anyhow::bail!("site root is not a directory: {}", root.display());
        }
        Ok(Self {
            root: Arc::new(root),
        })
    }

    async fn resolve(&self, raw_path: &str) -> Result<PathBuf, ResolveError> {
        let path_only = raw_path.split_once('?').map_or(raw_path, |(path, _)| path);
        let decoded = percent_decode_str(path_only)
            .decode_utf8()
            .map_err(|_| ResolveError::Invalid)?;
        if decoded.contains('\0') || decoded.contains('\\') {
            return Err(ResolveError::Invalid);
        }

        let relative = decoded.trim_start_matches('/');
        let relative_path = Path::new(relative);
        if relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return Err(ResolveError::OutsideRoot);
        }

        let mut candidate = self.root.join(relative_path);
        let metadata = tokio::fs::metadata(&candidate)
            .await
            .map_err(map_io_error)?;
        if metadata.is_dir() {
            candidate = candidate.join("index.html");
        }
        let canonical = tokio::fs::canonicalize(&candidate)
            .await
            .map_err(map_io_error)?;
        if !canonical.starts_with(self.root.as_ref()) {
            return Err(ResolveError::OutsideRoot);
        }
        if !tokio::fs::metadata(&canonical)
            .await
            .map_err(map_io_error)?
            .is_file()
        {
            return Err(ResolveError::NotFound);
        }
        Ok(canonical)
    }
}

#[derive(Debug)]
enum ResolveError {
    Invalid,
    OutsideRoot,
    NotFound,
    Io(io::Error),
}

fn map_io_error(error: io::Error) -> ResolveError {
    if error.kind() == io::ErrorKind::NotFound {
        ResolveError::NotFound
    } else {
        ResolveError::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct SiteProtocol {
    registry: CapabilityRegistry,
    source: SiteSource,
    issuer: SessionGrantIssuer,
}

#[derive(Debug, Clone)]
pub struct SessionGrantIssuer {
    identity: SecretKey,
    bootstrap_origin: String,
    endpoint_ticket: String,
    entry_path: String,
}

impl SessionGrantIssuer {
    pub fn new(
        identity: SecretKey,
        bootstrap_origin: String,
        endpoint_ticket: String,
        entry_path: String,
    ) -> Self {
        Self {
            identity,
            bootstrap_origin,
            endpoint_ticket,
            entry_path,
        }
    }
}

impl SiteProtocol {
    pub fn new(registry: CapabilityRegistry, site: StaticSite, issuer: SessionGrantIssuer) -> Self {
        Self {
            registry,
            source: SiteSource::Static(site),
            issuer,
        }
    }

    pub fn loopback(
        registry: CapabilityRegistry,
        site: LoopbackSite,
        issuer: SessionGrantIssuer,
    ) -> Self {
        Self {
            registry,
            source: SiteSource::Loopback(site),
            issuer,
        }
    }
}

impl ProtocolHandler for SiteProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        self.serve_connection(connection)
            .await
            .map_err(|error| AcceptError::from_err(io::Error::other(error.to_string())))
    }
}

impl SiteProtocol {
    async fn serve_connection(&self, connection: Connection) -> Result<()> {
        let (mut hello_send, mut hello_recv) = connection
            .accept_bi()
            .await
            .context("accept authorization stream")?;
        let hello: ClientAuthV4 = read_frame(&mut hello_recv)
            .await
            .context("read client authorization")?;
        let now = unix_now();
        let pending = match self.prepare_authorization(hello, now) {
            Ok(pending) => pending,
            Err(code) => {
                write_frame(&mut hello_send, &ServerAuthV4::Denied { code })
                    .await
                    .context("write authorization denial")?;
                hello_send.finish().context("finish authorization denial")?;
                let _ = tokio::time::timeout(Duration::from_secs(1), hello_send.stopped()).await;
                return Ok(());
            }
        };
        let challenge = SessionChallengeV4 {
            challenge_id: Uuid::new_v4(),
            session_id: pending.session_id(),
            nonce: rand::rng().random(),
            expires_at_unix: now.saturating_add(SESSION_CHALLENGE_TTL_SECONDS),
        };
        write_frame(&mut hello_send, &ServerAuthV4::Challenge(challenge.clone()))
            .await
            .context("write session challenge")?;
        let proof: ClientProofV4 = read_frame(&mut hello_recv)
            .await
            .context("read session proof")?;
        let proof_payload = pending.proof_payload(&self.issuer, &connection, &challenge);
        let proof_valid = verify_session_proof(
            pending.session_public_key(),
            &proof_payload,
            &proof.signature,
            unix_now(),
        )
        .is_ok();
        let authorization = if proof_valid {
            pending.finalize(&self.registry, connection.remote_id(), unix_now())
        } else {
            Err(DenialCode::Invalid)
        };
        let mut granted_session = None;
        let reply = match authorization {
            Ok(session) => match self.issue_grant(&pending, unix_now()) {
                Ok(session_grant) => {
                    granted_session = Some(session);
                    ServerHelloV4::Granted { session_grant }
                }
                Err(_) => {
                    self.registry.kick(session.session_id());
                    ServerHelloV4::Denied {
                        code: DenialCode::Invalid,
                    }
                }
            },
            Err(code) => ServerHelloV4::Denied { code },
        };
        write_frame(&mut hello_send, &reply)
            .await
            .context("write authorization response")?;
        hello_send
            .finish()
            .context("finish authorization response")?;
        let Some(session) = granted_session else {
            let _ = tokio::time::timeout(Duration::from_secs(1), hello_send.stopped()).await;
            return Ok(());
        };

        let stable_id = connection.stable_id();
        self.registry.session_connected(session, connection.clone());
        let limit = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS_PER_CONNECTION));
        loop {
            let Ok((send, recv)) = connection.accept_bi().await else {
                break;
            };
            let Ok(permit) = Arc::clone(&limit).acquire_owned().await else {
                break;
            };
            let registry = self.registry.clone();
            let source = self.source.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = serve_request(registry, source, session, send, recv).await;
            });
        }
        self.registry.session_disconnected(session, stable_id);
        Ok(())
    }

    fn prepare_authorization(
        &self,
        hello: ClientAuthV4,
        now_unix: i64,
    ) -> Result<PendingAuthorization, DenialCode> {
        match hello {
            ClientAuthV4::Admit {
                invite_id,
                capability,
                session_public_key,
            } => {
                iroh::PublicKey::from_bytes(&session_public_key)
                    .map_err(|_| DenialCode::Invalid)?;
                self.registry.can_admit(invite_id, &capability, now_unix)?;
                Ok(PendingAuthorization::Admit {
                    invite_id,
                    capability,
                    session_id: Uuid::new_v4(),
                    session_public_key,
                })
            }
            ClientAuthV4::Resume { session_grant } => {
                let grant = verify_session_grant(&session_grant, now_unix)
                    .map_err(|_| DenialCode::Invalid)?;
                if grant.host_id != *self.issuer.identity.public().as_bytes()
                    || grant.bootstrap_origin != self.issuer.bootstrap_origin
                    || grant.endpoint_ticket != self.issuer.endpoint_ticket
                    || grant.entry_path != self.issuer.entry_path
                {
                    return Err(DenialCode::Invalid);
                }
                self.registry.can_resume(&grant)?;
                Ok(PendingAuthorization::Resume {
                    session_grant,
                    grant,
                })
            }
        }
    }

    fn issue_grant(
        &self,
        pending: &PendingAuthorization,
        now_unix: i64,
    ) -> Result<String, urspace_protocol::SessionGrantError> {
        sign_session_grant(
            &self.issuer.identity,
            SessionGrantIssue {
                bootstrap_origin: self.issuer.bootstrap_origin.clone(),
                endpoint_ticket: self.issuer.endpoint_ticket.clone(),
                invite_id: pending.invite_id(),
                session_id: pending.session_id(),
                session_public_key: *pending.session_public_key(),
                issued_at_unix: now_unix,
                // Browser grants remain valid for the lifetime of this in-memory host
                // session. Kick, invite revocation, and host restart are authoritative.
                expires_at_unix: SESSION_GRANT_EXPIRY_UNIX,
                authorization_epoch: pending.authorization_epoch(),
                entry_path: self.issuer.entry_path.clone(),
            },
        )
    }
}

#[derive(Debug)]
enum PendingAuthorization {
    Admit {
        invite_id: Uuid,
        capability: [u8; 32],
        session_id: Uuid,
        session_public_key: [u8; 32],
    },
    Resume {
        session_grant: String,
        grant: SessionGrantPayload,
    },
}

impl PendingAuthorization {
    fn session_id(&self) -> Uuid {
        match self {
            Self::Admit { session_id, .. } => *session_id,
            Self::Resume { grant, .. } => grant.session_id,
        }
    }

    fn invite_id(&self) -> Uuid {
        match self {
            Self::Admit { invite_id, .. } => *invite_id,
            Self::Resume { grant, .. } => grant.invite_id,
        }
    }

    fn session_public_key(&self) -> &[u8; 32] {
        match self {
            Self::Admit {
                session_public_key, ..
            } => session_public_key,
            Self::Resume { grant, .. } => &grant.session_public_key,
        }
    }

    fn authorization_epoch(&self) -> u64 {
        match self {
            Self::Admit { .. } => 0,
            Self::Resume { grant, .. } => grant.authorization_epoch,
        }
    }

    fn proof_payload(
        &self,
        issuer: &SessionGrantIssuer,
        connection: &Connection,
        challenge: &SessionChallengeV4,
    ) -> SessionProofPayload {
        SessionProofPayload {
            version: urspace_protocol::SESSION_PROOF_VERSION,
            purpose: match self {
                Self::Admit { .. } => SessionProofPurpose::Admit,
                Self::Resume { .. } => SessionProofPurpose::Resume,
            },
            host_id: *issuer.identity.public().as_bytes(),
            site_id: issuer.identity.public().to_z32(),
            alpn: ALPN.to_vec(),
            session_id: challenge.session_id,
            session_grant_hash: match self {
                Self::Admit { .. } => [0_u8; 32],
                Self::Resume { session_grant, .. } => session_grant_hash(session_grant),
            },
            endpoint_id: *connection.remote_id().as_bytes(),
            challenge_id: challenge.challenge_id,
            nonce: challenge.nonce,
            expires_at_unix: challenge.expires_at_unix,
        }
    }

    fn finalize(
        &self,
        registry: &CapabilityRegistry,
        endpoint_id: EndpointId,
        now_unix: i64,
    ) -> Result<AuthorizedSession, DenialCode> {
        match self {
            Self::Admit {
                invite_id,
                capability,
                session_id,
                session_public_key,
            } => registry.admit(
                *invite_id,
                capability,
                *session_id,
                *session_public_key,
                endpoint_id,
                now_unix,
            ),
            Self::Resume { grant, .. } => registry.resume(grant, endpoint_id),
        }
    }
}

async fn serve_request(
    registry: CapabilityRegistry,
    source: SiteSource,
    session: AuthorizedSession,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) -> Result<()> {
    if !registry.session_is_active(session) {
        write_response_head(&mut send, 401, None, 0, Vec::new()).await?;
        send.finish()?;
        return Ok(());
    }
    let request: SiteRequest = read_frame(&mut recv).await.context("read site request")?;
    if request.body_length > MAX_REQUEST_BODY_BYTES {
        write_response_head(&mut send, 413, None, 0, Vec::new()).await?;
        send.finish()?;
        return Ok(());
    }
    let mut body = vec![0_u8; request.body_length as usize];
    tokio::io::AsyncReadExt::read_exact(&mut recv, &mut body).await?;
    match source {
        SiteSource::Static(site) => serve_static_request(site, request, send).await,
        SiteSource::Loopback(site) if request.method == RequestMethod::WebSocket => {
            serve_loopback_socket(registry, session, site, request, send, recv).await
        }
        SiteSource::Loopback(site) => serve_loopback_http(site, request, body, send).await,
    }
}

async fn serve_static_request(
    site: StaticSite,
    request: SiteRequest,
    mut send: iroh::endpoint::SendStream,
) -> Result<()> {
    if !matches!(request.method, RequestMethod::Get | RequestMethod::Head) {
        write_response_head(&mut send, 405, None, 0, Vec::new()).await?;
        send.finish()?;
        return Ok(());
    }
    let resolved = match site.resolve(&request.path).await {
        Ok(path) => path,
        Err(ResolveError::Invalid | ResolveError::OutsideRoot) => {
            write_response_head(&mut send, 403, None, 0, Vec::new()).await?;
            send.finish()?;
            return Ok(());
        }
        Err(ResolveError::NotFound) => {
            write_response_head(&mut send, 404, None, 0, Vec::new()).await?;
            send.finish()?;
            return Ok(());
        }
        Err(ResolveError::Io(error)) => return Err(error).context("resolve site path"),
    };
    let metadata = tokio::fs::metadata(&resolved).await?;
    let content_type = mime_guess::from_path(&resolved)
        .first_raw()
        .unwrap_or("application/octet-stream")
        .to_string();
    write_response_head(
        &mut send,
        200,
        Some(content_type),
        metadata.len(),
        Vec::new(),
    )
    .await?;
    if request.method == RequestMethod::Get {
        let mut file = tokio::fs::File::open(resolved).await?;
        tokio::io::copy(&mut file, &mut send).await?;
    }
    send.finish()?;
    Ok(())
}

async fn serve_loopback_http(
    site: LoopbackSite,
    request: SiteRequest,
    body: Vec<u8>,
    mut send: iroh::endpoint::SendStream,
) -> Result<()> {
    let url = upstream_url(&site.http_origin, &request.path)?;
    let method = match request.method {
        RequestMethod::Get => reqwest::Method::GET,
        RequestMethod::Head => reqwest::Method::HEAD,
        RequestMethod::Post => reqwest::Method::POST,
        RequestMethod::Put => reqwest::Method::PUT,
        RequestMethod::Patch => reqwest::Method::PATCH,
        RequestMethod::Delete => reqwest::Method::DELETE,
        RequestMethod::Options => reqwest::Method::OPTIONS,
        RequestMethod::WebSocket => unreachable!(),
    };
    let mut upstream = site.client.request(method, url).body(body);
    for header in request.headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(header.name.as_bytes()),
            reqwest::header::HeaderValue::from_str(&header.value),
        ) && !is_hop_by_hop(name.as_str())
        {
            upstream = upstream.header(name, value);
        }
    }
    let response = match upstream.send().await {
        Ok(response) => response,
        Err(_) => {
            write_response_head(&mut send, 502, None, 0, Vec::new()).await?;
            send.finish()?;
            return Ok(());
        }
    };
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let headers = response
        .headers()
        .iter()
        .filter(|(name, _)| {
            !is_hop_by_hop(name.as_str()) && *name != reqwest::header::CONTENT_LENGTH
        })
        .filter_map(|(name, value)| {
            value.to_str().ok().map(|value| Header {
                name: name.as_str().to_owned(),
                value: value.to_owned(),
            })
        })
        .collect();
    let mut chunks = response.bytes_stream();
    let mut bytes = Vec::new();
    let mut too_large = false;
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.context("read loopback response")?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BODY_BYTES {
            too_large = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    if too_large {
        write_response_head(&mut send, 502, None, 0, Vec::new()).await?;
    } else {
        write_response_head(&mut send, status, content_type, bytes.len() as u64, headers).await?;
        if request.method != RequestMethod::Head {
            tokio::io::AsyncWriteExt::write_all(&mut send, &bytes).await?;
        }
    }
    send.finish()?;
    Ok(())
}

async fn serve_loopback_socket(
    registry: CapabilityRegistry,
    session: AuthorizedSession,
    site: LoopbackSite,
    request: SiteRequest,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) -> Result<()> {
    let url = upstream_url(&site.ws_origin, &request.path)?;
    let (socket, _) = match tokio_tungstenite::connect_async(url.as_str()).await {
        Ok(socket) => socket,
        Err(_) => {
            write_response_head(&mut send, 502, None, 0, Vec::new()).await?;
            send.finish()?;
            return Ok(());
        }
    };
    write_response_head(&mut send, 101, None, 0, Vec::new()).await?;
    let (mut socket_send, mut socket_recv) = socket.split();
    let mut authorization_check = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = authorization_check.tick() => {
                if !registry.session_is_active(session) {
                    let close = SocketMessage::Close {
                        code: Some(1008),
                        reason: "Session revoked by host".into(),
                    };
                    let _ = write_frame(&mut send, &close).await;
                    let _ = socket_send.send(to_tungstenite(close)).await;
                    break;
                }
            }
            from_browser = read_frame::<_, SocketMessage>(&mut recv) => {
                let Ok(message) = from_browser else { break };
                socket_send.send(to_tungstenite(message)).await?;
            }
            from_upstream = socket_recv.next() => {
                let Some(message) = from_upstream else { break };
                let message = message?;
                let Some(message) = from_tungstenite(message) else { continue };
                let close = matches!(message, SocketMessage::Close { .. });
                write_frame(&mut send, &message).await?;
                if close { break; }
            }
        }
    }
    let _ = send.finish();
    Ok(())
}

fn upstream_url(origin: &Url, path: &str) -> Result<Url> {
    if !path.starts_with('/') || path.starts_with("//") || path.contains(['\r', '\n', '\\']) {
        anyhow::bail!("invalid upstream path");
    }
    origin.join(path).context("build upstream URL")
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

fn to_tungstenite(message: SocketMessage) -> Message {
    match message {
        SocketMessage::Text(text) => Message::Text(text.into()),
        SocketMessage::Binary(bytes) => Message::Binary(bytes.into()),
        SocketMessage::Ping(bytes) => Message::Ping(bytes.into()),
        SocketMessage::Pong(bytes) => Message::Pong(bytes.into()),
        SocketMessage::Close { code, reason } => {
            Message::Close(code.map(
                |code| tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code: code.into(),
                    reason: reason.into(),
                },
            ))
        }
    }
}

fn from_tungstenite(message: Message) -> Option<SocketMessage> {
    match message {
        Message::Text(text) => Some(SocketMessage::Text(text.to_string())),
        Message::Binary(bytes) => Some(SocketMessage::Binary(bytes.to_vec())),
        Message::Ping(bytes) => Some(SocketMessage::Ping(bytes.to_vec())),
        Message::Pong(bytes) => Some(SocketMessage::Pong(bytes.to_vec())),
        Message::Close(frame) => Some(SocketMessage::Close {
            code: frame.as_ref().map(|frame| frame.code.into()),
            reason: frame.map_or_else(String::new, |frame| frame.reason.to_string()),
        }),
        Message::Frame(_) => None,
    }
}

async fn write_response_head(
    send: &mut iroh::endpoint::SendStream,
    status: u16,
    content_type: Option<String>,
    content_length: u64,
    headers: Vec<Header>,
) -> io::Result<()> {
    write_frame(
        send,
        &SiteResponseHead {
            status,
            content_type,
            content_length,
            headers,
        },
    )
    .await
}

pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
    use iroh_tickets::endpoint::EndpointTicket;
    use urspace_protocol::{
        ALPN, ClientAuthV4, ClientProofV4, SESSION_PROOF_VERSION, ServerAuthV4, ServerHelloV4,
        SessionGrantPayload, SessionProofPayload, SessionProofPurpose, sign_session_proof,
        verify_session_grant,
    };

    fn grant(
        host: &SecretKey,
        invite_id: Uuid,
        session_id: Uuid,
        session_public_key: [u8; 32],
    ) -> SessionGrantPayload {
        SessionGrantPayload {
            version: urspace_protocol::SESSION_GRANT_VERSION,
            host_id: *host.public().as_bytes(),
            site_id: host.public().to_z32(),
            bootstrap_origin: "https://sites.example".into(),
            endpoint_ticket: EndpointTicket::new(iroh::EndpointAddr::new(host.public()))
                .to_string(),
            invite_id,
            session_id,
            session_public_key,
            issued_at_unix: 100,
            expires_at_unix: 1_000,
            authorization_epoch: 0,
            entry_path: "/".into(),
        }
    }

    async fn authorize_v4(
        connection: &Connection,
        endpoint_id: EndpointId,
        host: &SecretKey,
        session_key: &SecretKey,
        auth: ClientAuthV4,
        resume_grant: Option<&str>,
    ) -> String {
        let (mut send, mut recv) = connection.open_bi().await.unwrap();
        write_frame(&mut send, &auth).await.unwrap();
        let ServerAuthV4::Challenge(challenge) = read_frame(&mut recv).await.unwrap() else {
            panic!("host denied valid authorization");
        };
        let proof = SessionProofPayload {
            version: SESSION_PROOF_VERSION,
            purpose: if resume_grant.is_some() {
                SessionProofPurpose::Resume
            } else {
                SessionProofPurpose::Admit
            },
            host_id: *host.public().as_bytes(),
            site_id: host.public().to_z32(),
            alpn: ALPN.to_vec(),
            session_id: challenge.session_id,
            session_grant_hash: resume_grant.map(session_grant_hash).unwrap_or([0_u8; 32]),
            endpoint_id: *endpoint_id.as_bytes(),
            challenge_id: challenge.challenge_id,
            nonce: challenge.nonce,
            expires_at_unix: challenge.expires_at_unix,
        };
        write_frame(
            &mut send,
            &ClientProofV4 {
                signature: sign_session_proof(session_key, &proof).unwrap(),
            },
        )
        .await
        .unwrap();
        send.finish().unwrap();
        let ServerHelloV4::Granted { session_grant } = read_frame(&mut recv).await.unwrap() else {
            panic!("host rejected valid session proof");
        };
        session_grant
    }

    #[test]
    fn registry_enforces_admission_controls_without_expiring_active_sessions() {
        let registry = CapabilityRegistry::default();
        let id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let capability = [9_u8; 32];
        let host = SecretKey::generate();
        let session_key = SecretKey::generate();
        let endpoint_a = SecretKey::generate().public();
        let endpoint_b = SecretKey::generate().public();
        registry.insert(id, &capability, 200, 1);

        assert_eq!(
            registry.can_admit(id, &[8_u8; 32], 100),
            Err(DenialCode::Invalid)
        );
        let session = registry
            .admit(
                id,
                &capability,
                session_id,
                *session_key.public().as_bytes(),
                endpoint_a,
                100,
            )
            .unwrap();
        assert!(registry.session_is_active(session));
        let issued = grant(&host, id, session_id, *session_key.public().as_bytes());
        let resumed = registry.resume(&issued, endpoint_b).unwrap();
        assert!(registry.session_is_active(resumed));
        assert!(!registry.session_is_active(session));
        assert_eq!(
            registry.can_admit(id, &capability, 100),
            Err(DenialCode::SessionLimit)
        );
        assert_eq!(
            registry.can_admit(id, &capability, 200),
            Err(DenialCode::Expired)
        );
        assert!(registry.revoke(id));
        assert!(!registry.session_is_active(resumed));

        let revoked_id = Uuid::new_v4();
        registry.insert(revoked_id, &capability, 200, 2);
        assert!(registry.revoke(revoked_id));
        assert_eq!(
            registry.can_admit(revoked_id, &capability, 100),
            Err(DenialCode::Revoked)
        );

        let expired_id = Uuid::new_v4();
        registry.insert(expired_id, &capability, 100, 1);
        assert_eq!(
            registry.can_admit(expired_id, &capability, 100),
            Err(DenialCode::Expired)
        );
    }

    #[test]
    fn concurrent_admission_cannot_oversubscribe_the_final_slot() {
        let registry = CapabilityRegistry::default();
        let invite_id = Uuid::new_v4();
        let capability = [14_u8; 32];
        registry.insert(invite_id, &capability, 200, 1);
        let barrier = Arc::new(std::sync::Barrier::new(3));

        let attempts = [0_u8, 1_u8].map(|seed| {
            let registry = registry.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let session_key = SecretKey::generate();
                let endpoint = SecretKey::generate().public();
                let session_id = Uuid::from_u128(100 + u128::from(seed));
                barrier.wait();
                registry.admit(
                    invite_id,
                    &capability,
                    session_id,
                    *session_key.public().as_bytes(),
                    endpoint,
                    100,
                )
            })
        });
        barrier.wait();
        let results = attempts.map(|attempt| attempt.join().unwrap());

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(DenialCode::SessionLimit)))
                .count(),
            1
        );
        assert_eq!(registry.sessions().len(), 1);
    }

    #[test]
    fn rotation_preserves_admitted_sessions_and_closes_new_admissions() {
        let registry = CapabilityRegistry::default();
        let id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let capability = [10_u8; 32];
        let host = SecretKey::generate();
        let session_key = SecretKey::generate();
        let endpoint_a = SecretKey::generate().public();
        let endpoint_b = SecretKey::generate().public();
        registry.insert(id, &capability, 200, 2);

        registry
            .admit(
                id,
                &capability,
                session_id,
                *session_key.public().as_bytes(),
                endpoint_a,
                100,
            )
            .unwrap();
        assert!(registry.close_admissions(id));
        let issued = grant(&host, id, session_id, *session_key.public().as_bytes());
        assert!(registry.resume(&issued, endpoint_b).is_ok());
        assert_eq!(
            registry.can_admit(id, &capability, 100),
            Err(DenialCode::Revoked)
        );
        assert_eq!(
            registry.sessions(),
            vec![SessionInfo {
                session_id,
                endpoint_id: endpoint_b,
                connected: false,
            }]
        );
    }

    #[test]
    fn kicked_sessions_cannot_reconnect() {
        let registry = CapabilityRegistry::default();
        let id = Uuid::new_v4();
        let capability = [12_u8; 32];
        let host = SecretKey::generate();
        let session_a = SecretKey::generate();
        let session_b = SecretKey::generate();
        let session_a_id = Uuid::new_v4();
        let session_b_id = Uuid::new_v4();
        let endpoint_a = SecretKey::generate().public();
        let endpoint_b = SecretKey::generate().public();
        registry.insert(id, &capability, 200, 2);

        registry
            .admit(
                id,
                &capability,
                session_a_id,
                *session_a.public().as_bytes(),
                endpoint_a,
                100,
            )
            .unwrap();
        registry
            .admit(
                id,
                &capability,
                session_b_id,
                *session_b.public().as_bytes(),
                endpoint_b,
                100,
            )
            .unwrap();
        assert!(registry.kick(session_a_id));
        let issued = grant(&host, id, session_a_id, *session_a.public().as_bytes());
        assert_eq!(
            registry.resume(&issued, endpoint_a),
            Err(DenialCode::Revoked)
        );
        assert_eq!(registry.sessions().len(), 1);
        assert_eq!(registry.kick_all(), 1);
        assert!(registry.sessions().is_empty());
        let issued = grant(&host, id, session_b_id, *session_b.public().as_bytes());
        assert_eq!(
            registry.resume(&issued, endpoint_b),
            Err(DenialCode::Revoked)
        );
    }

    #[tokio::test]
    async fn static_site_confines_paths() {
        let temp = tempfile::tempdir().unwrap();
        tokio::fs::write(temp.path().join("index.html"), "hello")
            .await
            .unwrap();
        let site = StaticSite::open(temp.path()).await.unwrap();
        assert!(site.resolve("/").await.unwrap().ends_with("index.html"));
        assert!(matches!(
            site.resolve("/%2e%2e/secret").await,
            Err(ResolveError::OutsideRoot)
        ));
        assert!(matches!(
            site.resolve("/missing").await,
            Err(ResolveError::NotFound)
        ));
    }

    #[test]
    fn loopback_proxy_rejects_remote_or_ambiguous_origins() {
        assert!(LoopbackSite::open("http://127.0.0.1:8787").is_ok());
        assert!(LoopbackSite::open("http://[::1]:8787").is_ok());
        assert!(LoopbackSite::open("http://localhost:8787").is_ok());
        assert!(LoopbackSite::open("https://127.0.0.1:8787").is_err());
        assert!(LoopbackSite::open("http://example.com:8787").is_err());
        assert!(LoopbackSite::open("http://127.0.0.1:8787/api").is_err());
    }

    #[test]
    fn upstream_paths_cannot_replace_the_loopback_authority() {
        let origin = Url::parse("http://127.0.0.1:8787/").unwrap();
        assert_eq!(
            upstream_url(&origin, "/api/health?full=1")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:8787/api/health?full=1"
        );
        assert!(upstream_url(&origin, "//attacker.example/path").is_err());
        assert!(upstream_url(&origin, "/safe\\evil").is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn static_site_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        tokio::fs::write(outside.path().join("secret"), "nope")
            .await
            .unwrap();
        symlink(outside.path(), root.path().join("escape")).unwrap();
        let site = StaticSite::open(root.path()).await.unwrap();
        assert!(matches!(
            site.resolve("/escape/secret").await,
            Err(ResolveError::OutsideRoot)
        ));
    }

    #[tokio::test]
    async fn serves_a_file_over_an_authenticated_iroh_connection() {
        let root = tempfile::tempdir().unwrap();
        tokio::fs::write(root.path().join("index.html"), "mesh works")
            .await
            .unwrap();
        let site = StaticSite::open(root.path()).await.unwrap();
        let registry = CapabilityRegistry::default();
        let invite_id = Uuid::new_v4();
        let capability = [11_u8; 32];
        registry.insert(invite_id, &capability, unix_now() + 60, 1);

        let identity = SecretKey::generate();
        let server = Endpoint::builder(presets::Minimal)
            .secret_key(identity.clone())
            .bind()
            .await
            .unwrap();
        let server_addr = server.addr();
        let endpoint_ticket = EndpointTicket::new(server_addr.clone()).to_string();
        let issuer = SessionGrantIssuer::new(
            identity.clone(),
            "https://sites.example".into(),
            endpoint_ticket,
            "/".into(),
        );
        let router = Router::builder(server)
            .accept(ALPN, SiteProtocol::new(registry.clone(), site, issuer))
            .spawn();
        let client = Endpoint::bind(presets::Minimal).await.unwrap();
        let connection = client.connect(server_addr, ALPN).await.unwrap();
        let session_key = SecretKey::generate();

        let (mut hello_send, mut hello_recv) = connection.open_bi().await.unwrap();
        write_frame(
            &mut hello_send,
            &ClientAuthV4::Admit {
                invite_id,
                capability,
                session_public_key: *session_key.public().as_bytes(),
            },
        )
        .await
        .unwrap();
        let ServerAuthV4::Challenge(challenge) = read_frame(&mut hello_recv).await.unwrap() else {
            panic!("host denied valid admission");
        };
        let proof = SessionProofPayload {
            version: SESSION_PROOF_VERSION,
            purpose: SessionProofPurpose::Admit,
            host_id: *identity.public().as_bytes(),
            site_id: identity.public().to_z32(),
            alpn: ALPN.to_vec(),
            session_id: challenge.session_id,
            session_grant_hash: [0_u8; 32],
            endpoint_id: *client.id().as_bytes(),
            challenge_id: challenge.challenge_id,
            nonce: challenge.nonce,
            expires_at_unix: challenge.expires_at_unix,
        };
        write_frame(
            &mut hello_send,
            &ClientProofV4 {
                signature: sign_session_proof(&session_key, &proof).unwrap(),
            },
        )
        .await
        .unwrap();
        hello_send.finish().unwrap();
        let ServerHelloV4::Granted { session_grant } = read_frame(&mut hello_recv).await.unwrap()
        else {
            panic!("host rejected valid session proof");
        };
        let admitted = verify_session_grant(&session_grant, unix_now()).unwrap();

        let (mut request_send, mut response_recv) = connection.open_bi().await.unwrap();
        write_frame(
            &mut request_send,
            &SiteRequest {
                method: RequestMethod::Get,
                path: "/index.html".into(),
                headers: Vec::new(),
                body_length: 0,
            },
        )
        .await
        .unwrap();
        request_send.finish().unwrap();
        let head: SiteResponseHead = read_frame(&mut response_recv).await.unwrap();
        assert_eq!(head.status, 200);
        assert_eq!(head.content_type.as_deref(), Some("text/html"));
        let body = response_recv.read_to_end(1024).await.unwrap();
        assert_eq!(body, b"mesh works");

        assert_eq!(
            registry.sessions(),
            vec![SessionInfo {
                session_id: admitted.session_id,
                endpoint_id: client.id(),
                connected: true,
            }]
        );
        assert!(registry.kick(admitted.session_id));
        tokio::time::timeout(Duration::from_secs(2), connection.closed())
            .await
            .unwrap();

        client.close().await;
        router.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn grant_reconnects_from_a_fresh_endpoint_and_kick_still_wins() {
        let root = tempfile::tempdir().unwrap();
        tokio::fs::write(root.path().join("index.html"), "mesh resumes")
            .await
            .unwrap();
        let site = StaticSite::open(root.path()).await.unwrap();
        let registry = CapabilityRegistry::default();
        let invite_id = Uuid::new_v4();
        let capability = [13_u8; 32];
        registry.insert(invite_id, &capability, unix_now() + 60, 1);

        let identity = SecretKey::generate();
        let server = Endpoint::builder(presets::Minimal)
            .secret_key(identity.clone())
            .bind()
            .await
            .unwrap();
        let server_addr = server.addr();
        let issuer = SessionGrantIssuer::new(
            identity.clone(),
            "https://sites.example".into(),
            EndpointTicket::new(server_addr.clone()).to_string(),
            "/".into(),
        );
        let router = Router::builder(server)
            .accept(ALPN, SiteProtocol::new(registry.clone(), site, issuer))
            .spawn();

        let session_key = SecretKey::generate();
        let first = Endpoint::bind(presets::Minimal).await.unwrap();
        let first_connection = first.connect(server_addr.clone(), ALPN).await.unwrap();
        let first_grant = authorize_v4(
            &first_connection,
            first.id(),
            &identity,
            &session_key,
            ClientAuthV4::Admit {
                invite_id,
                capability,
                session_public_key: *session_key.public().as_bytes(),
            },
            None,
        )
        .await;
        let admitted = verify_session_grant(&first_grant, unix_now()).unwrap();

        let second = Endpoint::bind(presets::Minimal).await.unwrap();
        let second_connection = second.connect(server_addr.clone(), ALPN).await.unwrap();
        let rotated_grant = authorize_v4(
            &second_connection,
            second.id(),
            &identity,
            &session_key,
            ClientAuthV4::Resume {
                session_grant: first_grant.clone(),
            },
            Some(&first_grant),
        )
        .await;
        assert_eq!(
            verify_session_grant(&rotated_grant, unix_now())
                .unwrap()
                .session_id,
            admitted.session_id
        );
        tokio::task::yield_now().await;
        assert_eq!(registry.sessions().len(), 1);
        assert_eq!(registry.sessions()[0].endpoint_id, second.id());
        tokio::time::timeout(Duration::from_secs(2), first_connection.closed())
            .await
            .unwrap();

        assert!(registry.kick(admitted.session_id));
        tokio::time::timeout(Duration::from_secs(2), second_connection.closed())
            .await
            .unwrap();

        let third = Endpoint::bind(presets::Minimal).await.unwrap();
        let third_connection = third.connect(server_addr, ALPN).await.unwrap();
        let (mut send, mut recv) = third_connection.open_bi().await.unwrap();
        write_frame(
            &mut send,
            &ClientAuthV4::Resume {
                session_grant: rotated_grant,
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            read_frame::<_, ServerAuthV4>(&mut recv).await.unwrap(),
            ServerAuthV4::Denied {
                code: DenialCode::Revoked
            }
        ));

        first.close().await;
        second.close().await;
        third.close().await;
        router.shutdown().await.unwrap();
    }
}
