use std::io::Write as _;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use axum::Router as AxumRouter;
use axum::body::{Body, to_bytes};
use axum::extract::ws::{CloseFrame, Message as AxumSocketMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{FromRequestParts as _, State};
use axum::http::{HeaderMap, Request, Response, StatusCode, header};
use axum::response::IntoResponse as _;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt as _, StreamExt as _};
use iroh::protocol::Router;
use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, SecretKey, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use rand::Rng as _;
use tokio::io::AsyncBufReadExt as _;
use urspace_host::native_client::{
    NativeSiteClient, NativeTransport, receive_socket_message, send_socket_message,
};
use urspace_host::{
    CapabilityRegistry, LoopbackSite, SessionGrantIssuer, SiteProtocol, StaticSite, TunnelProtocol,
    unix_now,
};
use urspace_protocol::{
    ALPN, ClientAuthV4, ClientHello, ClientProofV4, INVITE_VERSION, InviteGrant, RequestMethod,
    SESSION_PROOF_VERSION, ServerAuthV4, ServerHello, ServerHelloV4, SessionProofPayload,
    SessionProofPurpose, SiteRequest, SiteResponseHead, TUNNEL_ALPN, alpn_for_invite, invite_url,
    read_frame, sign_invite, sign_session_proof, verify_invite_url, verify_session_grant,
    write_frame,
};
use uuid::Uuid;
use zeroize::Zeroize as _;

mod native_service;
mod service;
mod short_link;

