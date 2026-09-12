use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use futures_util::{SinkExt as _, StreamExt as _};
use iroh::EndpointId;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use medousa_site_protocol::{
    ClientHello, DenialCode, Header, RequestMethod, ServerHello, SiteRequest, SiteResponseHead,
    SocketMessage, read_frame, write_frame,
};
use percent_encoding::percent_decode_str;
use reqwest::redirect::Policy;
use subtle::ConstantTimeEq as _;
use tokio::sync::Semaphore;
use tokio_tungstenite::tungstenite::Message;
use url::Url;
use uuid::Uuid;

const MAX_CONCURRENT_REQUESTS_PER_CONNECTION: usize = 64;
const MAX_REQUEST_BODY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
struct InviteRecord {
    capability_hash: [u8; 32],
    expires_at_unix: i64,
    remaining_sessions: u32,
    admitted_endpoints: HashSet<EndpointId>,
    revoked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizedSession {
    invite_id: Uuid,
    endpoint_id: EndpointId,
    invite_expires_at_unix: i64,
}

impl AuthorizedSession {
    pub fn invite_expires_at_unix(self) -> i64 {
        self.invite_expires_at_unix
    }
}

#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    inner: Arc<Mutex<HashMap<Uuid, InviteRecord>>>,
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
            admitted_endpoints: HashSet::new(),
            revoked: false,
        };
        self.inner
            .lock()
            .expect("capability registry poisoned")
            .insert(invite_id, record);
    }

    pub fn authorize(
        &self,
        hello: &ClientHello,
        endpoint_id: EndpointId,
        now_unix: i64,
    ) -> Result<AuthorizedSession, DenialCode> {
        if hello.version != medousa_site_protocol::INVITE_VERSION {
            return Err(DenialCode::Invalid);
        }
        let candidate_hash = capability_hash(&hello.capability);
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        let record = guard.get_mut(&hello.invite_id).ok_or(DenialCode::Invalid)?;
        if record.capability_hash.ct_eq(&candidate_hash).unwrap_u8() != 1 {
            return Err(DenialCode::Invalid);
        }
        if record.revoked {
            return Err(DenialCode::Revoked);
        }
        if record.admitted_endpoints.contains(&endpoint_id) {
            return Ok(AuthorizedSession {
                invite_id: hello.invite_id,
                endpoint_id,
                invite_expires_at_unix: record.expires_at_unix,
            });
        }
        if record.expires_at_unix <= now_unix {
            return Err(DenialCode::Expired);
        }
        if record.remaining_sessions == 0 {
            return Err(DenialCode::SessionLimit);
        }
        record.remaining_sessions -= 1;
        record.admitted_endpoints.insert(endpoint_id);
        Ok(AuthorizedSession {
            invite_id: hello.invite_id,
            endpoint_id,
            invite_expires_at_unix: record.expires_at_unix,
        })
    }

    pub fn session_is_active(&self, session: AuthorizedSession) -> bool {
        self.inner
            .lock()
            .expect("capability registry poisoned")
            .get(&session.invite_id)
            .is_some_and(|record| {
                !record.revoked && record.admitted_endpoints.contains(&session.endpoint_id)
            })
    }

    pub fn revoke(&self, invite_id: Uuid) -> bool {
        let mut guard = self.inner.lock().expect("capability registry poisoned");
        let Some(record) = guard.get_mut(&invite_id) else {
            return false;
        };
        record.revoked = true;
        true
    }
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
}

impl SiteProtocol {
    pub fn new(registry: CapabilityRegistry, site: StaticSite) -> Self {
        Self {
            registry,
            source: SiteSource::Static(site),
        }
    }

