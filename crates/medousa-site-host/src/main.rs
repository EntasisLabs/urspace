use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::{Parser, Subcommand};
use iroh::protocol::Router;
use iroh::{Endpoint, SecretKey, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use medousa_site_host::{CapabilityRegistry, LoopbackSite, SiteProtocol, StaticSite, unix_now};
use medousa_site_protocol::{
    ALPN, ClientHello, INVITE_VERSION, InviteGrant, RequestMethod, ServerHello, SiteRequest,
    SiteResponseHead, invite_url, read_frame, sign_invite, verify_invite_url, write_frame,
};
use rand::Rng as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "medousa-site", about = "Capability-addressed sites over Iroh")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve {
        root: PathBuf,
        #[arg(long)]
        bootstrap_origin: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
        #[arg(long, default_value_t = 8)]
        max_sessions: u32,
        #[arg(long, default_value = "/")]
        entry_path: String,
        #[arg(long)]
        identity_file: Option<PathBuf>,
    },
    Proxy {
        #[arg(long)]
        upstream: String,
        #[arg(long)]
        bootstrap_origin: String,
        #[arg(long, default_value_t = 3600)]
        ttl_seconds: u64,
        #[arg(long, default_value_t = 8)]
        max_sessions: u32,
        #[arg(long, default_value = "/")]
        entry_path: String,
        #[arg(long)]
        identity_file: Option<PathBuf>,
    },
    Get {
        invite_url: String,
        #[arg(default_value = "/")]
        path: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve {
            root,
            bootstrap_origin,
            ttl_seconds,
            max_sessions,
            entry_path,
            identity_file,
        } => {
            serve(
                root,
                bootstrap_origin,
                ttl_seconds,
                max_sessions,
                entry_path,
                identity_file,
            )
            .await
        }
        Command::Proxy {
            upstream,
            bootstrap_origin,
            ttl_seconds,
            max_sessions,
            entry_path,
            identity_file,
        } => {
            proxy(
                upstream,
                bootstrap_origin,
                ttl_seconds,
                max_sessions,
                entry_path,
                identity_file,
            )
            .await
        }
        Command::Get { invite_url, path } => get(&invite_url, &path).await,
    }
}

async fn serve(
    root: PathBuf,
    bootstrap_origin: String,
    ttl_seconds: u64,
    max_sessions: u32,
    entry_path: String,
    identity_file: Option<PathBuf>,
) -> Result<()> {
    let site = StaticSite::open(root).await?;
    serve_protocol(
        SiteProtocol::new,
        site,
        bootstrap_origin,
        ttl_seconds,
        max_sessions,
        entry_path,
        identity_file,
    )
    .await
}

async fn proxy(
    upstream: String,
    bootstrap_origin: String,
    ttl_seconds: u64,
    max_sessions: u32,
    entry_path: String,
    identity_file: Option<PathBuf>,
) -> Result<()> {
    let site = LoopbackSite::open(&upstream)?;
    serve_protocol(
        SiteProtocol::loopback,
        site,
        bootstrap_origin,
        ttl_seconds,
        max_sessions,
        entry_path,
        identity_file,
    )
    .await
}

async fn serve_protocol<T>(
    make_protocol: fn(CapabilityRegistry, T) -> SiteProtocol,
    site: T,
    bootstrap_origin: String,
    ttl_seconds: u64,
    max_sessions: u32,
    entry_path: String,
    identity_file: Option<PathBuf>,
) -> Result<()> {
    if max_sessions == 0 {
        bail!("max-sessions must be greater than zero");
    }
    let identity_path = identity_file.unwrap_or(default_identity_path()?);
    let identity = load_or_create_identity(&identity_path)?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(identity.clone())
        .bind()
        .await
        .context("bind Iroh endpoint")?;
    endpoint.online().await;
    let registry = CapabilityRegistry::default();
    let invite_id = Uuid::new_v4();
    let capability: [u8; 32] = rand::rng().random();
    let ttl = i64::try_from(ttl_seconds).unwrap_or(i64::MAX);
    let expires_at_unix = unix_now().saturating_add(ttl);
    registry.insert(invite_id, &capability, expires_at_unix, max_sessions);

    let ticket = EndpointTicket::new(endpoint.addr()).to_string();
    let encoded = sign_invite(
        &identity,
        InviteGrant {
            bootstrap_origin,
            endpoint_ticket: ticket,
            invite_id,
            capability,
            expires_at_unix,
            entry_path,
            max_sessions,
        },
    )?;
    let url = invite_url(&encoded)?;
    let router = Router::builder(endpoint)
        .accept(ALPN, make_protocol(registry, site))
        .spawn();

    println!("Site identity: {}", identity.public().to_z32());
    println!("Invite expires at Unix time {expires_at_unix}");
    println!("Invite URL (treat as a secret):\n{url}");
    println!("Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    router.shutdown().await?;
    Ok(())
}

async fn get(raw_invite: &str, requested_path: &str) -> Result<()> {
    let invite = verify_invite_url(raw_invite, unix_now()).context("verify invitation")?;
    let ticket =
        EndpointTicket::from_str(&invite.endpoint_ticket).context("parse endpoint ticket")?;
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind Iroh client")?;
    endpoint.online().await;
    let connection = endpoint
        .connect(ticket.endpoint_addr().clone(), ALPN)
        .await
        .context("connect to site")?;

    let (mut hello_send, mut hello_recv) = connection.open_bi().await?;
    write_frame(
        &mut hello_send,
        &ClientHello {
            version: INVITE_VERSION,
            invite_id: invite.invite_id,
            capability: invite.capability,
        },
    )
    .await?;
    hello_send.finish()?;
    match read_frame::<_, ServerHello>(&mut hello_recv).await? {
        ServerHello::Granted { .. } => {}
        ServerHello::Denied { code } => bail!("site denied invitation: {code:?}"),
    }

    let path = if requested_path == "/" {
        &invite.entry_path
    } else {
        requested_path
    };
    let (mut send, mut recv) = connection.open_bi().await?;
    write_frame(
        &mut send,
        &SiteRequest {
            method: RequestMethod::Get,
            path: path.to_string(),
            headers: Vec::new(),
            body_length: 0,
        },
    )
    .await?;
    send.finish()?;
    let head: SiteResponseHead = read_frame(&mut recv).await?;
    if head.status != 200 {
        bail!("site returned HTTP-like status {}", head.status);
    }
    let mut stdout = tokio::io::stdout();
    tokio::io::copy(&mut recv, &mut stdout).await?;
    endpoint.close().await;
    Ok(())
}

fn default_identity_path() -> Result<PathBuf> {
    let base = dirs::data_local_dir().context("local data directory is unavailable")?;
    Ok(base.join("medousa-sites").join("identity.key"))
}

fn load_or_create_identity(path: &Path) -> Result<SecretKey> {
    match std::fs::read_to_string(path) {
        Ok(encoded) => {
            let bytes = URL_SAFE_NO_PAD
                .decode(encoded.trim())
                .context("decode site identity")?;
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("site identity has invalid length"))?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let identity = SecretKey::generate();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create identity directory {}", parent.display()))?;
            }
            write_private_file(path, URL_SAFE_NO_PAD.encode(identity.to_bytes()).as_bytes())?;
            Ok(identity)
        }
        Err(error) => Err(error).with_context(|| format!("read identity {}", path.display())),
    }
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("create identity {}", path.display()))?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}
