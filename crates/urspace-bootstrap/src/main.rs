use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, Response, StatusCode, Uri};
use axum::routing::any;
use clap::Parser;
use iroh_base::PublicKey;

const CSP: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self' https: wss:; style-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
const SERVICE_WORKER_ALLOWED: HeaderName = HeaderName::from_static("service-worker-allowed");
const PERMISSIONS_POLICY: HeaderName = HeaderName::from_static("permissions-policy");
const CROSS_ORIGIN_OPENER_POLICY: HeaderName =
    HeaderName::from_static("cross-origin-opener-policy");
const CROSS_ORIGIN_RESOURCE_POLICY: HeaderName =
    HeaderName::from_static("cross-origin-resource-policy");

#[derive(Debug, Parser)]
#[command(
    name = "urspace-bootstrap",
    about = "Hardened wildcard bootstrap server for Urspace"
)]
struct Cli {
    #[arg(long)]
    base_domain: String,
    #[arg(long, default_value = "0.0.0.0:8080")]
    listen: SocketAddr,
    #[arg(long, default_value = "apps/bootstrap/public")]
    root: PathBuf,
}

#[derive(Debug, Clone)]
struct AppState {
    base_domain: Arc<str>,
    assets: BootstrapAssets,
}

#[derive(Debug, Clone)]
struct BootstrapAssets {
    root: Arc<PathBuf>,
}

impl BootstrapAssets {
    async fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = tokio::fs::canonicalize(root.as_ref())
            .await
            .with_context(|| format!("resolve bootstrap root {}", root.as_ref().display()))?;
        if !tokio::fs::metadata(&root).await?.is_dir() {
            bail!("bootstrap root is not a directory: {}", root.display());
        }
        for asset in Asset::ALL {
            let resolved = tokio::fs::canonicalize(root.join(asset.relative_path()))
                .await
                .with_context(|| format!("resolve bootstrap asset {}", asset.relative_path()))?;
            if !resolved.starts_with(&root) || !tokio::fs::metadata(&resolved).await?.is_file() {
                bail!("bootstrap asset escapes root: {}", asset.relative_path());
            }
        }
        Ok(Self {
            root: Arc::new(root),
        })
    }

    async fn read(&self, asset: Asset) -> Result<Vec<u8>> {
        let path = tokio::fs::canonicalize(self.root.join(asset.relative_path())).await?;
        if !path.starts_with(self.root.as_ref()) {
            bail!("bootstrap asset escaped configured root");
        }
        tokio::fs::read(path).await.context("read bootstrap asset")
    }
}

#[derive(Debug, Clone, Copy)]
enum Asset {
    Open,
    Main,
    SocketShim,
    Style,
    ServiceWorker,
    WasmGlue,
    WasmModule,
}

impl Asset {
    const ALL: [Self; 7] = [
        Self::Open,
        Self::Main,
        Self::SocketShim,
        Self::Style,
        Self::ServiceWorker,
        Self::WasmGlue,
        Self::WasmModule,
    ];

    fn from_uri_path(path: &str) -> Option<Self> {
        match path {
            "/.urspace/open/" | "/.medousa/open/" => Some(Self::Open),
            "/.urspace/assets/main.js" | "/.medousa/assets/main.js" => Some(Self::Main),
            "/.urspace/assets/socket-shim.js" | "/.medousa/assets/socket-shim.js" => {
                Some(Self::SocketShim)
            }
            "/.urspace/assets/style.css" | "/.medousa/assets/style.css" => Some(Self::Style),
            "/sw.js" => Some(Self::ServiceWorker),
            "/.urspace/wasm/urspace_browser.js"
            | "/.medousa/wasm/medousa_site_browser.js"
            | "/.medousa/wasm/urspace_browser.js" => Some(Self::WasmGlue),
            "/.urspace/wasm/urspace_browser_bg.wasm"
            | "/.medousa/wasm/medousa_site_browser_bg.wasm"
            | "/.medousa/wasm/urspace_browser_bg.wasm" => Some(Self::WasmModule),
            _ => None,
        }
    }