    pub fn loopback(registry: CapabilityRegistry, site: LoopbackSite) -> Self {
        Self {
            registry,
            source: SiteSource::Loopback(site),
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
        let hello: ClientHello = read_frame(&mut hello_recv)
            .await
            .context("read client authorization")?;
        let now = unix_now();
        let authorization = self.registry.authorize(&hello, connection.remote_id(), now);
        let reply = match authorization {
            Ok(session) => ServerHello::Granted {
                expires_at_unix: session.invite_expires_at_unix(),
            },
            Err(code) => ServerHello::Denied { code },
        };
        write_frame(&mut hello_send, &reply)
            .await
            .context("write authorization response")?;
        hello_send
            .finish()
            .context("finish authorization response")?;
        let Ok(session) = authorization else {
            return Ok(());
        };

        let limit = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS_PER_CONNECTION));
        loop {
            let Ok((send, recv)) = connection.accept_bi().await else {
                break;
            };
            let permit = Arc::clone(&limit)
                .acquire_owned()
                .await
                .context("request limit closed")?;
            let registry = self.registry.clone();
            let source = self.source.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = serve_request(registry, source, session, send, recv).await;
            });
        }
        Ok(())
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
    use iroh::{Endpoint, SecretKey, endpoint::presets, protocol::Router};
    use medousa_site_protocol::{ALPN, ServerHello};

    fn hello(id: Uuid, capability: [u8; 32]) -> ClientHello {
        ClientHello {
            version: medousa_site_protocol::INVITE_VERSION,
            invite_id: id,
            capability,
        }
    }

    #[test]
    fn registry_enforces_admission_controls_without_expiring_active_sessions() {
        let registry = CapabilityRegistry::default();
        let id = Uuid::new_v4();
        let capability = [9_u8; 32];
        let endpoint_a = SecretKey::generate().public();
        let endpoint_b = SecretKey::generate().public();
        registry.insert(id, &capability, 200, 1);

        assert_eq!(
            registry.authorize(&hello(id, [8_u8; 32]), endpoint_a, 100),
            Err(DenialCode::Invalid)
        );
        let session = registry
            .authorize(&hello(id, capability), endpoint_a, 100)
            .unwrap();
        assert_eq!(session.invite_expires_at_unix(), 200);
        assert!(registry.session_is_active(session));
        assert_eq!(
            registry.authorize(&hello(id, capability), endpoint_a, 200),
            Ok(session)
        );
        assert_eq!(
            registry.authorize(&hello(id, capability), endpoint_b, 100),
            Err(DenialCode::SessionLimit)
        );
        assert_eq!(
            registry.authorize(&hello(id, capability), endpoint_b, 200),
            Err(DenialCode::Expired)
        );
        assert!(registry.session_is_active(session));
        assert!(registry.revoke(id));
        assert!(!registry.session_is_active(session));

        let revoked_id = Uuid::new_v4();
        registry.insert(revoked_id, &capability, 200, 2);
        assert!(registry.revoke(revoked_id));
        assert_eq!(
            registry.authorize(&hello(revoked_id, capability), endpoint_a, 100),
            Err(DenialCode::Revoked)
        );

        let expired_id = Uuid::new_v4();
        registry.insert(expired_id, &capability, 100, 1);
        assert_eq!(
            registry.authorize(&hello(expired_id, capability), endpoint_a, 100),
            Err(DenialCode::Expired)
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
            .secret_key(identity)
            .bind()
            .await
            .unwrap();
        let server_addr = server.addr();
        let router = Router::builder(server)
            .accept(ALPN, SiteProtocol::new(registry, site))
            .spawn();
        let client = Endpoint::bind(presets::Minimal).await.unwrap();
        let connection = client.connect(server_addr, ALPN).await.unwrap();

        let (mut hello_send, mut hello_recv) = connection.open_bi().await.unwrap();
        write_frame(&mut hello_send, &hello(invite_id, capability))
            .await
            .unwrap();
        hello_send.finish().unwrap();
        assert!(matches!(
            read_frame::<_, ServerHello>(&mut hello_recv).await.unwrap(),
            ServerHello::Granted { .. }
        ));

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

        client.close().await;
        router.shutdown().await.unwrap();
    }
}
