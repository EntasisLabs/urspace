use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

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
use uuid::Uuid;

const DEFAULT_BOOTSTRAP_ORIGIN: &str = "https://urspace.online";
const DEFAULT_TTL: &str = "1h";
const DEFAULT_MAX_SESSIONS: u32 = 4;

#[derive(Debug, Parser)]
#[command(
    name = "urspace",
    version,
    about = "Share localhost apps securely over Iroh"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Share an HTTP app that is already running on this machine.
    Serve {
        /// Loopback app, for example localhost:8787.
        #[arg(value_name = "LOCAL_APP", value_parser = normalize_loopback_app)]
        app: String,
        /// Public Urspace bootstrap origin.
        #[arg(long, default_value = DEFAULT_BOOTSTRAP_ORIGIN)]
        bootstrap_origin: String,
        /// Invitation lifetime, such as 10m, 1h, or 1d.
        #[arg(long, default_value = DEFAULT_TTL, value_parser = parse_duration)]
        ttl: Duration,
        /// Maximum successful browser connections for this invitation.
        #[arg(long, default_value_t = DEFAULT_MAX_SESSIONS)]
        max_sessions: u32,
        /// Path opened when the recipient connects.
        #[arg(long, default_value = "/")]
        entry_path: String,
        /// Stable local name used to select this app's identity.
        #[arg(long, value_parser = normalize_site_name)]
        name: Option<String>,
        /// Explicit identity key path for development and recovery.
        #[arg(long, hide = true)]
        identity_file: Option<PathBuf>,
    },
    /// Share a directory of static files.
    Static {
        #[arg(value_name = "DIRECTORY")]
        root: PathBuf,
        #[arg(long, default_value = DEFAULT_BOOTSTRAP_ORIGIN)]
        bootstrap_origin: String,
        #[arg(long, default_value = DEFAULT_TTL, value_parser = parse_duration)]
        ttl: Duration,
        #[arg(long, default_value_t = DEFAULT_MAX_SESSIONS)]
        max_sessions: u32,
        #[arg(long, default_value = "/")]
        entry_path: String,
        #[arg(long, value_parser = normalize_site_name)]
        name: Option<String>,
        #[arg(long, hide = true)]
        identity_file: Option<PathBuf>,
    },
    /// Fetch a path with the native protocol client.
    #[command(hide = true)]
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
            app,
            bootstrap_origin,
            ttl,
            max_sessions,
            entry_path,
            name,
            identity_file,
        } => {
            proxy(
                app,
                bootstrap_origin,
                ttl,
                max_sessions,
                entry_path,
                name,
                identity_file,
            )
            .await
        }
        Command::Static {
            root,
            bootstrap_origin,
            ttl,
            max_sessions,
            entry_path,
            name,
            identity_file,
        } => {
            serve_static(
                root,
                bootstrap_origin,
                ttl,
                max_sessions,
                entry_path,
                name,
                identity_file,
            )
            .await
        }
        Command::Get { invite_url, path } => get(&invite_url, &path).await,
    }
}

async fn proxy(
    upstream: String,
    bootstrap_origin: String,
    ttl: Duration,
    max_sessions: u32,
    entry_path: String,
    name: Option<String>,
    identity_file: Option<PathBuf>,
) -> Result<()> {
    let site = LoopbackSite::open(&upstream)?;
    ensure_app_is_listening(site.origin()).await?;
    let identity_path = identity_path(identity_file, name.as_deref(), site.origin())?;
    serve_protocol(
        SiteProtocol::loopback,
        site,
        format!("app at {}", upstream.trim_end_matches('/')),
        bootstrap_origin,
        ttl,
        max_sessions,
        entry_path,
        identity_path,
    )
    .await
}

