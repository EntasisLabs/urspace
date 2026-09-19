use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt as _, StreamExt as _};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{EndpointId, SecretKey};
use percent_encoding::percent_decode_str;
use rand::Rng as _;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq as _;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio_tungstenite::tungstenite::Message;
use url::Url;
use urspace_protocol::{
    ALPN, ClientAuthV4, ClientProofV4, DenialCode, Header, RequestMethod, ServerAuthV4,
    ServerHelloV4, SessionChallengeV4, SessionGrantIssue, SessionGrantPayload, SessionProofPayload,
    SessionProofPurpose, SiteRequest, SiteResponseHead, SocketMessage, TUNNEL_ALPN, TUNNEL_VERSION,
    TunnelOpenV1, TunnelStatusV1, read_frame, session_grant_hash, sign_session_grant,
    verify_session_grant, verify_session_proof, write_frame,
};
use uuid::Uuid;

pub mod embed;
pub mod native_client;

const MAX_CONCURRENT_REQUESTS_PER_CONNECTION: usize = 64;
const MAX_REQUEST_BODY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;
const SESSION_CHALLENGE_TTL_SECONDS: i64 = 15;
const SESSION_GRANT_EXPIRY_UNIX: i64 = i64::MAX;
const MAX_JOURNAL_EVENT_BYTES: usize = 8 * 1024;

#[derive(Debug)]
pub enum RegistryError {
    Denied(DenialCode),
    Persistence(io::Error),
}

impl RegistryError {
    fn denial_code(&self) -> DenialCode {
        match self {
            Self::Denied(code) => *code,
            Self::Persistence(_) => DenialCode::Invalid,
        }
    }
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(code) => write!(formatter, "authorization denied: {code:?}"),
            Self::Persistence(error) => write!(formatter, "persist authorization state: {error}"),
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Denied(_) => None,
            Self::Persistence(error) => Some(error),
        }
    }
}

impl From<DenialCode> for RegistryError {
    fn from(code: DenialCode) -> Self {
        Self::Denied(code)
    }
}

impl From<io::Error> for RegistryError {
    fn from(error: io::Error) -> Self {
        Self::Persistence(error)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum RegistryEvent {
    InviteInserted {
        invite_id: Uuid,
        capability_hash: [u8; 32],
        expires_at_unix: i64,
        max_sessions: u32,
        #[serde(default)]
        allow_tcp: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        access_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subject: Option<[u8; 32]>,
    },
    AdmissionsClosed {
        invite_id: Uuid,
    },
    AllAdmissionsClosed,
    SessionAdmitted {
        invite_id: Uuid,
        session_id: Uuid,
        session_public_key: [u8; 32],
        endpoint_id: [u8; 32],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operator_handle: Option<[u8; 8]>,
    },
    SessionOperatorHandleAssigned {
        session_id: Uuid,
        operator_handle: [u8; 8],
    },
    SessionEndpointUpdated {
        session_id: Uuid,
        endpoint_id: [u8; 32],
    },
    SessionKicked {
        session_id: Uuid,
    },
    SessionKickedAndAdmissionsClosed {
        session_id: Uuid,
        invite_id: Uuid,
    },
    AllSessionsKicked,
    AllSessionsKickedAndAdmissionsClosed {
        invite_id: Uuid,
    },
    InviteRevoked {
        invite_id: Uuid,
    },
}

#[derive(Debug)]
struct RegistryJournal {
    file: Mutex<File>,
}

#[derive(Debug, Clone)]
struct InviteRecord {
    capability_hash: [u8; 32],
    expires_at_unix: i64,
    remaining_sessions: u32,
    allow_tcp: bool,
    access_label: Option<String>,
    subject: Option<[u8; 32]>,
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
    operator_handle: [u8; 8],
    allow_tcp: bool,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: Uuid,
    pub endpoint_id: EndpointId,
    pub operator_handle: String,
    pub connected: bool,
    pub access_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizedSession {
    session_id: Uuid,
    invite_id: Uuid,
    endpoint_id: EndpointId,
}

#[derive(Debug, Clone, Copy)]
struct NewSession {
    session_id: Uuid,
    session_public_key: [u8; 32],
    endpoint_id: EndpointId,
}

impl AuthorizedSession {
    pub fn session_id(self) -> Uuid {
        self.session_id
    }
}

#[derive(Debug, Clone)]
pub struct CapabilityRegistry {
    inner: Arc<Mutex<RegistryState>>,
    journal: Option<Arc<RegistryJournal>>,
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RegistryState::default())),
            journal: None,
        }
    }
}

