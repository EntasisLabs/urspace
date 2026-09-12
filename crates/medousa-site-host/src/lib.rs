use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use medousa_site_protocol::{
    ClientHello, DenialCode, RequestMethod, ServerHello, SiteRequest, SiteResponseHead, read_frame,
    write_frame,
};
use percent_encoding::percent_decode_str;
use subtle::ConstantTimeEq as _;
use tokio::sync::Semaphore;
use uuid::Uuid;

const MAX_CONCURRENT_REQUESTS_PER_CONNECTION: usize = 64;

#[derive(Debug, Clone)]
struct InviteRecord {
    capability_hash: [u8; 32],
    expires_at_unix: i64,
    remaining_sessions: u32,
    revoked: bool,
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
            revoked: false,
        };
        self.inner
            .lock()
            .expect("capability registry poisoned")
            .insert(invite_id, record);
    }

    pub fn authorize(&self, hello: &ClientHello, now_unix: i64) -> Result<i64, DenialCode> {
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
        if record.expires_at_unix <= now_unix {
            return Err(DenialCode::Expired);
        }
        if record.remaining_sessions == 0 {
            return Err(DenialCode::SessionLimit);
        }
        record.remaining_sessions -= 1;
        Ok(record.expires_at_unix)
    }

    pub fn session_is_active(&self, invite_id: Uuid, now_unix: i64) -> bool {
        self.inner
            .lock()
            .expect("capability registry poisoned")
            .get(&invite_id)
            .is_some_and(|record| !record.revoked && record.expires_at_unix > now_unix)
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
    site: StaticSite,
}

impl SiteProtocol {
    pub fn new(registry: CapabilityRegistry, site: StaticSite) -> Self {
        Self { registry, site }
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
        let authorization = self.registry.authorize(&hello, now);
        let reply = match authorization {
            Ok(expires_at_unix) => ServerHello::Granted { expires_at_unix },
            Err(code) => ServerHello::Denied { code },
        };
        write_frame(&mut hello_send, &reply)
            .await
            .context("write authorization response")?;
        hello_send
            .finish()
            .context("finish authorization response")?;
        if authorization.is_err() {
            return Ok(());
        }

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
            let site = self.site.clone();
            let invite_id = hello.invite_id;
            tokio::spawn(async move {
                let _permit = permit;
                let _ = serve_request(registry, site, invite_id, send, recv).await;
            });
        }
        Ok(())
    }
}

async fn serve_request(
    registry: CapabilityRegistry,
    site: StaticSite,
    invite_id: Uuid,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) -> Result<()> {
    if !registry.session_is_active(invite_id, unix_now()) {
        write_response_head(&mut send, 401, None, 0).await?;
        send.finish()?;
        return Ok(());
    }
    let request: SiteRequest = read_frame(&mut recv).await.context("read site request")?;
    let resolved = match site.resolve(&request.path).await {
        Ok(path) => path,
        Err(ResolveError::Invalid | ResolveError::OutsideRoot) => {
            write_response_head(&mut send, 403, None, 0).await?;
            send.finish()?;
            return Ok(());
        }
        Err(ResolveError::NotFound) => {
            write_response_head(&mut send, 404, None, 0).await?;
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
    write_response_head(&mut send, 200, Some(content_type), metadata.len()).await?;
    if request.method == RequestMethod::Get {
        let mut file = tokio::fs::File::open(resolved).await?;
        tokio::io::copy(&mut file, &mut send).await?;
    }
    send.finish()?;
    Ok(())
}

async fn write_response_head(
    send: &mut iroh::endpoint::SendStream,
    status: u16,
    content_type: Option<String>,
    content_length: u64,
) -> io::Result<()> {
    write_frame(
        send,
        &SiteResponseHead {
            status,
            content_type,
            content_length,
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
            version: 1,
            invite_id: id,
            capability,
        }
    }

    #[test]
    fn registry_enforces_secret_expiry_revocation_and_session_limit() {
        let registry = CapabilityRegistry::default();
        let id = Uuid::new_v4();
        let capability = [9_u8; 32];
        registry.insert(id, &capability, 200, 1);

        assert_eq!(
            registry.authorize(&hello(id, [8_u8; 32]), 100),
            Err(DenialCode::Invalid)
        );
        assert_eq!(registry.authorize(&hello(id, capability), 100), Ok(200));
        assert_eq!(
            registry.authorize(&hello(id, capability), 100),
            Err(DenialCode::SessionLimit)
        );

        let revoked_id = Uuid::new_v4();
        registry.insert(revoked_id, &capability, 200, 2);
        assert!(registry.revoke(revoked_id));
        assert_eq!(
            registry.authorize(&hello(revoked_id, capability), 100),
            Err(DenialCode::Revoked)
        );

        let expired_id = Uuid::new_v4();
        registry.insert(expired_id, &capability, 100, 1);
        assert_eq!(
            registry.authorize(&hello(expired_id, capability), 100),
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