async fn serve_static(
    root: PathBuf,
    bootstrap_origin: String,
    ttl: Duration,
    max_sessions: u32,
    entry_path: String,
    name: Option<String>,
    identity_file: Option<PathBuf>,
) -> Result<()> {
    let canonical_root = tokio::fs::canonicalize(&root)
        .await
        .with_context(|| format!("resolve static directory {}", root.display()))?;
    let site = StaticSite::open(&canonical_root).await?;
    let source_key = canonical_root.to_string_lossy();
    let identity_path = identity_path(identity_file, name.as_deref(), &source_key)?;
    serve_protocol(
        SiteProtocol::new,
        site,
        format!("files from {}", canonical_root.display()),
        bootstrap_origin,
        ttl,
        max_sessions,
        entry_path,
        identity_path,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn serve_protocol<T>(
    make_protocol: fn(CapabilityRegistry, T) -> SiteProtocol,
    site: T,
    source_description: String,
    bootstrap_origin: String,
    ttl: Duration,
    max_sessions: u32,
    entry_path: String,
    identity_path: PathBuf,
) -> Result<()> {
    if max_sessions == 0 {
        bail!("max-sessions must be greater than zero");
    }
    let ttl_seconds = i64::try_from(ttl.as_secs()).context("invitation lifetime is too large")?;
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
    let expires_at_unix = unix_now().saturating_add(ttl_seconds);
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

    println!("Urspace is serving {source_description}");
    println!("Nothing was uploaded; application traffic travels over Iroh.\n");
    println!("Share URL (treat it as a secret):\n{url}\n");
    println!("Expires: in {}", format_duration(ttl));
    println!("Maximum browser connections: {max_sessions}");
    println!("Site identity: {}", identity.public().to_z32());
    println!("Press Ctrl+C to stop sharing.");
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

fn normalize_loopback_app(raw: &str) -> Result<String, String> {
    let candidate = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("http://{raw}")
    };
    let site = LoopbackSite::open(&candidate).map_err(|error| error.to_string())?;
    Ok(site.origin().to_owned())
}

async fn ensure_app_is_listening(origin: &str) -> Result<()> {
    let url = url::Url::parse(origin).context("parse canonical loopback app")?;
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

fn normalize_site_name(raw: &str) -> Result<String, String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.len() > 64
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("name must contain 1-64 letters, numbers, dashes, or underscores".into());
    }
    Ok(normalized)
}

fn parse_duration(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim().to_ascii_lowercase();
    let (digits, multiplier) = match raw.as_bytes().last().copied() {
        Some(b's') => (&raw[..raw.len() - 1], 1_u64),
        Some(b'm') => (&raw[..raw.len() - 1], 60),
        Some(b'h') => (&raw[..raw.len() - 1], 60 * 60),
        Some(b'd') => (&raw[..raw.len() - 1], 24 * 60 * 60),
        Some(byte) if byte.is_ascii_digit() => (raw.as_str(), 1),
        _ => {
            return Err(
                "duration must use seconds, minutes, hours, or days (for example 10m)".into(),
            );
        }
    };
    let amount = digits
        .parse::<u64>()
        .map_err(|_| "duration must start with a whole number".to_string())?;
    let seconds = amount
        .checked_mul(multiplier)
        .ok_or_else(|| "duration is too large".to_string())?;
    if seconds == 0 {
        return Err("duration must be greater than zero".into());
    }
    Ok(Duration::from_secs(seconds))
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds.is_multiple_of(86_400) {
        format!("{}d", seconds / 86_400)
    } else if seconds.is_multiple_of(3_600) {
        format!("{}h", seconds / 3_600)
    } else if seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

fn identity_path(
    explicit: Option<PathBuf>,
    name: Option<&str>,
    source_key: &str,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let base = dirs::data_local_dir().context("local data directory is unavailable")?;
    let key = name.map_or_else(
        || blake3::hash(source_key.as_bytes()).to_hex().to_string(),
        str::to_owned,
    );
    Ok(base
        .join("urspace")
        .join("sites")
        .join(format!("{key}.key")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_accepts_shorthand_loopback_app_and_secure_defaults() {
        let cli = Cli::try_parse_from(["urspace", "serve", "localhost:8787"]).unwrap();
        let Command::Serve {
            app,
            bootstrap_origin,
            ttl,
            max_sessions,
            ..
        } = cli.command
        else {
            panic!("expected serve command");
        };
        assert_eq!(app, "http://127.0.0.1:8787/");
        assert_eq!(bootstrap_origin, DEFAULT_BOOTSTRAP_ORIGIN);
        assert_eq!(ttl, Duration::from_secs(3_600));
        assert_eq!(max_sessions, DEFAULT_MAX_SESSIONS);
    }

    #[test]
    fn serve_rejects_non_loopback_or_credentialed_apps() {
        assert!(Cli::try_parse_from(["urspace", "serve", "https://example.com"]).is_err());
        assert!(
            Cli::try_parse_from(["urspace", "serve", "http://user:pass@localhost:8787"]).is_err()
        );
        assert!(Cli::try_parse_from(["urspace", "serve", "localhost:8787/admin"]).is_err());
    }

    #[test]
    fn duration_parser_is_bounded_and_human_friendly() {
        assert_eq!(parse_duration("10m").unwrap(), Duration::from_secs(600));
        assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7_200));
        assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
        assert!(parse_duration("0s").is_err());
        assert!(parse_duration("1.5h").is_err());
    }

    #[test]
    fn site_names_are_safe_and_canonical() {
        assert_eq!(normalize_site_name(" BoxClub ").unwrap(), "boxclub");
        assert!(normalize_site_name("../boxclub").is_err());
        assert!(normalize_site_name("").is_err());
    }

    #[tokio::test]
    async fn app_preflight_accepts_a_listening_loopback_socket() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap());
        ensure_app_is_listening(&origin).await.unwrap();
    }
}