impl CapabilityRegistry {
    /// Opens a registry backed by a crash-tolerant, append-only journal.
    ///
    /// The journal contains capability hashes, public browser keys, and
    /// revocation state. It never contains invitation capabilities or browser
    /// private keys.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create registry directory {}", parent.display()))?;
        }
        let mut state = RegistryState::default();
        let valid_length = if path.exists() {
            replay_registry_journal(path, &mut state)?
        } else {
            0
        };
        let file = open_private_journal(path, valid_length)?;
        let registry = Self {
            inner: Arc::new(Mutex::new(state)),
            journal: Some(Arc::new(RegistryJournal {
                file: Mutex::new(file),
            })),
        };
        registry.assign_missing_operator_handles()?;
        Ok(registry)
    }

    pub fn insert(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        expires_at_unix: i64,
        max_sessions: u32,
    ) -> Result<(), RegistryError> {
        self.insert_for(invite_id, capability, expires_at_unix, max_sessions, None)
    }

    /// Inserts an invitation whose admitted sessions carry an operator-visible label.
    ///
    /// The label is local administrative metadata, not an authenticated user identity.
    pub fn insert_for(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        expires_at_unix: i64,
        max_sessions: u32,
        access_label: Option<String>,
    ) -> Result<(), RegistryError> {
        self.insert_for_access(
            invite_id,
            capability,
            expires_at_unix,
            max_sessions,
            access_label,
            false,
            None,
        )
    }

    pub fn insert_for_tcp(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        expires_at_unix: i64,
        max_sessions: u32,
        access_label: Option<String>,
    ) -> Result<(), RegistryError> {
        self.insert_for_access(
            invite_id,
            capability,
            expires_at_unix,
            max_sessions,
            access_label,
            true,
            None,
        )
    }

    pub fn insert_subject_bound(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        subject: [u8; 32],
        expires_at_unix: i64,
        max_sessions: u32,
    ) -> Result<(), RegistryError> {
        self.insert_for_access(
            invite_id,
            capability,
            expires_at_unix,
            max_sessions,
            None,
            false,
            Some(subject),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_for_access(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        expires_at_unix: i64,
        max_sessions: u32,
        access_label: Option<String>,
        allow_tcp: bool,
        subject: Option<[u8; 32]>,
    ) -> Result<(), RegistryError> {
        let event = RegistryEvent::InviteInserted {
            invite_id,
            capability_hash: capability_hash(capability),
            expires_at_unix,
            max_sessions,
            allow_tcp,
            access_label,
            subject,
        };
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        if max_sessions == 0 || guard.invites.contains_key(&invite_id) {
            return Err(invalid_registry_event());
        }
        self.append_event(&event)?;
        apply_registry_event(&mut guard, event)?;
        Ok(())
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

    pub fn require_subject(
        &self,
        invite_id: Uuid,
        session_public_key: &[u8; 32],
    ) -> Result<(), DenialCode> {
        let guard = self.inner.lock().expect("capability registry poisoned");
        let record = guard.invites.get(&invite_id).ok_or(DenialCode::Invalid)?;
        if !subject_matches(record, session_public_key) {
            return Err(DenialCode::Invalid);
        }
        Ok(())
    }

    pub fn can_admit_tcp(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        now_unix: i64,
    ) -> Result<(), DenialCode> {
        self.can_admit(invite_id, capability, now_unix)?;
        let guard = self.inner.lock().expect("capability registry poisoned");
        guard
            .invites
            .get(&invite_id)
            .filter(|record| record.allow_tcp)
            .map(|_| ())
            .ok_or(DenialCode::Invalid)
    }

    pub fn admit(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        session_id: Uuid,
        session_public_key: [u8; 32],
        endpoint_id: EndpointId,
        now_unix: i64,
    ) -> Result<AuthorizedSession, RegistryError> {
        self.can_admit_locked(
            invite_id,
            capability,
            NewSession {
                session_id,
                session_public_key,
                endpoint_id,
            },
            now_unix,
            false,
        )
    }

    pub fn admit_tcp(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        session_id: Uuid,
        session_public_key: [u8; 32],
        endpoint_id: EndpointId,
        now_unix: i64,
    ) -> Result<AuthorizedSession, RegistryError> {
        self.can_admit_locked(
            invite_id,
            capability,
            NewSession {
                session_id,
                session_public_key,
                endpoint_id,
            },
            now_unix,
            true,
        )
    }

    fn can_admit_locked(
        &self,
        invite_id: Uuid,
        capability: &[u8; 32],
        session: NewSession,
        now_unix: i64,
        require_tcp: bool,
    ) -> Result<AuthorizedSession, RegistryError> {
        let candidate_hash = capability_hash(capability);
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        if guard.sessions.contains_key(&session.session_id) {
            return Err(DenialCode::Invalid.into());
        }
        let record = guard.invites.get(&invite_id).ok_or(DenialCode::Invalid)?;
        if record.capability_hash.ct_eq(&candidate_hash).unwrap_u8() != 1 {
            return Err(DenialCode::Invalid.into());
        }
        if require_tcp && !record.allow_tcp {
            return Err(DenialCode::Invalid.into());
        }
        if record.revoked || record.admissions_closed {
            return Err(DenialCode::Revoked.into());
        }
        if record.expires_at_unix <= now_unix {
            return Err(DenialCode::Expired.into());
        }
        if record.remaining_sessions == 0 {
            return Err(DenialCode::SessionLimit.into());
        }
        if !subject_matches(record, &session.session_public_key) {
            return Err(DenialCode::Invalid.into());
        }
        let event = RegistryEvent::SessionAdmitted {
            invite_id,
            session_id: session.session_id,
            session_public_key: session.session_public_key,
            endpoint_id: *session.endpoint_id.as_bytes(),
            operator_handle: Some(random_operator_handle(&guard)),
        };
        self.append_event(&event)?;
        apply_registry_event(&mut guard, event)?;
        Ok(AuthorizedSession {
            session_id: session.session_id,
            invite_id,
            endpoint_id: session.endpoint_id,
        })
    }

    pub fn can_resume(
        &self,
        grant: &urspace_protocol::SessionGrantPayload,
    ) -> Result<(), DenialCode> {
        let guard = self.inner.lock().expect("capability registry poisoned");
        validate_grant_record(&guard, grant).map(|_| ())
    }

    pub fn can_resume_tcp(
        &self,
        grant: &urspace_protocol::SessionGrantPayload,
    ) -> Result<(), DenialCode> {
        let guard = self.inner.lock().expect("capability registry poisoned");
        validate_grant_record(&guard, grant)
            .and_then(|record| record.allow_tcp.then_some(()).ok_or(DenialCode::Invalid))
    }

    pub fn resume(
        &self,
        grant: &urspace_protocol::SessionGrantPayload,
        endpoint_id: EndpointId,
    ) -> Result<AuthorizedSession, RegistryError> {
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        validate_grant_record(&guard, grant)?;
        let event = RegistryEvent::SessionEndpointUpdated {
            session_id: grant.session_id,
            endpoint_id: *endpoint_id.as_bytes(),
        };
        self.append_event(&event)?;
        apply_registry_event(&mut guard, event)?;
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

    pub fn close_admissions(&self, invite_id: Uuid) -> Result<bool, RegistryError> {
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        if !guard.invites.contains_key(&invite_id) {
            return Ok(false);
        }
        if guard
            .invites
            .get(&invite_id)
            .is_some_and(|record| record.admissions_closed)
        {
            return Ok(true);
        };
        let event = RegistryEvent::AdmissionsClosed { invite_id };
        self.append_event(&event)?;
        apply_registry_event(&mut guard, event)?;
        Ok(true)
    }

    pub fn close_all_admissions(&self) -> Result<usize, RegistryError> {
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        let count = guard
            .invites
            .values()
            .filter(|record| !record.admissions_closed && !record.revoked)
            .count();
        if count == 0 {
            return Ok(0);
        }
        let event = RegistryEvent::AllAdmissionsClosed;
        self.append_event(&event)?;
        apply_registry_event(&mut guard, event)?;
        Ok(count)
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
                    operator_handle: display_operator_handle(record.operator_handle),
                    connected: guard.active.contains_key(session_id),
                    access_label: guard
                        .invites
                        .get(&record.invite_id)
                        .and_then(|invite| invite.access_label.clone()),
                })
            })
            .collect();
        sessions.sort_by(|left, right| left.operator_handle.cmp(&right.operator_handle));
        sessions
    }

    fn assign_missing_operator_handles(&self) -> Result<(), RegistryError> {
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        let missing: Vec<_> = guard
            .sessions
            .iter()
            .filter_map(|(session_id, record)| {
                (record.operator_handle == [0_u8; 8]).then_some(*session_id)
            })
            .collect();
        for session_id in missing {
            let event = RegistryEvent::SessionOperatorHandleAssigned {
                session_id,
                operator_handle: random_operator_handle(&guard),
            };
            self.append_event(&event)?;
            apply_registry_event(&mut guard, event)?;
        }
        Ok(())
    }

    pub fn kick(&self, session_id: Uuid) -> Result<bool, RegistryError> {
        let connection = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            let Some(record) = guard.sessions.get(&session_id) else {
                return Ok(false);
            };
            if record.kicked {
                return Ok(false);
            }
            let event = RegistryEvent::SessionKicked { session_id };
            self.append_event(&event)?;
            apply_registry_event(&mut guard, event)?;
            guard
                .active
                .remove(&session_id)
                .map(|active| active.connection)
        };
        if let Some(connection) = connection {
            connection.close(0_u8.into(), b"session kicked by host");
        }
        Ok(true)
    }

    pub fn kick_and_close_admissions(
        &self,
        session_id: Uuid,
        invite_id: Uuid,
    ) -> Result<bool, RegistryError> {
        let connection = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            let Some(record) = guard.sessions.get(&session_id) else {
                return Ok(false);
            };
            if record.kicked {
                return Ok(false);
            }
            if !guard.invites.contains_key(&invite_id) {
                return Ok(false);
            }
            let event = RegistryEvent::SessionKickedAndAdmissionsClosed {
                session_id,
                invite_id,
            };
            self.append_event(&event)?;
            apply_registry_event(&mut guard, event)?;
            guard
                .active
                .remove(&session_id)
                .map(|active| active.connection)
        };
        if let Some(connection) = connection {
            connection.close(0_u8.into(), b"session kicked by host");
        }
        Ok(true)
    }

    pub fn kick_all(&self) -> Result<usize, RegistryError> {
        let (count, connections) = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            let count = guard
                .sessions
                .values()
                .filter(|record| !record.kicked)
                .count();
            if count == 0 {
                return Ok(0);
            }
            let event = RegistryEvent::AllSessionsKicked;
            self.append_event(&event)?;
            apply_registry_event(&mut guard, event)?;
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
        Ok(count)
    }

    pub fn kick_all_and_close_admissions(&self, invite_id: Uuid) -> Result<usize, RegistryError> {
        let (count, connections) = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            if !guard.invites.contains_key(&invite_id) {
                return Ok(0);
            }
            let count = guard
                .sessions
                .values()
                .filter(|record| !record.kicked)
                .count();
            if count == 0 {
                return Ok(0);
            }
            let event = RegistryEvent::AllSessionsKickedAndAdmissionsClosed { invite_id };
            self.append_event(&event)?;
            apply_registry_event(&mut guard, event)?;
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
        Ok(count)
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

    pub fn revoke(&self, invite_id: Uuid) -> Result<bool, RegistryError> {
        let connections = {
            let mut guard = self.inner.lock().expect("capability registry poisoned");
            let Some(record) = guard.invites.get(&invite_id) else {
                return Ok(false);
            };
            if record.revoked {
                return Ok(true);
            }
            let event = RegistryEvent::InviteRevoked { invite_id };
            self.append_event(&event)?;
            apply_registry_event(&mut guard, event)?;
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
        Ok(true)
    }

    fn append_event(&self, event: &RegistryEvent) -> io::Result<()> {
        let Some(journal) = &self.journal else {
            return Ok(());
        };
        let mut encoded = serde_json::to_vec(event).map_err(io::Error::other)?;
        if encoded.len() > MAX_JOURNAL_EVENT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "authorization journal event is too large",
            ));
        }
        encoded.push(b'\n');
        let mut file = journal.file.lock().expect("registry journal poisoned");
        file.write_all(&encoded)?;
        file.sync_data()
    }
}

