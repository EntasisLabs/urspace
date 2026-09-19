//! Embeddable host handle for Rust applications.
//!
//! The CLI remains the hands-off path. This API is for apps that want to mint
//! bearer URLs and subject-bound invites from their own process.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use iroh::protocol::Router;
use iroh::{Endpoint, SecretKey, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use rand::Rng as _;
use url::Url;
use uuid::Uuid;

use crate::{
    CapabilityRegistry, LoopbackSite, SessionGrantIssuer, SessionInfo, SiteProtocol, unix_now,
};
use urspace_protocol::{
    ALPN, InviteGrant, SubjectInviteIssue, invite_url, sign_invite, sign_subject_invite,
};

const DEFAULT_BOOTSTRAP_ORIGIN: &str = "https://urspace.online";

/// Options for an embedded Urspace host.
#[derive(Debug, Clone)]
pub struct ShareOptions {
    /// Public bootstrap origin signed into bearer invitation URLs.
    pub bootstrap_origin: String,
    /// First path a newly admitted client should open.
    pub entry_path: String,
    /// How long newly minted invitations accept admissions.
    pub invite_ttl: Duration,
    /// Maximum admissions for a newly minted bearer invitation.
    pub max_sessions: u32,
}

impl Default for ShareOptions {
    fn default() -> Self {
        Self {
            bootstrap_origin: DEFAULT_BOOTSTRAP_ORIGIN.to_owned(),
            entry_path: "/".to_owned(),
            invite_ttl: Duration::from_secs(60 * 60),
            max_sessions: 4,
        }
    }
}

/// Options for a subject-bound mint.
#[derive(Debug, Clone)]
pub struct MintOptions {
    /// How long the minted invite may be used to finish admit.
    pub ttl: Duration,
    /// Maximum successful admissions. Use `1` for a single browser key.
    pub max_sessions: u32,
}

impl Default for MintOptions {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(30),
            max_sessions: 1,
        }
    }
}

/// A running Urspace host owned by the embedding application.
pub struct Site {
    endpoint: Endpoint,
    identity: SecretKey,
    registry: CapabilityRegistry,
    router: Router,
    bootstrap_origin: String,
    entry_path: String,
    default_ttl: Duration,
    default_max_sessions: u32,
}

impl Site {
    /// Share a loopback HTTP app using the production Iroh relay preset.
    pub async fn serve(upstream: impl AsRef<str>) -> Result<Self> {
        Self::bind(upstream, ShareOptions::default(), false).await
    }

    /// Share a loopback HTTP app with explicit invitation defaults.
    pub async fn serve_with(upstream: impl AsRef<str>, options: ShareOptions) -> Result<Self> {
        Self::bind(upstream, options, false).await
    }

    /// Share a loopback HTTP app without contacting a public relay.
    ///
    /// Use this in tests or isolated networks. Browser clients that need the
    /// production relay should call [`Site::serve`] instead.
    pub async fn serve_local(upstream: impl AsRef<str>, options: ShareOptions) -> Result<Self> {
        Self::bind(upstream, options, true).await
    }

    async fn bind(
        upstream: impl AsRef<str>,
        options: ShareOptions,
        local_only: bool,
    ) -> Result<Self> {
        if options.max_sessions == 0 {
            bail!("max-sessions must be greater than zero");
        }
        let site = LoopbackSite::open(upstream.as_ref())?;
        ensure_app_is_listening(site.origin()).await?;
        let identity = SecretKey::generate();
        let endpoint = if local_only {
            Endpoint::builder(presets::Minimal)
                .secret_key(identity.clone())
                .bind()
                .await
                .context("bind local Urspace endpoint")?
        } else {
            let endpoint = Endpoint::builder(presets::N0)
                .secret_key(identity.clone())
                .bind()
                .await
                .context("bind Urspace endpoint")?;
            tokio::time::timeout(Duration::from_secs(30), endpoint.online())
                .await
                .context("timed out bringing the Urspace host online")?;
            endpoint
        };
        let registry = CapabilityRegistry::default();
        let issuer = SessionGrantIssuer::new(
            identity.clone(),
            options.bootstrap_origin.clone(),
            EndpointTicket::new(endpoint.addr()).to_string(),
            options.entry_path.clone(),
        );
        let router = Router::builder(endpoint.clone())
            .accept(ALPN, SiteProtocol::loopback(registry.clone(), site, issuer))
            .spawn();
        Ok(Self {
            endpoint,
            identity,
            registry,
            router,
            bootstrap_origin: options.bootstrap_origin,
            entry_path: options.entry_path,
            default_ttl: options.invite_ttl,
            default_max_sessions: options.max_sessions,
        })
    }

    /// Site identity as a z-base-32 public key.
    pub fn site_id(&self) -> String {
        self.identity.public().to_z32()
    }

    /// Mint a bearer invitation URL. Possession of the URL is enough to enroll.
    pub fn bearer_invite(&self) -> Result<Url> {
        self.bearer_invite_with(self.default_ttl, self.default_max_sessions)
    }

