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
use rand::Rng as _;
use tokio::io::AsyncBufReadExt as _;
use urspace_host::{
    CapabilityRegistry, LoopbackSite, SessionGrantIssuer, SiteProtocol, StaticSite, unix_now,
};
use urspace_protocol::{
    ALPN, ClientAuthV4, ClientHello, ClientProofV4, INVITE_VERSION, InviteGrant, RequestMethod,
    SESSION_PROOF_VERSION, ServerAuthV4, ServerHello, ServerHelloV4, SessionProofPayload,
    SessionProofPurpose, SiteRequest, SiteResponseHead, alpn_for_invite, invite_url, read_frame,
    sign_invite, sign_session_proof, verify_invite_url, verify_session_grant, write_frame,
};
use uuid::Uuid;

mod short_link;

use short_link::{DEFAULT_SHORT_ORIGIN, ShortLinkPublisher};

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
        /// How long the invitation accepts new browser sessions.
        #[arg(long, default_value = DEFAULT_TTL, value_parser = parse_duration)]
        ttl: Duration,
        /// Maximum unique browser sessions admitted by this invitation.
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
        /// Publish an encrypted short link instead of printing the direct capability URL.
        #[arg(long)]
        short: bool,
        /// Short-link service origin. Intended for compatible self-hosted deployments.
        #[arg(long, default_value = DEFAULT_SHORT_ORIGIN, hide = true)]
        short_origin: String,
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
        #[arg(long)]
        short: bool,
        #[arg(long, default_value = DEFAULT_SHORT_ORIGIN, hide = true)]
        short_origin: String,
    },
    /// Fetch a path with the native protocol client.
    #[command(hide = true)]
    Get {
        invite_url: String,
        #[arg(default_value = "/")]
        path: String,
    },
}

struct ShareSettings {
    bootstrap_origin: String,
    ttl: Duration,
    max_sessions: u32,
    entry_path: String,
    name: Option<String>,
    identity_file: Option<PathBuf>,
    short: bool,
    short_origin: String,
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
            short,
            short_origin,
        } => {
            proxy(
                app,
                ShareSettings {
                    bootstrap_origin,
                    ttl,
                    max_sessions,
                    entry_path,
                    name,
                    identity_file,
                    short,
                    short_origin,
                },
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
            short,
            short_origin,
        } => {
            serve_static(
                root,
                ShareSettings {
                    bootstrap_origin,
                    ttl,
                    max_sessions,
                    entry_path,
                    name,
                    identity_file,
                    short,
                    short_origin,
                },
            )
            .await
        }
        Command::Get { invite_url, path } => get(&invite_url, &path).await,
    }
}

async fn proxy(upstream: String, settings: ShareSettings) -> Result<()> {
    let site = LoopbackSite::open(&upstream)?;
    ensure_app_is_listening(site.origin()).await?;
    let identity_path = identity_path(
        settings.identity_file,
        settings.name.as_deref(),
        site.origin(),
    )?;
    serve_protocol(
        SiteProtocol::loopback,
        site,
        format!("app at {}", upstream.trim_end_matches('/')),
        settings.bootstrap_origin,
        settings.ttl,
        settings.max_sessions,
        settings.entry_path,
        identity_path,
        settings
            .short
            .then(|| ShortLinkPublisher::new(&settings.short_origin))
            .transpose()?,
    )
    .await
}