fn replay_registry_journal(path: &Path, state: &mut RegistryState) -> Result<u64> {
    let file = File::open(path)
        .with_context(|| format!("open authorization journal {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut valid_length = 0_u64;
    loop {
        let mut line = Vec::new();
        let bytes = reader
            .read_until(b'\n', &mut line)
            .with_context(|| format!("read authorization journal {}", path.display()))?;
        if bytes == 0 {
            break;
        }
        if line.len() > MAX_JOURNAL_EVENT_BYTES + 1 {
            anyhow::bail!("authorization journal contains an oversized event");
        }
        if !line.ends_with(b"\n") {
            // A power loss may leave the last append incomplete. Only complete,
            // fsynced newline-delimited events are authoritative.
            break;
        }
        line.pop();
        let event: RegistryEvent = serde_json::from_slice(&line)
            .with_context(|| format!("parse authorization journal {}", path.display()))?;
        apply_registry_event(state, event)
            .map_err(anyhow::Error::new)
            .with_context(|| format!("replay authorization journal {}", path.display()))?;
        valid_length = valid_length.saturating_add(bytes as u64);
    }
    Ok(valid_length)
}

fn open_private_journal(path: &Path, valid_length: u64) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true).read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .with_context(|| format!("open authorization journal {}", path.display()))?;
    let actual_length = file.metadata()?.len();
    if actual_length != valid_length {
        file.set_len(valid_length)
            .with_context(|| format!("repair authorization journal {}", path.display()))?;
        file.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, permissions)
            .with_context(|| format!("secure authorization journal {}", path.display()))?;
    }
    Ok(file)
}