use service::{ControlAction, ControlResponse, ControlServer, ManagedServiceConfig, SessionView};
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
    /// Run or control an always-on named Urspace service.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Connect to a private service through a native localhost endpoint.
    Connect {
        /// Local name for this private site enrollment.
        #[arg(value_parser = normalize_site_name)]
        name: String,
        /// Mount the remote service on this local TCP endpoint, for example localhost:9090.
        #[arg(value_name = "LOCAL_ENDPOINT", value_parser = normalize_local_mount, conflicts_with = "listen")]
        local_endpoint: Option<SocketAddr>,
        /// One-time invitation used to enroll this device.
        #[arg(long, value_name = "URL", conflicts_with = "invite_stdin")]
        invite: Option<String>,
        /// Read a one-time enrollment invitation from standard input.
        #[arg(long, conflicts_with = "invite")]
        invite_stdin: bool,
        /// Loopback address for the local browser gateway.
        #[arg(long, value_parser = normalize_connect_listen)]
        listen: Option<SocketAddr>,
    },
    /// Fetch a path with the native protocol client.
    #[command(hide = true)]
    Get {
        invite_url: String,
        #[arg(default_value = "/")]
        path: String,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Install and start a named app as a user service.
    Install {
        /// Loopback app, for example localhost:8787.
        #[arg(value_name = "LOCAL_APP", value_parser = normalize_loopback_app)]
        app: String,
        /// Stable name for this service and its local identity.
        #[arg(long, value_parser = normalize_site_name)]
        name: String,
        #[arg(long, default_value = DEFAULT_BOOTSTRAP_ORIGIN)]
        bootstrap_origin: String,
        #[arg(long, default_value = DEFAULT_TTL, value_parser = parse_duration)]
        ttl: Duration,
        #[arg(long, default_value_t = DEFAULT_MAX_SESSIONS)]
        max_sessions: u32,
        #[arg(long, default_value = "/")]
        entry_path: String,
        #[arg(long)]
        short: bool,
        #[arg(long, default_value = DEFAULT_SHORT_ORIGIN, hide = true)]
        short_origin: String,
    },
    /// Run one named local app until stopped by a signal or control command.
    Run {
        /// Loopback app, for example localhost:8787.
        #[arg(value_name = "LOCAL_APP", value_parser = normalize_loopback_app)]
        app: String,
        /// Stable name for this service and its local identity.
        #[arg(long, value_parser = normalize_site_name)]
        name: String,
        #[arg(long, default_value = DEFAULT_BOOTSTRAP_ORIGIN)]
        bootstrap_origin: String,
        #[arg(long, default_value = DEFAULT_TTL, value_parser = parse_duration)]
        ttl: Duration,
        #[arg(long, default_value_t = DEFAULT_MAX_SESSIONS)]
        max_sessions: u32,
        #[arg(long, default_value = "/")]
        entry_path: String,
        #[arg(long)]
        short: bool,
        #[arg(long, default_value = DEFAULT_SHORT_ORIGIN, hide = true)]
        short_origin: String,
    },
    /// Show whether a named service is running.
    Status {
        #[arg(value_parser = normalize_site_name)]
        name: String,
    },
    /// Start an installed named service.
    Start {
        #[arg(value_parser = normalize_site_name)]
        name: String,
    },
    /// Restart an installed named service and create a fresh invitation.
    Restart {
        #[arg(value_parser = normalize_site_name)]
        name: String,
    },
    /// Create a fresh invitation for a running service.
    Invite {
        #[arg(value_parser = normalize_site_name)]
        name: String,
        /// Local label shown beside devices admitted with this invitation.
        #[arg(long = "for", value_name = "PERSON_OR_DEVICE", value_parser = normalize_access_label)]
        access_label: Option<String>,
        /// Admission limit for this invitation (defaults to 1 with --for).
        #[arg(long, value_name = "COUNT", value_parser = clap::value_parser!(u32).range(1..))]
        max_sessions: Option<u32>,
        /// Print the direct invitation required by native device enrollment.
        #[arg(long)]
        direct: bool,
        /// Permit this invitation to mount the app as a raw local TCP port.
        #[arg(long)]
        tcp: bool,
    },
    /// List browser and native devices admitted to a running service.
    Sessions {
        #[arg(value_parser = normalize_site_name)]
        name: String,
    },
    /// Disconnect and revoke one admitted browser or native-device session.
    Kick {
        #[arg(value_parser = normalize_site_name)]
        name: String,
        session_handle: String,
    },
    /// Disconnect and revoke every admitted browser or native-device session.
    KickAll {
        #[arg(value_parser = normalize_site_name)]
        name: String,
    },
    /// Gracefully stop a running service.
    Stop {
        #[arg(value_parser = normalize_site_name)]
        name: String,
    },
    /// Remove automatic startup while preserving site identity and access state.
    Uninstall {
        #[arg(value_parser = normalize_site_name)]
        name: String,
    },
    /// Internal supervisor entry point.
    #[command(hide = true)]
    ManagedRun {
        #[arg(value_parser = normalize_site_name)]
        name: String,
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
        Command::Service { command } => match command {
            ServiceCommand::Install {
                app,
                name,
                bootstrap_origin,
                ttl,
                max_sessions,
                entry_path,
                short,
                short_origin,
            } => {
                install_managed_service(
                    app,
                    name,
                    ShareSettings {
                        bootstrap_origin,
                        ttl,
                        max_sessions,
                        entry_path,
                        name: None,
                        identity_file: None,
                        short,
                        short_origin,
                    },
                )
                .await
            }
            ServiceCommand::Run {
                app,
                name,
                bootstrap_origin,
                ttl,
                max_sessions,
                entry_path,
                short,
                short_origin,
            } => {
                run_service(
                    app,
                    name,
                    ShareSettings {
                        bootstrap_origin,
                        ttl,
                        max_sessions,
                        entry_path,
                        name: None,
                        identity_file: None,
                        short,
                        short_origin,
                    },
                    true,
                )
                .await
            }
            ServiceCommand::Status { name } => service_status(&name).await,
            ServiceCommand::Start { name } => start_managed_service(&name).await,
            ServiceCommand::Restart { name } => restart_managed_service(&name).await,
            ServiceCommand::Invite {
                name,
                access_label,
                max_sessions,
                direct,
                tcp,
            } => {
                let max_sessions = max_sessions.or(access_label.as_ref().map(|_| 1));
                service_request(
                    &name,
                    ControlAction::Invite {
                        access_label,
                        max_sessions,
                        direct: direct || tcp,
                        tcp,
                    },
                )
                .await
            }
            ServiceCommand::Sessions { name } => {
                service_request(&name, ControlAction::Sessions).await
            }
            ServiceCommand::Kick {
                name,
                session_handle,
            } => service_request(&name, ControlAction::Kick { session_handle }).await,
            ServiceCommand::KickAll { name } => {
                service_request(&name, ControlAction::KickAll).await
            }
            ServiceCommand::Stop { name } => stop_service(&name).await,
            ServiceCommand::Uninstall { name } => uninstall_managed_service(&name).await,
            ServiceCommand::ManagedRun { name } => run_managed_service(&name).await,
        },
        Command::Connect {
            name,
            local_endpoint,
            invite,
            invite_stdin,
            listen,
        } => connect_site(name, local_endpoint, invite, invite_stdin, listen).await,
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

async fn install_managed_service(
    upstream: String,
    name: String,
    settings: ShareSettings,
) -> Result<()> {
    native_service::ensure_supported()?;
    validate_share_settings(&settings)?;
    let site = LoopbackSite::open(&upstream)?;
    ensure_app_is_listening(site.origin()).await?;
    ControlServer::ensure_available(&name).await?;

    let config = ManagedServiceConfig::new(
        upstream,
        settings.bootstrap_origin,
        settings.ttl.as_secs(),
        settings.max_sessions,
        settings.entry_path,
        settings.short,
        settings.short_origin,
    );
    service::save_managed_config(&name, &config)?;
    let executable = std::env::current_exe().context("locate the Urspace executable")?;
    let data_directory = service::data_directory()?;
    let service_directory = service::service_directory(&name)?;
    let outcome = native_service::install(&name, &executable, &data_directory, &service_directory)?;
    await_service_ready(&name).await.with_context(|| {
        format!(
            "service was installed at {} but did not become ready",
            outcome.definition_path.display()
        )
    })?;

    println!("Installed and started Urspace service `{name}`.");
    println!("Definition: {}", outcome.definition_path.display());
    if let Some(note) = outcome.persistence_note {
        println!("{note}");
    }
    service_request(
        &name,
        ControlAction::Invite {
            access_label: None,
            max_sessions: None,
            direct: false,
            tcp: false,
        },
    )
    .await
}

async fn run_managed_service(name: &str) -> Result<()> {
    let config = service::load_managed_config(name)?;
    run_service(
        config.app,
        name.to_owned(),
        ShareSettings {
            bootstrap_origin: config.bootstrap_origin,
            ttl: Duration::from_secs(config.ttl_seconds),
            max_sessions: config.max_sessions,
            entry_path: config.entry_path,
            name: None,
            identity_file: None,
            short: config.short,
            short_origin: config.short_origin,
        },
        false,
    )
    .await
}

async fn service_status(name: &str) -> Result<()> {
    match service::request(name, ControlAction::Status).await {
        Ok(response) => print_service_response(response),
        Err(_) if native_service::is_installed(name)? => {
            println!("Service `{name}` is installed but is not accepting control requests.");
            println!("Run `urspace service start {name}` to start it.");
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn start_managed_service(name: &str) -> Result<()> {
    native_service::start(name)?;
    await_service_ready(name).await?;
    println!("Started Urspace service `{name}`.");
    service_request(
        name,
        ControlAction::Invite {
            access_label: None,
            max_sessions: None,
            direct: false,
            tcp: false,
        },
    )
    .await
}

async fn restart_managed_service(name: &str) -> Result<()> {
    let _ = service::request(name, ControlAction::Stop).await;
    native_service::restart(name)?;
    await_service_ready(name).await?;
    println!("Restarted Urspace service `{name}`.");
    service_request(
        name,
        ControlAction::Invite {
            access_label: None,
            max_sessions: None,
            direct: false,
            tcp: false,
        },
    )
    .await
}

async fn stop_service(name: &str) -> Result<()> {
    if !native_service::is_installed(name)? {
        return service_request(name, ControlAction::Stop).await;
    }
    let _ = service::request(name, ControlAction::Stop).await;
    native_service::stop(name)?;
    println!("Stopped Urspace service `{name}`. It remains installed.");
    Ok(())
}

async fn uninstall_managed_service(name: &str) -> Result<()> {
    if !native_service::is_installed(name)? {
        bail!("service `{name}` is not installed");
    }
    let _ = service::request(name, ControlAction::Stop).await;
    let definition = native_service::uninstall(name)?;
    println!("Uninstalled Urspace service `{name}` from automatic startup.");
    println!("Removed definition: {}", definition.display());
    println!("Its site identity and browser access state were preserved.");
    Ok(())
}

async fn await_service_ready(name: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    loop {
        if service::request(name, ControlAction::Status)
            .await
            .is_ok_and(|response| response.ok)
        {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!("timed out waiting for service `{name}` to become ready");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn validate_share_settings(settings: &ShareSettings) -> Result<()> {
    if settings.max_sessions == 0 {
        bail!("max-sessions must be greater than zero");
    }
    let ttl_seconds =
        i64::try_from(settings.ttl.as_secs()).context("invitation lifetime is too large")?;
    let identity = SecretKey::generate();
    let ticket = EndpointTicket::new(EndpointAddr::new(identity.public())).to_string();
    sign_invite(
        &identity,
        InviteGrant {
            bootstrap_origin: settings.bootstrap_origin.clone(),
            endpoint_ticket: ticket,
            invite_id: Uuid::new_v4(),
            capability: [0_u8; 32],
            expires_at_unix: unix_now().saturating_add(ttl_seconds),
            entry_path: settings.entry_path.clone(),
            max_sessions: settings.max_sessions,
        },
    )?;
    if settings.short {
        ShortLinkPublisher::new(&settings.short_origin)?;
    }
    Ok(())
}

struct ServiceRuntime {
    endpoint: Endpoint,
    identity: SecretKey,
    registry: CapabilityRegistry,
    bootstrap_origin: String,
    ttl_seconds: i64,
    max_sessions: u32,
    entry_path: String,
    short_links: Option<ShortLinkPublisher>,
    current_invite_id: Uuid,
    current_raw_url: url::Url,
    source_description: String,
}

impl ServiceRuntime {
    async fn handle(&mut self, action: ControlAction) -> Result<(ControlResponse, bool)> {
        match action {
            ControlAction::Status => {
                let mut response = ControlResponse::success("service is running");
                response.site_id = Some(self.identity.public().to_z32());
                response.source = Some(self.source_description.clone());
                response.sessions = self.session_views();
                Ok((response, false))
            }
            ControlAction::Invite {
                access_label,
                max_sessions,
                direct,
                tcp,
            } => {
                let access_label = access_label
                    .as_deref()
                    .map(normalize_access_label)
                    .transpose()
                    .map_err(anyhow::Error::msg)?;
                let invitation_sessions = max_sessions.unwrap_or(self.max_sessions);
                if invitation_sessions == 0 {
                    bail!("max-sessions must be greater than zero");
                }
                let url = self
                    .rotate_invite(access_label.as_deref(), invitation_sessions, !direct, tcp)
                    .await?;
                let message = access_label.as_deref().map_or_else(
                    || "created a fresh invitation; previously admitted browsers remain authorized".to_owned(),
                    |label| format!(
                        "created a {invitation_sessions}-session enrollment for `{label}`; the label is local administrative metadata, not verified identity"
                    ),
                );
                let mut response = ControlResponse::success(message);
                response.share_url = Some(url.to_string());
                Ok((response, false))
            }
            ControlAction::Sessions => {
                let mut response = ControlResponse::success("admitted device sessions");
                response.sessions = self.session_views();
                Ok((response, false))
            }
            ControlAction::Kick {
                session_handle: handle,
            } => {
                let matches: Vec<_> = self
                    .registry
                    .sessions()
                    .into_iter()
                    .filter(|session| session.operator_handle == handle)
                    .collect();
                let [session] = matches.as_slice() else {
                    bail!("no active admitted session has that handle");
                };
                if !self
                    .registry
                    .kick_and_close_admissions(session.session_id, self.current_invite_id)?
                {
                    bail!("no active admitted session has that handle");
                }
                let url = self.replace_closed_invite().await?;
                let mut response = ControlResponse::success(
                    "device session revoked and outstanding invitation rotated",
                );
                response.share_url = Some(url.to_string());
                Ok((response, false))
            }
            ControlAction::KickAll => {
                let count = self
                    .registry
                    .kick_all_and_close_admissions(self.current_invite_id)?;
                let mut response =
                    ControlResponse::success(format!("revoked {count} admitted device session(s)"));
                if count > 0 {
                    response.share_url = Some(self.replace_closed_invite().await?.to_string());
                }
                Ok((response, false))
            }
            ControlAction::Stop => Ok((
                ControlResponse::success("service is stopping gracefully"),
                true,
            )),
        }
    }

    fn session_views(&self) -> Vec<SessionView> {
        self.registry
            .sessions()
            .into_iter()
            .map(|session| SessionView {
                operator_handle: session.operator_handle,
                session_id: "redacted".into(),
                endpoint_id: "redacted".into(),
                connected: session.connected,
                access_label: session.access_label,
            })
            .collect()
    }

    async fn rotate_invite(
        &mut self,
        access_label: Option<&str>,
        max_sessions: u32,
        publish_short_link: bool,
        allow_tcp: bool,
    ) -> Result<url::Url> {
        self.registry.close_admissions(self.current_invite_id)?;
        self.replace_closed_invite_for(access_label, max_sessions, publish_short_link, allow_tcp)
            .await
    }

    async fn replace_closed_invite(&mut self) -> Result<url::Url> {
        self.replace_closed_invite_for(None, self.max_sessions, true, false)
            .await
    }

    async fn replace_closed_invite_for(
        &mut self,
        access_label: Option<&str>,
        max_sessions: u32,
        publish_short_link: bool,
        allow_tcp: bool,
    ) -> Result<url::Url> {
        replace_closed_invite(
            &self.endpoint,
            &self.identity,
            &self.registry,
            &self.bootstrap_origin,
            self.ttl_seconds,
            max_sessions,
            &self.entry_path,
            if publish_short_link {
                self.short_links.as_ref()
            } else {
                None
            },
            access_label,
            allow_tcp,
            &mut self.current_invite_id,
            &mut self.current_raw_url,
        )
        .await
    }
}

async fn run_service(
    upstream: String,
    name: String,
    settings: ShareSettings,
    print_startup_invite: bool,
) -> Result<()> {
    if settings.max_sessions == 0 {
        bail!("max-sessions must be greater than zero");
    }
    let ttl_seconds =
        i64::try_from(settings.ttl.as_secs()).context("invitation lifetime is too large")?;
    let managed_config_path = service::managed_config_path(&name)?;
    if managed_config_path.exists() {
        let requested = ManagedServiceConfig::new(
            upstream.clone(),
            settings.bootstrap_origin.clone(),
            settings.ttl.as_secs(),
            settings.max_sessions,
            settings.entry_path.clone(),
            settings.short,
            settings.short_origin.clone(),
        );
        if service::load_managed_config(&name)? != requested {
            bail!(
                "service `{name}` must use its installed settings to preserve existing browser access"
            );
        }
    }
    let site = LoopbackSite::open(&upstream)?;
    ensure_app_is_listening(site.origin()).await?;
    ControlServer::ensure_available(&name).await?;
    let identity_path = identity_path(None, Some(&name), site.origin())?;
    let identity = load_or_create_identity(&identity_path)?;
    let service_directory = service::service_directory(&name)?;
    let registry = CapabilityRegistry::open(service_directory.join("authorization.jsonl"))?;
    let (endpoint, stable_ticket) =
        bind_service_endpoint(&identity, &service_directory.join("relay.txt")).await?;
    let control = ControlServer::bind(&name, &identity.public().to_z32()).await?;
    registry.close_all_admissions()?;
    let (invite_id, raw_url, expires_at_unix) = mint_invite(
        &endpoint,
        &identity,
        &registry,
        &settings.bootstrap_origin,
        ttl_seconds,
        settings.max_sessions,
        None,
        false,
        &settings.entry_path,
    )?;
    let short_links = settings
        .short
        .then(|| ShortLinkPublisher::new(&settings.short_origin))
        .transpose()?;
    let share_url = if print_startup_invite {
        Some(publish_share_url(&raw_url, expires_at_unix, short_links.as_ref()).await)
    } else {
        None
    };
    let issuer = SessionGrantIssuer::new(
        identity.clone(),
        settings.bootstrap_origin.clone(),
        stable_ticket,
        settings.entry_path.clone(),
    );
    let router = Router::builder(endpoint.clone())
        .accept(
            ALPN,
            SiteProtocol::loopback(registry.clone(), site.clone(), issuer.clone()),
        )
        .accept(
            TUNNEL_ALPN,
            TunnelProtocol::new(registry.clone(), site, issuer),
        )
        .spawn();
    let source_description = format!("app at {}", upstream.trim_end_matches('/'));

    println!("Urspace service `{name}` is running {source_description}");
    if let Some(share_url) = share_url {
        println!("Share URL (treat it as a secret):\n{share_url}\n");
    }
    println!("Site identity: {}", identity.public().to_z32());
    println!("Authorization changes are persisted before they take effect.");
    if print_startup_invite {
        println!("Manage it from another terminal with `urspace service status {name}`.");
        println!("Press Ctrl+C or run `urspace service stop {name}` to stop.\n");
    } else {
        println!("Running as an operating-system user service.\n");
    }

    let mut runtime = ServiceRuntime {
        endpoint,
        identity,
        registry,
        bootstrap_origin: settings.bootstrap_origin,
        ttl_seconds,
        max_sessions: settings.max_sessions,
        entry_path: settings.entry_path,
        short_links,
        current_invite_id: invite_id,
        current_raw_url: raw_url,
        source_description,
    };
    let shutdown = shutdown_signal();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            signal = &mut shutdown => {
                signal?;
                break;
            }
            accepted = control.accept() => {
                let (mut stream, action) = accepted?;
                let result = runtime.handle(action).await;
                let (response, stop) = match result {
                    Ok(result) => result,
                    Err(error) => (ControlResponse::failure(error.to_string()), false),
                };
                // The authenticated action remains authoritative even if the
                // local caller disconnects before reading its response.
                let _ = service::send_response(&mut stream, &response).await;
                if stop {
                    break;
                }
            }
        }
    }
    router.shutdown().await?;
    Ok(())
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("listen for service termination")?;
        tokio::select! {
            signal = tokio::signal::ctrl_c() => signal.context("listen for Ctrl+C"),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.context("listen for Ctrl+C")
    }
}

async fn bind_service_endpoint(
    identity: &SecretKey,
    relay_path: &Path,
) -> Result<(Endpoint, String)> {
    let pinned_relay = match std::fs::read_to_string(relay_path) {
        Ok(raw) => Some(
            raw.trim()
                .parse::<RelayUrl>()
                .context("parse pinned service relay")?,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("read pinned service relay {}", relay_path.display()));
        }
    };
    let mut builder = Endpoint::builder(presets::N0).secret_key(identity.clone());
    if let Some(relay) = &pinned_relay {
        builder = builder.relay_mode(RelayMode::custom([relay.clone()]));
    }
    let endpoint = builder.bind().await.context("bind Iroh service endpoint")?;
    tokio::time::timeout(Duration::from_secs(30), endpoint.online())
        .await
        .context("timed out connecting to the pinned Iroh relay")?;
    let relay = pinned_relay
        .or_else(|| endpoint.addr().relay_urls().next().cloned())
        .context("Iroh service did not select a relay; a relay is required for browser sessions")?;
    if !relay_path.exists() {
        write_private_file(relay_path, relay.to_string().as_bytes())?;
    }
    let stable_addr = EndpointAddr::new(identity.public()).with_relay_url(relay);
    Ok((endpoint, EndpointTicket::new(stable_addr).to_string()))
}

async fn service_request(name: &str, action: ControlAction) -> Result<()> {
    let response = service::request(name, action).await?;
    print_service_response(response)
}

fn print_service_response(response: ControlResponse) -> Result<()> {
    if !response.ok {
        bail!(response.message);
    }
    println!("{}", response.message);
    if let Some(source) = response.source {
        println!("Source: {source}");
    }
    if let Some(site_id) = response.site_id {
        println!("Site identity: {site_id}");
    }
    if response.sessions.is_empty() {
        if matches!(
            response.message.as_str(),
            "admitted device sessions" | "admitted browser sessions"
        ) {
            println!("No device sessions have been admitted.");
        }
    } else {
        for session in response.sessions {
            let state = if session.connected {
                "connected"
            } else {
                "disconnected (may reconnect)"
            };
            let handle = if session.operator_handle.is_empty() {
                "unavailable-restart-service"
            } else {
                &session.operator_handle
            };
            if let Some(label) = session.access_label {
                println!("{label}  {handle}  {state}");
            } else {
                println!("{handle}  {state}");
            }
        }
    }
    if let Some(url) = response.share_url {
        println!("Share URL (treat it as a secret):\n{url}");
    }
    Ok(())
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
        None,
        false,
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
    access_label: Option<&str>,
    allow_tcp: bool,
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
    if allow_tcp {
        registry.insert_for_tcp(
            invite_id,
            &capability,
            expires_at_unix,
            max_sessions,
            access_label.map(str::to_owned),
        )?;
    } else {
        registry.insert_for(
            invite_id,
            &capability,
            expires_at_unix,
            max_sessions,
            access_label.map(str::to_owned),
        )?;
    }
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
                let count = registry.kick_all()?;
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
                if kick_session(&registry, command[5..].trim())? {
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
    // Close the bearer URL before creating its replacement. If signing or
    // persistence fails, admissions fail closed instead of leaving the old
    // invitation usable after an operator requested rotation.
    registry.close_admissions(*current_invite_id)?;
    replace_closed_invite(
        endpoint,
        identity,
        registry,
        bootstrap_origin,
        ttl_seconds,
        max_sessions,
        entry_path,
        short_links,
        None,
        false,
        current_invite_id,
        current_raw_url,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn replace_closed_invite(
    endpoint: &Endpoint,
    identity: &SecretKey,
    registry: &CapabilityRegistry,
    bootstrap_origin: &str,
    ttl_seconds: i64,
    max_sessions: u32,
    entry_path: &str,
    short_links: Option<&ShortLinkPublisher>,
    access_label: Option<&str>,
    allow_tcp: bool,
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
        access_label,
        allow_tcp,
        entry_path,
    )?;
    let share_url = publish_share_url(&raw_url, expires_at_unix, short_links).await;
    *current_invite_id = next_invite_id;
    *current_raw_url = raw_url;
    Ok(share_url)
}

fn print_sessions(registry: &CapabilityRegistry) {
    let sessions = registry.sessions();
    if sessions.is_empty() {
        println!("No device sessions have been admitted.");
        return;
    }
    for session in sessions {
        let state = if session.connected {
            "connected"
        } else {
            "disconnected (may reconnect)"
        };
        if let Some(label) = session.access_label {
            println!("{label}  {}  {state}", session.operator_handle);
        } else {
            println!("{}  {state}", session.operator_handle);
        }
    }
}

fn kick_session(registry: &CapabilityRegistry, prefix: &str) -> Result<bool> {
    if prefix.is_empty() {
        println!("Usage: kick <session-handle-prefix>");
        return Ok(false);
    }
    let matches: Vec<_> = registry
        .sessions()
        .into_iter()
        .filter(|session| session.operator_handle.starts_with(prefix))
        .collect();
    match matches.as_slice() {
        [] => {
            println!("No admitted session matches `{prefix}`.");
            Ok(false)
        }
        [session] => {
            if registry.kick(session.session_id)? {
                println!("Kicked the selected device session.");
                Ok(true)
            } else {
                Ok(false)
            }
        }
        _ => {
            println!("Session prefix `{prefix}` is ambiguous; enter more characters.");
            Ok(false)
        }
    }
}

#[derive(Clone)]
struct LocalGatewayState {
    client: Arc<NativeSiteClient>,
    expected_authority: Arc<str>,
}

async fn connect_site(
    name: String,
    local_endpoint: Option<SocketAddr>,
    mut invitation: Option<String>,
    invite_stdin: bool,
    listen: Option<SocketAddr>,
) -> Result<()> {
    let session_path = service::data_directory()?
        .join("clients")
        .join(&name)
        .join("device.json");
    if invite_stdin {
        let mut line = String::new();
        eprintln!("Paste the one-time Urspace enrollment invitation, then press Enter:");
        std::io::stdin()
            .read_line(&mut line)
            .context("read enrollment invitation")?;
        invitation = Some(line.trim().to_owned());
        line.zeroize();
    }

    let connection_result: Result<(NativeSiteClient, tokio::net::TcpListener)> = async {
        match invitation.as_deref() {
            Some("") => bail!("the enrollment invitation is empty"),
            Some(_) if session_path.exists() => bail!(
                "device `{name}` is already enrolled; choose another name or reconnect without an invitation"
            ),
            Some(raw) => {
                let transport = if local_endpoint.is_some() {
                    NativeTransport::Tcp
                } else {
                    NativeTransport::Web
                };
                let address = local_endpoint
                    .or(listen)
                    .unwrap_or(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)));
                let listener = tokio::net::TcpListener::bind(address)
                    .await
                    .with_context(|| format!("bind native gateway to {address}"))?;
                let local_port = listener.local_addr()?.port();
                let client =
                    NativeSiteClient::enroll(
                        raw,
                        session_path.clone(),
                        local_port,
                        transport,
                        unix_now(),
                    )
                    .await?;
                Ok((client, listener))
            }
            None if session_path.exists() => {
                let (client, listener) = if let Some(address) = local_endpoint {
                    let listener = tokio::net::TcpListener::bind(address)
                        .await
                        .with_context(|| format!("bind local port mount to {address}"))?;
                    let local_port = listener.local_addr()?.port();
                    let client =
                        NativeSiteClient::resume_tcp(session_path.clone(), local_port, unix_now())
                            .await?;
                    (client, listener)
                } else {
                    let client = NativeSiteClient::resume(session_path.clone(), unix_now()).await?;
                    if listen.is_some() && client.transport() == NativeTransport::Tcp {
                        bail!(
                            "--listen configures the browser gateway; pass localhost:<port> to override a TCP mount"
                        );
                    }
                    let address = listen.unwrap_or(SocketAddr::from((
                        Ipv4Addr::LOCALHOST,
                        client.local_port(),
                    )));
                    let listener = tokio::net::TcpListener::bind(address)
                        .await
                        .with_context(|| format!("bind local Urspace endpoint to {address}"))?;
                    (client, listener)
                };
                Ok((client, listener))
            }
            None => bail!(
                "device `{name}` is not enrolled; run `urspace connect {name} --invite-stdin` first"
            ),
        }
    }
    .await;
    if let Some(invitation) = invitation.as_mut() {
        invitation.zeroize();
    }
    let (client, listener) = connection_result?;
    if client.transport() == NativeTransport::Tcp {
        return serve_local_tcp_mount(name, client, listener).await;
    }
    let local_address = listener.local_addr()?;
    let authority = format!(
        "{}.localhost:{}",
        client.local_origin_label(),
        local_address.port()
    );
    let entry_path = client.entry_path().to_owned();
    let site_handle = public_site_handle(client.site_id());
    let state = LocalGatewayState {
        client: Arc::new(client),
        expected_authority: authority.clone().into(),
    };
    let app = AxumRouter::new()
        .fallback(local_gateway_request)
        .with_state(state);

    println!("Urspace connected device `{name}` to site {site_handle}.");
    println!("Private local URL: http://{authority}{entry_path}");
    println!("The device proof key is stored privately and Cloudflare is not used.");
    println!("Press Ctrl+C to close the local gateway.");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .context("run native Urspace gateway")
}

async fn serve_local_tcp_mount(
    name: String,
    client: NativeSiteClient,
    listener: tokio::net::TcpListener,
) -> Result<()> {
    let local_address = listener.local_addr()?;
    let site_handle = public_site_handle(client.site_id());
    let client = Arc::new(client);
    println!(
        "Urspace mounted `{name}` ({site_handle}) at localhost:{}.",
        local_address.port()
    );
    println!("TCP traffic is carried over the encrypted Iroh connection.");
    println!("Cloudflare is not used. Press Ctrl+C to remove the local mount.");

    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal?;
                return Ok(());
            }
            accepted = listener.accept() => {
                let (local, _) = accepted.context("accept local port connection")?;
                let client = Arc::clone(&client);
                tokio::spawn(async move {
                    let _ = bridge_local_tcp(local, client).await;
                });
            }
        }
    }
}

async fn bridge_local_tcp(
    local: tokio::net::TcpStream,
    client: Arc<NativeSiteClient>,
) -> Result<()> {
    let mesh = client.open_tunnel().await?;
    let (mut local_read, mut local_write) = local.into_split();
    let mut mesh_send = mesh.send;
    let mut mesh_recv = mesh.recv;
    let upload = async {
        tokio::io::copy(&mut local_read, &mut mesh_send).await?;
        mesh_send.finish().map_err(std::io::Error::other)
    };
    let download = async {
        tokio::io::copy(&mut mesh_recv, &mut local_write).await?;
        tokio::io::AsyncWriteExt::shutdown(&mut local_write).await
    };
    tokio::try_join!(upload, download)?;
    Ok(())
}

async fn local_gateway_request(
    State(state): State<LocalGatewayState>,
    request: Request<Body>,
) -> Response<Body> {
    let (mut parts, body) = request.into_parts();
    if !local_authority_matches(&parts.headers, &state.expected_authority) {
        return local_error(
            StatusCode::MISDIRECTED_REQUEST,
            "Unknown local Urspace site",
        );
    }
    let path = parts
        .uri
        .path_and_query()
        .map_or_else(|| "/".to_owned(), ToString::to_string);

    if parts
        .headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
    {
        return match WebSocketUpgrade::from_request_parts(&mut parts, &state).await {
            Ok(upgrade) => upgrade
                .on_upgrade(move |socket| bridge_local_socket(socket, state.client, path))
                .into_response(),
            Err(_) => local_error(StatusCode::BAD_REQUEST, "Invalid WebSocket upgrade"),
        };
    }

    let method = match request_method(&parts.method) {
        Some(method) => method,
        None => return local_error(StatusCode::METHOD_NOT_ALLOWED, "Unsupported request method"),
    };
    let headers = forward_request_headers(&parts.headers);
    let body = match to_bytes(body, 16 * 1024 * 1024).await {
        Ok(body) => body.to_vec(),
        Err(_) => return local_error(StatusCode::PAYLOAD_TOO_LARGE, "Request body is too large"),
    };
    match state.client.fetch(method, path, headers, body).await {
        Ok(response) => native_http_response(response),
        Err(_) => local_error(StatusCode::BAD_GATEWAY, "Private site request failed"),
    }
}

fn local_authority_matches(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| host.eq_ignore_ascii_case(expected))
}

fn request_method(method: &axum::http::Method) -> Option<RequestMethod> {
    match *method {
        axum::http::Method::GET => Some(RequestMethod::Get),
        axum::http::Method::HEAD => Some(RequestMethod::Head),
        axum::http::Method::POST => Some(RequestMethod::Post),
        axum::http::Method::PUT => Some(RequestMethod::Put),
        axum::http::Method::PATCH => Some(RequestMethod::Patch),
        axum::http::Method::DELETE => Some(RequestMethod::Delete),
        axum::http::Method::OPTIONS => Some(RequestMethod::Options),
        _ => None,
    }
}

fn forward_request_headers(headers: &HeaderMap) -> Vec<urspace_protocol::Header> {
    headers
        .iter()
        .filter(|(name, _)| !is_gateway_hop_header(name.as_str()))
        .filter_map(|(name, value)| {
            value.to_str().ok().map(|value| urspace_protocol::Header {
                name: name.as_str().to_owned(),
                value: value.to_owned(),
            })
        })
        .collect()
}

fn native_http_response(response: urspace_host::native_client::NativeResponse) -> Response<Body> {
    let mut builder = Response::builder().status(response.status);
    if let Some(content_type) = response.content_type
        && let Ok(value) = axum::http::HeaderValue::from_str(&content_type)
    {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    for item in response.headers {
        if is_gateway_hop_header(&item.name) || item.name.eq_ignore_ascii_case("content-type") {
            continue;
        }
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::from_bytes(item.name.as_bytes()),
            axum::http::HeaderValue::from_str(&item.value),
        ) {
            builder = builder.header(name, value);
        }
    }
    builder = builder
        .header(header::REFERRER_POLICY, "no-referrer")
        .header("x-urspace-native", "1");
    builder.body(Body::from(response.body)).unwrap_or_else(|_| {
        local_error(
            StatusCode::BAD_GATEWAY,
            "Private site returned an invalid response",
        )
    })
}

fn local_error(status: StatusCode, message: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(message))
        .expect("static local error response is valid")
}