    fn relative_path(self) -> &'static str {
        match self {
            Self::Open => ".urspace/open/index.html",
            Self::Main => ".urspace/assets/main.js",
            Self::SocketShim => ".urspace/assets/socket-shim.js",
            Self::Style => ".urspace/assets/style.css",
            Self::ServiceWorker => "sw.js",
            Self::WasmGlue => ".urspace/wasm/urspace_browser.js",
            Self::WasmModule => ".urspace/wasm/urspace_browser_bg.wasm",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Open => "text/html; charset=utf-8",
            Self::Main | Self::SocketShim | Self::ServiceWorker | Self::WasmGlue => {
                "text/javascript; charset=utf-8"
            }
            Self::Style => "text/css; charset=utf-8",
            Self::WasmModule => "application/wasm",
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let base_domain: Arc<str> = normalize_base_domain(&cli.base_domain)?.into();
    let assets = BootstrapAssets::open(&cli.root).await?;
    let state = AppState {
        base_domain: Arc::clone(&base_domain),
        assets,
    };
    let app = Router::new().fallback(any(handle)).with_state(state);
    let listener = tokio::net::TcpListener::bind(cli.listen)
        .await
        .with_context(|| format!("bind bootstrap listener {}", cli.listen))?;
    println!(
        "Bootstrap listening on {} for *.{}",
        cli.listen, base_domain
    );
    axum::serve(listener, app)
        .await
        .context("serve bootstrap")?;
    Ok(())
}

async fn handle(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response<Body> {
    if method != Method::GET && method != Method::HEAD {
        return response(
            StatusCode::METHOD_NOT_ALLOWED,
            "text/plain; charset=utf-8",
            Vec::new(),
            false,
        );
    }
    if uri.path() == "/healthz" {
        return response(
            StatusCode::OK,
            "text/plain; charset=utf-8",
            if method == Method::HEAD {
                Vec::new()
            } else {
                b"ok\n".to_vec()
            },
            false,
        );
    }
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !valid_site_host(host, &state.base_domain) {
        return response(
            StatusCode::MISDIRECTED_REQUEST,
            "text/plain; charset=utf-8",
            b"invalid site host\n".to_vec(),
            false,
        );
    }
    let Some(asset) = Asset::from_uri_path(uri.path()) else {
        return response(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"not found\n".to_vec(),
            false,
        );
    };
    match state.assets.read(asset).await {
        Ok(bytes) => response(
            StatusCode::OK,
            asset.content_type(),
            if method == Method::HEAD {
                Vec::new()
            } else {
                bytes
            },
            matches!(asset, Asset::ServiceWorker),
        ),
        Err(_) => response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "text/plain; charset=utf-8",
            b"bootstrap asset unavailable\n".to_vec(),
            false,
        ),
    }
}

fn response(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
    service_worker: bool,
) -> Response<Body> {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        CROSS_ORIGIN_OPENER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        CROSS_ORIGIN_RESOURCE_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        PERMISSIONS_POLICY,
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    if service_worker {
        headers.insert(SERVICE_WORKER_ALLOWED, HeaderValue::from_static("/"));
    }
    response
}

fn normalize_base_domain(raw: &str) -> Result<String> {
    let domain = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty()
        || domain.contains(['/', ':', '@'])
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        bail!("base-domain must be a plain DNS name without scheme, path, or port");
    }
    Ok(domain)
}

fn valid_site_host(raw_host: &str, base_domain: &str) -> bool {
    let Ok(authority) = raw_host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    let host = authority.host().trim_end_matches('.').to_ascii_lowercase();
    let Some(site_id) = host.strip_suffix(&format!(".{base_domain}")) else {
        return false;
    };
    if site_id.is_empty() || site_id.contains('.') {
        return false;
    }
    PublicKey::from_z32(site_id).is_ok_and(|key| key.to_z32() == site_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh_base::SecretKey;

    async fn fixture_state(base_domain: &str) -> (tempfile::TempDir, AppState) {
        let temp = tempfile::tempdir().unwrap();
        for asset in Asset::ALL {
            let path = temp.path().join(asset.relative_path());
            tokio::fs::create_dir_all(path.parent().unwrap())
                .await
                .unwrap();
            tokio::fs::write(path, b"fixture").await.unwrap();
        }
        let state = AppState {
            base_domain: Arc::from(base_domain),
            assets: BootstrapAssets::open(temp.path()).await.unwrap(),
        };
        (temp, state)
    }

    #[test]
    fn accepts_only_canonical_site_identity_subdomains() {
        let site_id = SecretKey::generate().public().to_z32();
        assert!(valid_site_host(
            &format!("{site_id}.sites.example:443"),
            "sites.example"
        ));
        assert!(!valid_site_host("sites.example", "sites.example"));
        assert!(!valid_site_host("not-a-key.sites.example", "sites.example"));
        assert!(!valid_site_host(
            &format!("extra.{site_id}.sites.example"),
            "sites.example"
        ));
        assert!(!valid_site_host(
            &format!("{site_id}.attacker.example"),
            "sites.example"
        ));
    }

    #[test]
    fn base_domain_rejects_url_or_authority_syntax() {
        assert_eq!(
            normalize_base_domain("Sites.Example.").unwrap(),
            "sites.example"
        );
        assert!(normalize_base_domain("https://sites.example").is_err());
        assert!(normalize_base_domain("sites.example:443").is_err());
        assert!(normalize_base_domain("-bad.example").is_err());
    }

    #[test]
    fn serves_only_the_explicit_bootstrap_surface() {
        assert!(Asset::from_uri_path("/.urspace/open/").is_some());
        assert!(Asset::from_uri_path("/sw.js").is_some());
        assert!(Asset::from_uri_path("/.urspace/wasm/urspace_browser_bg.wasm").is_some());
        assert!(Asset::from_uri_path("/.medousa/open/").is_some());
        assert!(Asset::from_uri_path("/").is_none());
        assert!(Asset::from_uri_path("/.urspace/wasm/../../secret").is_none());
        assert!(Asset::from_uri_path("/.urspace/open/index.html").is_none());
    }

    #[tokio::test]
    async fn handler_enforces_host_allowlist_and_security_headers() {
        let (_temp, state) = fixture_state("sites.example").await;
        let site_id = SecretKey::generate().public().to_z32();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_str(&format!("{site_id}.sites.example")).unwrap(),
        );
        let response = handle(
            State(state.clone()),
            Method::GET,
            Uri::from_static("/sw.js"),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[SERVICE_WORKER_ALLOWED], "/");
        assert!(
            response.headers()[header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .contains("'wasm-unsafe-eval'")
        );
        assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");

        let mut invalid_headers = HeaderMap::new();
        invalid_headers.insert(
            header::HOST,
            HeaderValue::from_static("attacker.sites.example"),
        );
        let response = handle(
            State(state),
            Method::GET,
            Uri::from_static("/.urspace/open/"),
            invalid_headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    }
}