fn apply_registry_event(
    state: &mut RegistryState,
    event: RegistryEvent,
) -> Result<(), RegistryError> {
    match event {
        RegistryEvent::InviteInserted {
            invite_id,
            capability_hash,
            expires_at_unix,
            max_sessions,
            allow_tcp,
            access_label,
            subject,
        } => {
            if max_sessions == 0 || state.invites.contains_key(&invite_id) {
                return Err(invalid_registry_event());
            }
            state.invites.insert(
                invite_id,
                InviteRecord {
                    capability_hash,
                    expires_at_unix,
                    remaining_sessions: max_sessions,
                    allow_tcp,
                    access_label,
                    subject,
                    admitted_sessions: HashSet::new(),
                    admissions_closed: false,
                    revoked: false,
                },
            );
        }
        RegistryEvent::AdmissionsClosed { invite_id } => {
            state
                .invites
                .get_mut(&invite_id)
                .ok_or_else(invalid_registry_event)?
                .admissions_closed = true;
        }
        RegistryEvent::AllAdmissionsClosed => {
            for invite in state.invites.values_mut() {
                invite.admissions_closed = true;
            }
        }
        RegistryEvent::SessionAdmitted {
            invite_id,
            session_id,
            session_public_key,
            endpoint_id,
            operator_handle,
        } => {
            if state.sessions.contains_key(&session_id) {
                return Err(invalid_registry_event());
            }
            let operator_handle = operator_handle.unwrap_or([0_u8; 8]);
            if operator_handle != [0_u8; 8]
                && state
                    .sessions
                    .values()
                    .any(|session| session.operator_handle == operator_handle)
            {
                return Err(invalid_registry_event());
            }
            let endpoint_id =
                EndpointId::from_bytes(&endpoint_id).map_err(|_| invalid_registry_event())?;
            let invite = state
                .invites
                .get_mut(&invite_id)
                .ok_or_else(invalid_registry_event)?;
            if invite.remaining_sessions == 0 || invite.revoked || invite.admissions_closed {
                return Err(invalid_registry_event());
            }
            invite.remaining_sessions -= 1;
            let allow_tcp = invite.allow_tcp;
            invite.admitted_sessions.insert(session_id);
            state.sessions.insert(
                session_id,
                SessionRecord {
                    invite_id,
                    session_public_key,
                    authorization_epoch: 0,
                    endpoint_id,
                    operator_handle,
                    allow_tcp,
                    kicked: false,
                },
            );
        }
        RegistryEvent::SessionOperatorHandleAssigned {
            session_id,
            operator_handle,
        } => {
            if operator_handle == [0_u8; 8]
                || state
                    .sessions
                    .values()
                    .any(|session| session.operator_handle == operator_handle)
            {
                return Err(invalid_registry_event());
            }
            let session = state
                .sessions
                .get_mut(&session_id)
                .ok_or_else(invalid_registry_event)?;
            if session.operator_handle != [0_u8; 8] {
                return Err(invalid_registry_event());
            }
            session.operator_handle = operator_handle;
        }
        RegistryEvent::SessionEndpointUpdated {
            session_id,
            endpoint_id,
        } => {
            let endpoint_id =
                EndpointId::from_bytes(&endpoint_id).map_err(|_| invalid_registry_event())?;
            state
                .sessions
                .get_mut(&session_id)
                .ok_or_else(invalid_registry_event)?
                .endpoint_id = endpoint_id;
        }
        RegistryEvent::SessionKicked { session_id } => {
            state
                .sessions
                .get_mut(&session_id)
                .ok_or_else(invalid_registry_event)?
                .kicked = true;
        }
        RegistryEvent::SessionKickedAndAdmissionsClosed {
            session_id,
            invite_id,
        } => {
            if !state.sessions.contains_key(&session_id) || !state.invites.contains_key(&invite_id)
            {
                return Err(invalid_registry_event());
            }
            state
                .sessions
                .get_mut(&session_id)
                .expect("session existence checked")
                .kicked = true;
            state
                .invites
                .get_mut(&invite_id)
                .expect("invite existence checked")
                .admissions_closed = true;
        }
        RegistryEvent::AllSessionsKicked => {
            for session in state.sessions.values_mut() {
                session.kicked = true;
            }
        }
        RegistryEvent::AllSessionsKickedAndAdmissionsClosed { invite_id } => {
            if !state.invites.contains_key(&invite_id) {
                return Err(invalid_registry_event());
            }
            for session in state.sessions.values_mut() {
                session.kicked = true;
            }
            state
                .invites
                .get_mut(&invite_id)
                .expect("invite existence checked")
                .admissions_closed = true;
        }
        RegistryEvent::InviteRevoked { invite_id } => {
            state
                .invites
                .get_mut(&invite_id)
                .ok_or_else(invalid_registry_event)?
                .revoked = true;
        }
    }
    Ok(())
}