fn is_gateway_hop_header(name: &str) -> bool {
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

async fn bridge_local_socket(socket: WebSocket, client: Arc<NativeSiteClient>, path: String) {
    let Ok(mut mesh) = client.open_socket(path).await else {
        let mut socket = socket;
        let _ = socket
            .send(AxumSocketMessage::Close(Some(CloseFrame {
                code: 1011,
                reason: "Private site connection failed".into(),
            })))
            .await;
        return;
    };
    let (mut browser_send, mut browser_recv) = socket.split();
    loop {
        tokio::select! {
            from_browser = browser_recv.next() => {
                let Some(Ok(message)) = from_browser else { break };
                let message = axum_to_socket_message(message);
                let close = matches!(message, urspace_protocol::SocketMessage::Close { .. });
                if send_socket_message(&mut mesh.send, &message).await.is_err() || close {
                    break;
                }
            }
            from_site = receive_socket_message(&mut mesh.recv) => {
                let Ok(message) = from_site else { break };
                let close = matches!(message, urspace_protocol::SocketMessage::Close { .. });
                if browser_send.send(socket_message_to_axum(message)).await.is_err() || close {
                    break;
                }
            }
        }
    }
}

fn axum_to_socket_message(message: AxumSocketMessage) -> urspace_protocol::SocketMessage {
    match message {
        AxumSocketMessage::Text(text) => urspace_protocol::SocketMessage::Text(text.to_string()),
        AxumSocketMessage::Binary(bytes) => urspace_protocol::SocketMessage::Binary(bytes.to_vec()),
        AxumSocketMessage::Ping(bytes) => urspace_protocol::SocketMessage::Ping(bytes.to_vec()),
        AxumSocketMessage::Pong(bytes) => urspace_protocol::SocketMessage::Pong(bytes.to_vec()),
        AxumSocketMessage::Close(frame) => urspace_protocol::SocketMessage::Close {
            code: frame.as_ref().map(|frame| frame.code),
            reason: frame.map_or_else(String::new, |frame| frame.reason.to_string()),
        },
    }
}

fn socket_message_to_axum(message: urspace_protocol::SocketMessage) -> AxumSocketMessage {
    match message {
        urspace_protocol::SocketMessage::Text(text) => AxumSocketMessage::Text(text.into()),
        urspace_protocol::SocketMessage::Binary(bytes) => AxumSocketMessage::Binary(bytes.into()),
        urspace_protocol::SocketMessage::Ping(bytes) => AxumSocketMessage::Ping(bytes.into()),
        urspace_protocol::SocketMessage::Pong(bytes) => AxumSocketMessage::Pong(bytes.into()),
        urspace_protocol::SocketMessage::Close { code, reason } => {
            AxumSocketMessage::Close(code.map(|code| CloseFrame {
                code,
                reason: reason.into(),
            }))
        }
    }
}

fn public_site_handle(site_id: &str) -> String {
    format!("site-{}", &blake3::hash(site_id.as_bytes()).to_hex()[..12])
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

fn normalize_access_label(raw: &str) -> Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() || normalized.len() > 120 || normalized.chars().any(char::is_control) {
        return Err("access label must contain 1-120 printable characters".into());
    }
    Ok(normalized.to_owned())
}

fn normalize_local_mount(raw: &str) -> Result<SocketAddr, String> {
    let normalized = raw
        .trim()
        .strip_prefix("localhost:")
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or_else(|| raw.trim().to_owned());
    let address = normalized
        .parse::<SocketAddr>()
        .map_err(|_| "local endpoint must look like localhost:9090".to_owned())?;
    if address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
        return Err("local endpoint must use localhost or 127.0.0.1 with a fixed port".into());
    }
    Ok(address)
}