async fn serve_static(root: PathBuf, settings: ShareSettings) -> Result<()> {
    let canonical_root = tokio::fs::canonicalize(&root)
        .await
        .with_context(|| format!("resolve static directory {}", root.display()))?;
    let site = StaticSite::open(&canonical_root).await?;
    let source_key = canonical_root.to_string_lossy();
    let identity_path = identity_path(
        settings.identity_file,
        settings.name.as_deref(),
        &source_key,
    )?;
    serve_protocol(
        SiteProtocol::new,
        site,
        format!("files from {}", canonical_root.display()),
        settings.bootstrap_origin,
        settings.ttl,
        settings.max_sessions,
        settings.entry_path,
        identity_path,
        settings
            .short
            .then(|| ShortLinkPublisher::new(&settings.short_origin))
            .transpose()?,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn serve_protocol<T>(
    make_protocol: fn(CapabilityRegistry, T, SessionGrantIssuer) -> SiteProtocol,
    site: T,
    source_description: String,
    bootstrap_origin: String,
    ttl: Duration,
    max_sessions: u32,
    entry_path: String,
    identity_path: PathBuf,
    short_links: Option<ShortLinkPublisher>,
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
    let (invite_id, raw_url, expires_at_unix) = mint_invite(
        &endpoint,
        &identity,
        &registry,
        &bootstrap_origin,
        ttl_seconds,
        max_sessions,
        &entry_path,
    )?;
    let share_url = publish_share_url(&raw_url, expires_at_unix, short_links.as_ref()).await;
    let shortened = share_url != raw_url;
    let issuer = SessionGrantIssuer::new(
        identity.clone(),
        bootstrap_origin.clone(),
        EndpointTicket::new(endpoint.addr()).to_string(),
        entry_path.clone(),
    );
    let router = Router::builder(endpoint.clone())
        .accept(ALPN, make_protocol(registry.clone(), site, issuer))
        .spawn();

    println!("Urspace is serving {source_description}");
    if shortened {
        println!(
            "Only an end-to-end encrypted link envelope was uploaded; application traffic travels over Iroh.\n"
        );
    } else {
        println!("Nothing was uploaded; application traffic travels over Iroh.\n");
    }
    println!("Share URL (treat it as a secret):\n{share_url}\n");
    if shortened {
        println!("Run `raw` to print the direct capability URL.");
    }
    println!("Accepts new browser sessions for: {}", format_duration(ttl));
    println!("Maximum admitted browser sessions: {max_sessions}");
    println!("Site identity: {}", identity.public().to_z32());
    println!("Commands: invite | rotate | raw | sessions | kick <session> | kick all | help");
    println!("Press Ctrl+C to stop sharing.\n");

    let console = operator_console(
        endpoint,
        identity,
        registry,
        bootstrap_origin,
        ttl_seconds,
        max_sessions,
        entry_path,
        short_links,
        invite_id,
        raw_url,
    );
    tokio::pin!(console);
    tokio::select! {
        signal = tokio::signal::ctrl_c() => signal?,
        result = &mut console => {
            result?;
            tokio::signal::ctrl_c().await?;
        }
    }
    router.shutdown().await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn mint_invite(
    endpoint: &Endpoint,
    identity: &SecretKey,
    registry: &CapabilityRegistry,
    bootstrap_origin: &str,
    ttl_seconds: i64,
    max_sessions: u32,
    entry_path: &str,
) -> Result<(Uuid, url::Url, i64)> {
    let invite_id = Uuid::new_v4();
    let capability: [u8; 32] = rand::rng().random();
    let expires_at_unix = unix_now().saturating_add(ttl_seconds);
    let ticket = EndpointTicket::new(endpoint.addr()).to_string();
    let encoded = sign_invite(
        identity,
        InviteGrant {
            bootstrap_origin: bootstrap_origin.to_owned(),
            endpoint_ticket: ticket,
            invite_id,
            capability,
            expires_at_unix,
            entry_path: entry_path.to_owned(),
            max_sessions,
        },
    )?;
    let url = invite_url(&encoded)?;
    registry.insert(invite_id, &capability, expires_at_unix, max_sessions);
    Ok((invite_id, url, expires_at_unix))
}

async fn publish_share_url(
    raw_url: &url::Url,
    expires_at_unix: i64,
    short_links: Option<&ShortLinkPublisher>,
) -> url::Url {
    let Some(short_links) = short_links else {
        return raw_url.clone();
    };
    match short_links.publish(raw_url, expires_at_unix).await {
        Ok(short_url) => short_url,
        Err(error) => {
            eprintln!("Short-link publishing failed; using the direct URL: {error:#}");
            raw_url.clone()
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn operator_console(
    endpoint: Endpoint,
    identity: SecretKey,
    registry: CapabilityRegistry,
    bootstrap_origin: String,
    ttl_seconds: i64,
    max_sessions: u32,
    entry_path: String,
    short_links: Option<ShortLinkPublisher>,
    mut current_invite_id: Uuid,
    mut current_raw_url: url::Url,
) -> Result<()> {
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let command = line.trim();
        match command {
            "" => {}
            "invite" | "rotate" => {
                let url = rotate_invite(
                    &endpoint,
                    &identity,
                    &registry,
                    &bootstrap_origin,
                    ttl_seconds,
                    max_sessions,
                    &entry_path,
                    short_links.as_ref(),
                    &mut current_invite_id,
                    &mut current_raw_url,
                )
                .await?;
                println!("New share URL (treat it as a secret):\n{url}");
                if url != current_raw_url {
                    println!("Run `raw` to print the direct capability URL.");
                }
                println!("Previously admitted sessions remain connected.");
            }
            "raw" => println!("Direct capability URL (treat it as a secret):\n{current_raw_url}"),
            "sessions" => print_sessions(&registry),
            "kick all" => {
                let count = registry.kick_all();
                println!("Kicked {count} admitted session(s).");
                if count > 0 {
                    let url = rotate_invite(
                        &endpoint,
                        &identity,
                        &registry,
                        &bootstrap_origin,
                        ttl_seconds,
                        max_sessions,
                        &entry_path,
                        short_links.as_ref(),
                        &mut current_invite_id,
                        &mut current_raw_url,
                    )
                    .await?;
                    println!("The old URL is closed to newcomers. Fresh share URL:\n{url}");
                }
            }
            "help" => {
                println!("invite          stop new admissions on the old URL and print a new one");
                println!("rotate          alias for invite");
                println!("raw             print the current direct capability URL");
                println!("sessions        list admitted browser identities");
                println!("kick <session>  disconnect and deny one browser identity");
                println!("kick all        disconnect and deny every admitted browser identity");
            }
            command if command.starts_with("kick ") => {
                if kick_session(&registry, command[5..].trim()) {
                    let url = rotate_invite(
                        &endpoint,
                        &identity,
                        &registry,
                        &bootstrap_origin,
                        ttl_seconds,
                        max_sessions,
                        &entry_path,
                        short_links.as_ref(),
                        &mut current_invite_id,
                        &mut current_raw_url,
                    )
                    .await?;
                    println!("The old URL is closed to newcomers. Fresh share URL:\n{url}");
                }
            }
            _ => println!("Unknown command. Run `help` for available commands."),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn rotate_invite(
    endpoint: &Endpoint,
    identity: &SecretKey,
    registry: &CapabilityRegistry,
    bootstrap_origin: &str,
    ttl_seconds: i64,
    max_sessions: u32,
    entry_path: &str,
    short_links: Option<&ShortLinkPublisher>,
    current_invite_id: &mut Uuid,
    current_raw_url: &mut url::Url,
) -> Result<url::Url> {
    let (next_invite_id, raw_url, expires_at_unix) = mint_invite(
        endpoint,
        identity,
        registry,
        bootstrap_origin,
        ttl_seconds,
        max_sessions,
        entry_path,
    )?;
    let share_url = publish_share_url(&raw_url, expires_at_unix, short_links).await;
    registry.close_admissions(*current_invite_id);
    *current_invite_id = next_invite_id;
    *current_raw_url = raw_url;
    Ok(share_url)
}

fn print_sessions(registry: &CapabilityRegistry) {
    let sessions = registry.sessions();
    if sessions.is_empty() {
        println!("No browser sessions have been admitted.");
        return;
    }
    for session in sessions {
        let state = if session.connected {
            "connected"
        } else {
            "disconnected (may reconnect)"
        };
        println!(
            "{}  {}  {state}",
            session.session_id,
            session.endpoint_id.to_z32()
        );
    }
}

fn kick_session(registry: &CapabilityRegistry, prefix: &str) -> bool {
    if prefix.is_empty() {
        println!("Usage: kick <session-id-prefix>");
        return false;
    }
    let matches: Vec<_> = registry
        .sessions()
        .into_iter()
        .filter(|session| {
            session.session_id.to_string().starts_with(prefix)
                || session.endpoint_id.to_z32().starts_with(prefix)
        })
        .collect();
    match matches.as_slice() {
        [] => {
            println!("No admitted session matches `{prefix}`.");
            false
        }
        [session] => {
            if registry.kick(session.session_id) {
                println!("Kicked session {}.", session.session_id);
                true
            } else {
                false
            }
        }
        _ => {
            println!("Session prefix `{prefix}` is ambiguous; enter more characters.");
            false
        }
    }
}

async fn get(raw_invite: &str, requested_path: &str) -> Result<()> {
    let invite = verify_invite_url(raw_invite, unix_now()).context("verify invitation")?;
    let ticket =
        EndpointTicket::from_str(&invite.endpoint_ticket).context("parse endpoint ticket")?;
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind Iroh client")?;
    endpoint.online().await;
    let alpn = alpn_for_invite(invite.version)?;
    let connection = endpoint
        .connect(ticket.endpoint_addr().clone(), alpn)
        .await
        .context("connect to site")?;

    if invite.version == INVITE_VERSION {
        let session_key = SecretKey::generate();
        let (mut hello_send, mut hello_recv) = connection.open_bi().await?;
        write_frame(
            &mut hello_send,
            &ClientAuthV4::Admit {
                invite_id: invite.invite_id,
                capability: invite.capability,
                session_public_key: *session_key.public().as_bytes(),
            },
        )
        .await?;
        let challenge = match read_frame(&mut hello_recv).await? {
            ServerAuthV4::Challenge(challenge) => challenge,
            ServerAuthV4::Denied { code } => bail!("site denied invitation: {code:?}"),
        };
        let proof = SessionProofPayload {
            version: SESSION_PROOF_VERSION,
            purpose: SessionProofPurpose::Admit,
            host_id: *ticket.endpoint_addr().id.as_bytes(),
            site_id: ticket.endpoint_addr().id.to_z32(),
            alpn: ALPN.to_vec(),
            session_id: challenge.session_id,
            session_grant_hash: [0_u8; 32],
            endpoint_id: *endpoint.id().as_bytes(),
            challenge_id: challenge.challenge_id,
            nonce: challenge.nonce,
            expires_at_unix: challenge.expires_at_unix,
        };
        write_frame(
            &mut hello_send,
            &ClientProofV4 {
                signature: sign_session_proof(&session_key, &proof)?,
            },
        )
        .await?;
        hello_send.finish()?;
        match read_frame(&mut hello_recv).await? {
            ServerHelloV4::Granted { session_grant } => {
                verify_session_grant(&session_grant, unix_now())?;
            }
            ServerHelloV4::Denied { code } => bail!("site denied invitation: {code:?}"),
        }
    } else {
        let (mut hello_send, mut hello_recv) = connection.open_bi().await?;
        write_frame(
            &mut hello_send,
            &ClientHello {
                version: invite.version,
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
            short,
            short_origin,
            ..
        } = cli.command
        else {
            panic!("expected serve command");
        };
        assert_eq!(app, "http://127.0.0.1:8787/");
        assert_eq!(bootstrap_origin, DEFAULT_BOOTSTRAP_ORIGIN);
        assert_eq!(ttl, Duration::from_secs(3_600));
        assert_eq!(max_sessions, DEFAULT_MAX_SESSIONS);
        assert!(!short);
        assert_eq!(short_origin, DEFAULT_SHORT_ORIGIN);
    }

    #[test]
    fn encrypted_short_links_require_explicit_operator_opt_in() {
        let cli = Cli::try_parse_from(["urspace", "serve", "localhost:8787", "--short"]).unwrap();
        let Command::Serve { short, .. } = cli.command else {
            panic!("expected serve command");
        };
        assert!(short);
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