fn invalid_registry_event() -> RegistryError {
    RegistryError::Persistence(io::Error::new(
        io::ErrorKind::InvalidData,
        "authorization journal is inconsistent",
    ))
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

fn random_operator_handle(state: &RegistryState) -> [u8; 8] {
    loop {
        let candidate: [u8; 8] = rand::rng().random();
        if candidate != [0_u8; 8]
            && !state
                .sessions
                .values()
                .any(|session| session.operator_handle == candidate)
        {
            return candidate;
        }
    }
}

fn display_operator_handle(handle: [u8; 8]) -> String {
    format!("session-{}", URL_SAFE_NO_PAD.encode(handle))
}

fn subject_matches(record: &InviteRecord, session_public_key: &[u8; 32]) -> bool {
    match record.subject {
        None => true,
        Some(expected) => expected.ct_eq(session_public_key).unwrap_u8() == 1,
    }
}

#[derive(Debug, Clone)]
pub struct StaticSite {
    root: Arc<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LoopbackSite {
    http_origin: Url,
    ws_origin: Url,
    tcp_address: SocketAddr,
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
        let tcp_address = SocketAddr::new(
            http_origin
                .host_str()
                .map(|host| host.trim_matches(['[', ']']))
                .and_then(|host| host.parse::<IpAddr>().ok())
                .context("loopback upstream must resolve to a numeric address")?,
            http_origin
                .port_or_known_default()
                .context("loopback upstream must have a port")?,
        );
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
            tcp_address,
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
    Loopback(Box<LoopbackSite>),
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
pub struct TunnelProtocol {
    authorization: SiteProtocol,
    site: LoopbackSite,
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
            source: SiteSource::Loopback(Box::new(site)),
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
        let Some(session) = self.authorize_connection(&connection, ALPN, false).await? else {
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

    async fn authorize_connection(
        &self,
        connection: &Connection,
        alpn: &[u8],
        require_tcp: bool,
    ) -> Result<Option<AuthorizedSession>> {
        let (mut hello_send, mut hello_recv) = connection
            .accept_bi()
            .await
            .context("accept authorization stream")?;
        let hello: ClientAuthV4 = read_frame(&mut hello_recv)
            .await
            .context("read client authorization")?;
        let now = unix_now();
        let pending = match self.prepare_authorization(hello, now, require_tcp) {
            Ok(pending) => pending,
            Err(code) => {
                write_frame(&mut hello_send, &ServerAuthV4::Denied { code })
                    .await
                    .context("write authorization denial")?;
                hello_send.finish().context("finish authorization denial")?;
                let _ = tokio::time::timeout(Duration::from_secs(1), hello_send.stopped()).await;
                return Ok(None);
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
        let proof_payload = pending.proof_payload(&self.issuer, connection, &challenge, alpn);
        let proof_valid = verify_session_proof(
            pending.session_public_key(),
            &proof_payload,
            &proof.signature,
            unix_now(),
        )
        .is_ok();
        let authorization = if proof_valid {
            pending.finalize(
                &self.registry,
                connection.remote_id(),
                unix_now(),
                require_tcp,
            )
        } else {
            Err(RegistryError::Denied(DenialCode::Invalid))
        };
        let mut granted_session = None;
        let reply = match authorization {
            Ok(session) => match self.issue_grant(&pending, unix_now()) {
                Ok(session_grant) => {
                    granted_session = Some(session);
                    ServerHelloV4::Granted { session_grant }
                }
                Err(_) => {
                    let _ = self.registry.kick(session.session_id());
                    ServerHelloV4::Denied {
                        code: DenialCode::Invalid,
                    }
                }
            },
            Err(error) => {
                if matches!(error, RegistryError::Persistence(_)) {
                    eprintln!("Urspace could not persist an authorization change: {error}");
                }
                ServerHelloV4::Denied {
                    code: error.denial_code(),
                }
            }
        };
        write_frame(&mut hello_send, &reply)
            .await
            .context("write authorization response")?;
        hello_send
            .finish()
            .context("finish authorization response")?;
        let Some(session) = granted_session else {
            let _ = tokio::time::timeout(Duration::from_secs(1), hello_send.stopped()).await;
            return Ok(None);
        };
        Ok(Some(session))
    }

    fn prepare_authorization(
        &self,
        hello: ClientAuthV4,
        now_unix: i64,
        require_tcp: bool,
    ) -> Result<PendingAuthorization, DenialCode> {
        match hello {
            ClientAuthV4::Admit {
                invite_id,
                capability,
                session_public_key,
            } => {
                iroh::PublicKey::from_bytes(&session_public_key)
                    .map_err(|_| DenialCode::Invalid)?;
                if require_tcp {
                    self.registry
                        .can_admit_tcp(invite_id, &capability, now_unix)?;
                } else {
                    self.registry.can_admit(invite_id, &capability, now_unix)?;
                }
                self.registry
                    .require_subject(invite_id, &session_public_key)?;
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
                if require_tcp {
                    self.registry.can_resume_tcp(&grant)?;
                } else {
                    self.registry.can_resume(&grant)?;
                }
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
                // The host registry is authoritative for grant lifetime. Foreground
                // registries disappear on restart; named services restore theirs.
                expires_at_unix: SESSION_GRANT_EXPIRY_UNIX,
                authorization_epoch: pending.authorization_epoch(),
                entry_path: self.issuer.entry_path.clone(),
            },
        )
    }
}

impl TunnelProtocol {
    pub fn new(
        registry: CapabilityRegistry,
        site: LoopbackSite,
        issuer: SessionGrantIssuer,
    ) -> Self {
        Self {
            authorization: SiteProtocol::loopback(registry, site.clone(), issuer),
            site,
        }
    }

    async fn serve_connection(&self, connection: Connection) -> Result<()> {
        let Some(session) = self
            .authorization
            .authorize_connection(&connection, TUNNEL_ALPN, true)
            .await?
        else {
            return Ok(());
        };
        let stable_id = connection.stable_id();
        self.authorization
            .registry
            .session_connected(session, connection.clone());
        let limit = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS_PER_CONNECTION));
        loop {
            let Ok((send, recv)) = connection.accept_bi().await else {
                break;
            };
            let Ok(permit) = Arc::clone(&limit).acquire_owned().await else {
                break;
            };
            let registry = self.authorization.registry.clone();
            let site = self.site.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = serve_tcp_tunnel(registry, site, session, send, recv).await;
            });
        }
        self.authorization
            .registry
            .session_disconnected(session, stable_id);
        Ok(())
    }
}

impl ProtocolHandler for TunnelProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        self.serve_connection(connection)
            .await
            .map_err(|error| AcceptError::from_err(io::Error::other(error.to_string())))
    }
}