    /// Mint a bearer invitation URL with an explicit lifetime and session cap.
    pub fn bearer_invite_with(&self, ttl: Duration, max_sessions: u32) -> Result<Url> {
        if max_sessions == 0 {
            bail!("max-sessions must be greater than zero");
        }
        let invite_id = Uuid::new_v4();
        let capability: [u8; 32] = rand::rng().random();
        let ttl_seconds =
            i64::try_from(ttl.as_secs()).context("invitation lifetime is too large")?;
        let expires_at_unix = unix_now().saturating_add(ttl_seconds);
        let encoded = sign_invite(
            &self.identity,
            InviteGrant {
                bootstrap_origin: self.bootstrap_origin.clone(),
                endpoint_ticket: EndpointTicket::new(self.endpoint.addr()).to_string(),
                invite_id,
                capability,
                expires_at_unix,
                entry_path: self.entry_path.clone(),
                max_sessions,
            },
        )?;
        let url = invite_url(&encoded)?;
        self.registry
            .insert(invite_id, &capability, expires_at_unix, max_sessions)?;
        Ok(url)
    }

    /// Mint a subject-bound invite for one browser or application key.
    ///
    /// The returned token is not a transferable URL. Admit succeeds only when
    /// the client proves the matching private key.
    pub fn mint(&self, subject: [u8; 32]) -> Result<String> {
        self.mint_with(subject, MintOptions::default())
    }

    /// Mint a subject-bound invite with an explicit lifetime and use count.
    pub fn mint_with(&self, subject: [u8; 32], options: MintOptions) -> Result<String> {
        if options.max_sessions == 0 {
            bail!("max-sessions must be greater than zero");
        }
        iroh::PublicKey::from_bytes(&subject).context("mint subject is not a valid public key")?;
        let invite_id = Uuid::new_v4();
        let capability: [u8; 32] = rand::rng().random();
        let ttl_seconds =
            i64::try_from(options.ttl.as_secs()).context("mint lifetime is too large")?;
        let expires_at_unix = unix_now().saturating_add(ttl_seconds);
        self.registry.insert_subject_bound(
            invite_id,
            &capability,
            subject,
            expires_at_unix,
            options.max_sessions,
        )?;
        sign_subject_invite(
            &self.identity,
            SubjectInviteIssue {
                bootstrap_origin: self.bootstrap_origin.clone(),
                endpoint_ticket: EndpointTicket::new(self.endpoint.addr()).to_string(),
                invite_id,
                capability,
                subject,
                expires_at_unix,
                max_sessions: options.max_sessions,
                entry_path: self.entry_path.clone(),
            },
        )
        .context("sign subject-bound invitation")
    }

    /// Admitted sessions, including whether each is currently connected.
    pub fn sessions(&self) -> Vec<SessionInfo> {
        self.registry.sessions()
    }

    /// Kick one session by its operator handle (`session-…`).
    pub fn kick(&self, session_handle: &str) -> Result<bool> {
        let matches: Vec<_> = self
            .registry
            .sessions()
            .into_iter()
            .filter(|session| {
                session.operator_handle == session_handle
                    || session.operator_handle.starts_with(session_handle)
            })
            .collect();
        if matches.len() != 1 {
            bail!("session handle is missing or ambiguous");
        }
        Ok(self.registry.kick(matches[0].session_id)?)
    }

    /// Stop the host and close every live connection.
    pub async fn shutdown(self) -> Result<()> {
        self.router.shutdown().await?;
        Ok(())
    }
}

async fn ensure_app_is_listening(origin: &str) -> Result<()> {
    let url = Url::parse(origin).context("parse canonical loopback app")?;
    let host = url.host_str().context("loopback app has no host")?;
    let port = url
        .port_or_known_default()
        .context("loopback app has no port")?;
    let connection = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    .with_context(|| format!("timed out connecting to {origin}"))?;
    connection.with_context(|| {
        format!("could not connect to {origin}; start the local app before sharing it")
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use crate::native_client::NativeSiteClient;
    use urspace_protocol::RequestMethod;

    async fn hello_origin() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0_u8; 1024];
                    let _ = socket.read(&mut buf).await;
                    let body = b"embed-ok";
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket.write_all(body).await;
                });
            }
        });
        (format!("http://{address}"), task)
    }

    #[tokio::test]
    async fn embedded_host_accepts_bearer_and_subject_bound_clients() {
        let (origin, server) = hello_origin().await;
        let site = Site::serve_local(&origin, ShareOptions::default())
            .await
            .unwrap();
        let invite = site.bearer_invite().unwrap();
        assert!(invite.as_str().contains("#u4="));

        let client = NativeSiteClient::connect(invite.as_str(), unix_now())
            .await
            .unwrap();
        let response = client
            .fetch(RequestMethod::Get, "/".into(), Vec::new(), Vec::new())
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"embed-ok");
        drop(client);

        let session_key = SecretKey::generate();
        let token = site.mint(*session_key.public().as_bytes()).unwrap();
        assert!(token.starts_with("usi1."));
        let minted = NativeSiteClient::connect_minted(&token, session_key, unix_now())
            .await
            .unwrap();
        let response = minted
            .fetch(RequestMethod::Get, "/".into(), Vec::new(), Vec::new())
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"embed-ok");
        drop(minted);

        let stranger = SecretKey::generate();
        let stolen = site.mint(*stranger.public().as_bytes()).unwrap();
        let wrong_key = SecretKey::generate();
        assert!(
            NativeSiteClient::connect_minted(&stolen, wrong_key, unix_now())
                .await
                .is_err()
        );

        site.shutdown().await.unwrap();
        server.abort();
    }
}