fn normalize_connect_listen(raw: &str) -> Result<SocketAddr, String> {
    let address = raw
        .parse::<SocketAddr>()
        .map_err(|_| "listen must be a loopback IP address and port".to_owned())?;
    if address.ip() != Ipv4Addr::LOCALHOST {
        return Err("native gateways may listen only on 127.0.0.1".into());
    }
    Ok(address)
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
    let key = name.map_or_else(
        || blake3::hash(source_key.as_bytes()).to_hex().to_string(),
        str::to_owned,
    );
    Ok(service::data_directory()?
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
        .with_context(|| format!("create private file {}", path.display()))?;
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
    fn named_service_commands_are_explicit_and_loopback_only() {
        let cli = Cli::try_parse_from([
            "urspace",
            "service",
            "install",
            "localhost:8787",
            "--name",
            "BoxClub",
            "--short",
        ])
        .unwrap();
        let Command::Service {
            command: ServiceCommand::Install {
                app, name, short, ..
            },
        } = cli.command
        else {
            panic!("expected service install command");
        };
        assert_eq!(app, "http://127.0.0.1:8787/");
        assert_eq!(name, "boxclub");
        assert!(short);

        let cli = Cli::try_parse_from([
            "urspace",
            "service",
            "run",
            "localhost:8787",
            "--name",
            "BoxClub",
            "--short",
        ])
        .unwrap();
        let Command::Service {
            command: ServiceCommand::Run {
                app, name, short, ..
            },
        } = cli.command
        else {
            panic!("expected service run command");
        };
        assert_eq!(app, "http://127.0.0.1:8787/");
        assert_eq!(name, "boxclub");
        assert!(short);

        assert!(
            Cli::try_parse_from(["urspace", "service", "run", "example.com", "--name", "app"])
                .is_err()
        );
        assert!(Cli::try_parse_from(["urspace", "service", "run", "localhost:8787"]).is_err());
        assert!(
            Cli::try_parse_from([
                "urspace",
                "service",
                "install",
                "example.com",
                "--name",
                "app"
            ])
            .is_err()
        );
        assert!(matches!(
            Cli::try_parse_from(["urspace", "service", "start", "BoxClub"])
                .unwrap()
                .command,
            Command::Service {
                command: ServiceCommand::Start { name }
            } if name == "boxclub"
        ));
        assert!(matches!(
            Cli::try_parse_from(["urspace", "service", "managed-run", "BoxClub"])
                .unwrap()
                .command,
            Command::Service {
                command: ServiceCommand::ManagedRun { name }
            } if name == "boxclub"
        ));
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

    #[test]
    fn named_access_invites_default_to_one_session() {
        let cli = Cli::try_parse_from([
            "urspace",
            "service",
            "invite",
            "BoxClub",
            "--for",
            " Alice / work laptop ",
        ])
        .unwrap();
        let Command::Service {
            command:
                ServiceCommand::Invite {
                    name,
                    access_label,
                    max_sessions,
                    direct,
                    tcp,
                },
        } = cli.command
        else {
            panic!("expected service invite command");
        };
        assert_eq!(name, "boxclub");
        assert_eq!(access_label.as_deref(), Some("Alice / work laptop"));
        assert_eq!(max_sessions, None);
        assert!(!direct);
        assert!(!tcp);
        assert!(normalize_access_label("bad\nlabel").is_err());

        let cli = Cli::try_parse_from([
            "urspace",
            "service",
            "invite",
            "boxclub",
            "--for",
            "Alice / work laptop",
            "--tcp",
        ])
        .unwrap();
        let Command::Service {
            command: ServiceCommand::Invite { direct, tcp, .. },
        } = cli.command
        else {
            panic!("expected service invite command");
        };
        assert!(!direct);
        assert!(tcp);
    }

    #[test]
    fn native_connect_accepts_only_loopback_gateways() {
        let cli = Cli::try_parse_from([
            "urspace",
            "connect",
            "BoxClub",
            "--invite-stdin",
            "--listen",
            "127.0.0.1:8080",
        ])
        .unwrap();
        let Command::Connect {
            name,
            local_endpoint,
            invite,
            invite_stdin,
            listen,
        } = cli.command
        else {
            panic!("expected connect command");
        };
        assert_eq!(name, "boxclub");
        assert!(local_endpoint.is_none());
        assert!(invite.is_none());
        assert!(invite_stdin);
        assert_eq!(listen, Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 8080))));
        assert!(
            Cli::try_parse_from(["urspace", "connect", "boxclub", "--listen", "0.0.0.0:8080"])
                .is_err()
        );

        let cli = Cli::try_parse_from([
            "urspace",
            "connect",
            "boxclub",
            "localhost:9090",
            "--invite-stdin",
        ])
        .unwrap();
        let Command::Connect { local_endpoint, .. } = cli.command else {
            panic!("expected connect command");
        };
        assert_eq!(
            local_endpoint,
            Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 9090)))
        );
        assert!(Cli::try_parse_from(["urspace", "connect", "boxclub", "0.0.0.0:9090"]).is_err());
    }

    #[test]
    fn local_gateway_requires_the_exact_random_authority() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "token.localhost:8080".parse().unwrap());
        assert!(local_authority_matches(&headers, "token.localhost:8080"));
        assert!(!local_authority_matches(
            &headers,
            "different.localhost:8080"
        ));
        assert!(!local_authority_matches(&headers, "token.localhost:8081"));
    }

    #[test]
    fn native_websocket_messages_preserve_payloads_and_close_codes() {
        let binary = urspace_protocol::SocketMessage::Binary(vec![1, 2, 3]);
        assert_eq!(
            axum_to_socket_message(socket_message_to_axum(binary.clone())),
            binary
        );
        let close = urspace_protocol::SocketMessage::Close {
            code: Some(1008),
            reason: "revoked".into(),
        };
        assert_eq!(
            axum_to_socket_message(socket_message_to_axum(close.clone())),
            close
        );
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