async fn serve_tcp_tunnel(
    registry: CapabilityRegistry,
    site: LoopbackSite,
    session: AuthorizedSession,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) -> Result<()> {
    if !registry.session_is_active(session) {
        write_frame(&mut send, &TunnelStatusV1::Denied).await?;
        send.finish()?;
        return Ok(());
    }
    let request: TunnelOpenV1 = read_frame(&mut recv).await.context("read tunnel request")?;
    if request.version != TUNNEL_VERSION {
        write_frame(&mut send, &TunnelStatusV1::Denied).await?;
        send.finish()?;
        return Ok(());
    }
    let upstream = match TcpStream::connect(site.tcp_address).await {
        Ok(upstream) => upstream,
        Err(_) => {
            write_frame(&mut send, &TunnelStatusV1::Denied).await?;
            send.finish()?;
            return Ok(());
        }
    };
    write_frame(&mut send, &TunnelStatusV1::Ready).await?;
    let (mut upstream_read, mut upstream_write) = upstream.into_split();
    let upload = async {
        tokio::io::copy(&mut recv, &mut upstream_write).await?;
        tokio::io::AsyncWriteExt::shutdown(&mut upstream_write).await
    };
    let download = async {
        tokio::io::copy(&mut upstream_read, &mut send).await?;
        send.finish().map_err(io::Error::other)
    };
    tokio::try_join!(upload, download)?;
    Ok(())
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
        alpn: &[u8],
    ) -> SessionProofPayload {
        SessionProofPayload {
            version: urspace_protocol::SESSION_PROOF_VERSION,
            purpose: match self {
                Self::Admit { .. } => SessionProofPurpose::Admit,
                Self::Resume { .. } => SessionProofPurpose::Resume,
            },
            host_id: *issuer.identity.public().as_bytes(),
            site_id: issuer.identity.public().to_z32(),
            alpn: alpn.to_vec(),
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
        require_tcp: bool,
    ) -> Result<AuthorizedSession, RegistryError> {
        match self {
            Self::Admit {
                invite_id,
                capability,
                session_id,
                session_public_key,
            } => {
                if require_tcp {
                    registry.admit_tcp(
                        *invite_id,
                        capability,
                        *session_id,
                        *session_public_key,
                        endpoint_id,
                        now_unix,
                    )
                } else {
                    registry.admit(
                        *invite_id,
                        capability,
                        *session_id,
                        *session_public_key,
                        endpoint_id,
                        now_unix,
                    )
                }
            }
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
            serve_loopback_socket(registry, session, *site, request, send, recv).await
        }
        SiteSource::Loopback(site) => serve_loopback_http(*site, request, body, send).await,
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
        registry.insert(id, &capability, 200, 1).unwrap();

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
        assert!(registry.revoke(id).unwrap());
        assert!(!registry.session_is_active(resumed));

        let revoked_id = Uuid::new_v4();
        registry.insert(revoked_id, &capability, 200, 2).unwrap();
        assert!(registry.revoke(revoked_id).unwrap());
        assert_eq!(
            registry.can_admit(revoked_id, &capability, 100),
            Err(DenialCode::Revoked)
        );

        let expired_id = Uuid::new_v4();
        registry.insert(expired_id, &capability, 100, 1).unwrap();
        assert_eq!(
            registry.can_admit(expired_id, &capability, 100),
            Err(DenialCode::Expired)
        );
    }

    #[test]
    fn tcp_mounts_require_an_explicitly_scoped_invitation() {
        let registry = CapabilityRegistry::default();
        let web_invite = Uuid::new_v4();
        let tcp_invite = Uuid::new_v4();
        let capability = [15_u8; 32];
        registry.insert(web_invite, &capability, 200, 1).unwrap();
        registry
            .insert_for_tcp(tcp_invite, &capability, 200, 1, None)
            .unwrap();

        assert_eq!(
            registry.can_admit_tcp(web_invite, &capability, 100),
            Err(DenialCode::Invalid)
        );
        assert!(registry.can_admit_tcp(tcp_invite, &capability, 100).is_ok());
        let session_id = Uuid::new_v4();
        let session_key = SecretKey::generate();
        let endpoint = SecretKey::generate().public();
        registry
            .admit_tcp(
                tcp_invite,
                &capability,
                session_id,
                *session_key.public().as_bytes(),
                endpoint,
                100,
            )
            .unwrap();
        let host = SecretKey::generate();
        let issued = grant(
            &host,
            tcp_invite,
            session_id,
            *session_key.public().as_bytes(),
        );
        assert!(registry.can_resume_tcp(&issued).is_ok());
    }

    #[test]
    fn subject_bound_invites_reject_a_different_session_key() {
        let registry = CapabilityRegistry::default();
        let invite_id = Uuid::new_v4();
        let capability = [21_u8; 32];
        let bound = SecretKey::generate();
        let other = SecretKey::generate();
        let endpoint = SecretKey::generate().public();
        registry
            .insert_subject_bound(invite_id, &capability, *bound.public().as_bytes(), 200, 1)
            .unwrap();

        assert_eq!(
            registry.require_subject(invite_id, other.public().as_bytes()),
            Err(DenialCode::Invalid)
        );
        assert!(
            registry
                .require_subject(invite_id, bound.public().as_bytes())
                .is_ok()
        );
        assert!(matches!(
            registry.admit(
                invite_id,
                &capability,
                Uuid::new_v4(),
                *other.public().as_bytes(),
                endpoint,
                100,
            ),
            Err(RegistryError::Denied(DenialCode::Invalid))
        ));
        registry
            .admit(
                invite_id,
                &capability,
                Uuid::new_v4(),
                *bound.public().as_bytes(),
                endpoint,
                100,
            )
            .unwrap();
    }

    #[test]
    fn concurrent_admission_cannot_oversubscribe_the_final_slot() {
        let registry = CapabilityRegistry::default();
        let invite_id = Uuid::new_v4();
        let capability = [14_u8; 32];
        registry.insert(invite_id, &capability, 200, 1).unwrap();
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
                .filter(|result| {
                    matches!(result, Err(RegistryError::Denied(DenialCode::SessionLimit)))
                })
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
        registry.insert(id, &capability, 200, 2).unwrap();

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
        let operator_handle = registry.sessions()[0].operator_handle.clone();
        assert!(registry.close_admissions(id).unwrap());
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
                operator_handle,
                connected: false,
                access_label: None,
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
        registry.insert(id, &capability, 200, 2).unwrap();

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
        assert!(registry.kick(session_a_id).unwrap());
        let issued = grant(&host, id, session_a_id, *session_a.public().as_bytes());
        assert!(matches!(
            registry.resume(&issued, endpoint_a),
            Err(RegistryError::Denied(DenialCode::Revoked))
        ));
        assert_eq!(registry.sessions().len(), 1);
        assert_eq!(registry.kick_all().unwrap(), 1);
        assert!(registry.sessions().is_empty());
        let issued = grant(&host, id, session_b_id, *session_b.public().as_bytes());
        assert!(matches!(
            registry.resume(&issued, endpoint_b),
            Err(RegistryError::Denied(DenialCode::Revoked))
        ));
    }

    #[test]
    fn authorization_journal_restores_sessions_and_revocations_without_bearer_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("authorization.jsonl");
        let invite_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let capability = [42_u8; 32];
        let session_key = SecretKey::generate();
        let endpoint = SecretKey::generate().public();
        let host = SecretKey::generate();

        let registry = CapabilityRegistry::open(&journal).unwrap();
        registry.insert(invite_id, &capability, 200, 2).unwrap();
        registry
            .admit(
                invite_id,
                &capability,
                session_id,
                *session_key.public().as_bytes(),
                endpoint,
                100,
            )
            .unwrap();
        registry.close_admissions(invite_id).unwrap();
        let operator_handle = registry.sessions()[0].operator_handle.clone();
        drop(registry);

        let encoded = std::fs::read_to_string(&journal).unwrap();
        assert!(encoded.contains("capability_hash"));
        assert!(!encoded.contains("\"capability\":"));

        let registry = CapabilityRegistry::open(&journal).unwrap();
        assert_eq!(
            registry.sessions(),
            vec![SessionInfo {
                session_id,
                endpoint_id: endpoint,
                operator_handle,
                connected: false,
                access_label: None,
            }]
        );
        assert_eq!(
            registry.can_admit(invite_id, &capability, 100),
            Err(DenialCode::Revoked)
        );
        let issued = grant(
            &host,
            invite_id,
            session_id,
            *session_key.public().as_bytes(),
        );
        assert!(registry.can_resume(&issued).is_ok());
        let current_invite_id = Uuid::new_v4();
        registry
            .insert(current_invite_id, &capability, 200, 1)
            .unwrap();
        registry
            .kick_and_close_admissions(session_id, current_invite_id)
            .unwrap();
        drop(registry);

        let registry = CapabilityRegistry::open(&journal).unwrap();
        assert_eq!(
            registry.can_admit(current_invite_id, &capability, 100),
            Err(DenialCode::Revoked)
        );
        assert!(matches!(
            registry.resume(&issued, endpoint),
            Err(RegistryError::Denied(DenialCode::Revoked))
        ));
    }

    #[test]
    fn access_labels_follow_admitted_sessions_across_restart() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("authorization.jsonl");
        let invite_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let capability = [45_u8; 32];
        let session_key = SecretKey::generate();
        let endpoint = SecretKey::generate().public();

        let registry = CapabilityRegistry::open(&journal).unwrap();
        registry
            .insert_for(
                invite_id,
                &capability,
                200,
                1,
                Some("Alice / work laptop".into()),
            )
            .unwrap();
        registry
            .admit(
                invite_id,
                &capability,
                session_id,
                *session_key.public().as_bytes(),
                endpoint,
                100,
            )
            .unwrap();
        let operator_handle = registry.sessions()[0].operator_handle.clone();
        drop(registry);

        let encoded = std::fs::read_to_string(&journal).unwrap();
        assert!(encoded.contains("Alice / work laptop"));
        assert!(!encoded.contains("\"capability\":"));

        let registry = CapabilityRegistry::open(&journal).unwrap();
        assert_eq!(
            registry.sessions(),
            vec![SessionInfo {
                session_id,
                endpoint_id: endpoint,
                operator_handle,
                connected: false,
                access_label: Some("Alice / work laptop".into()),
            }]
        );
    }

    #[test]
    fn old_journals_receive_stable_random_operator_handles() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("authorization.jsonl");
        let invite_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let capability = [46_u8; 32];
        let session_key = SecretKey::generate();
        let endpoint = SecretKey::generate().public();

        let registry = CapabilityRegistry::open(&journal).unwrap();
        registry.insert(invite_id, &capability, 200, 1).unwrap();
        let legacy_event = RegistryEvent::SessionAdmitted {
            invite_id,
            session_id,
            session_public_key: *session_key.public().as_bytes(),
            endpoint_id: *endpoint.as_bytes(),
            operator_handle: None,
        };
        registry.append_event(&legacy_event).unwrap();
        apply_registry_event(&mut registry.inner.lock().unwrap(), legacy_event).unwrap();
        drop(registry);

        let registry = CapabilityRegistry::open(&journal).unwrap();
        let handle = registry.sessions()[0].operator_handle.clone();
        assert!(handle.starts_with("session-"));
        assert_eq!(handle.len(), 19);
        drop(registry);
        assert!(
            std::fs::read_to_string(&journal)
                .unwrap()
                .contains("session_operator_handle_assigned")
        );

        let reopened = CapabilityRegistry::open(&journal).unwrap();
        assert_eq!(reopened.sessions()[0].operator_handle, handle);
    }

    #[test]
    fn authorization_journal_discards_an_incomplete_final_append() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("authorization.jsonl");
        let invite_id = Uuid::new_v4();
        let capability = [43_u8; 32];
        let registry = CapabilityRegistry::open(&journal).unwrap();
        registry.insert(invite_id, &capability, 200, 1).unwrap();
        drop(registry);

        let valid_length = std::fs::metadata(&journal).unwrap().len();
        let mut file = OpenOptions::new().append(true).open(&journal).unwrap();
        file.write_all(b"{\"event\":\"session_").unwrap();
        file.sync_all().unwrap();
        drop(file);

        let registry = CapabilityRegistry::open(&journal).unwrap();
        assert_eq!(std::fs::metadata(&journal).unwrap().len(), valid_length);
        assert!(registry.can_admit(invite_id, &capability, 100).is_ok());
    }

    #[test]
    fn authorization_journal_rejects_a_tampered_complete_event() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("authorization.jsonl");
        let invite_id = Uuid::new_v4();
        let registry = CapabilityRegistry::open(&journal).unwrap();
        registry.insert(invite_id, &[44_u8; 32], 200, 1).unwrap();
        drop(registry);

        let mut file = OpenOptions::new().append(true).open(&journal).unwrap();
        file.write_all(
            format!(
                "{{\"event\":\"admissions_closed\",\"invite_id\":\"{invite_id}\",\"unexpected\":true}}\n"
            )
            .as_bytes(),
        )
            .unwrap();
        file.sync_all().unwrap();
        drop(file);

        assert!(CapabilityRegistry::open(&journal).is_err());
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
        registry
            .insert(invite_id, &capability, unix_now() + 60, 1)
            .unwrap();

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

        let sessions = registry.sessions();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, admitted.session_id);
        assert_eq!(sessions[0].endpoint_id, client.id());
        assert!(sessions[0].operator_handle.starts_with("session-"));
        assert!(sessions[0].connected);
        assert_eq!(sessions[0].access_label, None);
        assert!(registry.kick(admitted.session_id).unwrap());
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
        registry
            .insert(invite_id, &capability, unix_now() + 60, 1)
            .unwrap();

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

        assert!(registry.kick(admitted.session_id).unwrap());
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

    #[tokio::test]
    async fn persisted_grant_resumes_after_host_restart_with_stable_ticket() {
        let directory = tempfile::tempdir().unwrap();
        let journal = directory.path().join("authorization.jsonl");
        let root = directory.path().join("site");
        tokio::fs::create_dir(&root).await.unwrap();
        tokio::fs::write(root.join("index.html"), "mesh survives restart")
            .await
            .unwrap();

        let identity = SecretKey::generate();
        let stable_ticket =
            EndpointTicket::new(iroh::EndpointAddr::new(identity.public())).to_string();
        let invite_id = Uuid::new_v4();
        let capability = [15_u8; 32];
        let session_key = SecretKey::generate();

        let first_registry = CapabilityRegistry::open(&journal).unwrap();
        first_registry
            .insert(invite_id, &capability, unix_now() + 60, 1)
            .unwrap();
        let first_server = Endpoint::builder(presets::Minimal)
            .secret_key(identity.clone())
            .bind()
            .await
            .unwrap();
        let first_addr = first_server.addr();
        let first_router = Router::builder(first_server)
            .accept(
                ALPN,
                SiteProtocol::new(
                    first_registry.clone(),
                    StaticSite::open(&root).await.unwrap(),
                    SessionGrantIssuer::new(
                        identity.clone(),
                        "https://sites.example".into(),
                        stable_ticket.clone(),
                        "/".into(),
                    ),
                ),
            )
            .spawn();
        let first_client = Endpoint::bind(presets::Minimal).await.unwrap();
        let first_connection = first_client.connect(first_addr, ALPN).await.unwrap();
        let grant = authorize_v4(
            &first_connection,
            first_client.id(),
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
        let admitted = verify_session_grant(&grant, unix_now()).unwrap();
        first_client.close().await;
        first_router.shutdown().await.unwrap();
        drop(first_registry);

        let restarted_registry = CapabilityRegistry::open(&journal).unwrap();
        let restarted_server = Endpoint::builder(presets::Minimal)
            .secret_key(identity.clone())
            .bind()
            .await
            .unwrap();
        let restarted_addr = restarted_server.addr();
        let restarted_router = Router::builder(restarted_server)
            .accept(
                ALPN,
                SiteProtocol::new(
                    restarted_registry.clone(),
                    StaticSite::open(&root).await.unwrap(),
                    SessionGrantIssuer::new(
                        identity.clone(),
                        "https://sites.example".into(),
                        stable_ticket,
                        "/".into(),
                    ),
                ),
            )
            .spawn();
        let restarted_client = Endpoint::bind(presets::Minimal).await.unwrap();
        let restarted_connection = restarted_client
            .connect(restarted_addr, ALPN)
            .await
            .unwrap();
        let refreshed_grant = authorize_v4(
            &restarted_connection,
            restarted_client.id(),
            &identity,
            &session_key,
            ClientAuthV4::Resume {
                session_grant: grant.clone(),
            },
            Some(&grant),
        )
        .await;

        assert_eq!(
            verify_session_grant(&refreshed_grant, unix_now())
                .unwrap()
                .session_id,
            admitted.session_id
        );
        assert_eq!(
            restarted_registry.sessions()[0].session_id,
            admitted.session_id
        );
        assert_eq!(
            restarted_registry.sessions()[0].endpoint_id,
            restarted_client.id()
        );

        restarted_client.close().await;
        restarted_router.shutdown().await.unwrap();
    }
}
