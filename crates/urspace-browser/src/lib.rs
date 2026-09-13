#![cfg_attr(not(target_family = "wasm"), allow(dead_code))]

use std::{
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use iroh::{Endpoint, EndpointAddr, RelayMode, RelayUrl, TransportAddr, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use urspace_protocol::{
    ClientHello, Header, RequestMethod, ServerHello, SiteRequest, SiteResponseHead, SocketMessage,
    alpn_for_invite, read_frame, verify_invite_url, write_frame,
};
use uuid::Uuid;
use wasm_bindgen::{JsError, JsValue, prelude::wasm_bindgen};
use zeroize::{Zeroize, Zeroizing};

const MAX_BOOTSTRAP_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
const SITE_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const SITE_AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Serialize)]
struct BrowserSiteResponse {
    status: u16,
    content_type: Option<String>,
    headers: Vec<Header>,
    body: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct BrowserHeader {
    name: String,
    value: String,
}

#[wasm_bindgen]
pub struct SiteClient {
    endpoint: Endpoint,
    endpoint_addr: EndpointAddr,
    connection: Mutex<iroh::endpoint::Connection>,
    invite_id: Uuid,
    invite_version: u8,
    capability: Zeroizing<[u8; 32]>,
    closed: AtomicBool,
    entry_path: String,
    site_id: String,
}

#[wasm_bindgen]
impl SiteClient {
    #[wasm_bindgen(js_name = connect)]
    pub async fn connect(mut invitation_url: String, now_unix: f64) -> Result<Self, JsError> {
        if !now_unix.is_finite() || now_unix < 0.0 || now_unix > i64::MAX as f64 {
            invitation_url.zeroize();
            return Err(JsError::new("browser clock is outside the supported range"));
        }
        let now_unix = now_unix.floor() as i64;
        let mut invite = match verify_invite_url(&invitation_url, now_unix) {
            Ok(invite) => invite,
            Err(error) => {
                invitation_url.zeroize();
                return Err(JsError::new(&format!("invitation rejected: {error}")));
            }
        };
        invitation_url.zeroize();

        let capability = Zeroizing::new(std::mem::take(&mut invite.capability));
        let ticket = EndpointTicket::from_str(&invite.endpoint_ticket)
            .map_err(|error| JsError::new(&format!("invalid endpoint ticket: {error}")))?;
        let endpoint_addr = normalize_relay_hosts(ticket.endpoint_addr())
            .map_err(|error| JsError::new(&format!("invalid relay address: {error}")))?;
        let relay_urls: Vec<_> = endpoint_addr.relay_urls().cloned().collect();
        if relay_urls.is_empty() {
            return Err(JsError::new("invitation does not contain a browser relay"));
        }
        let endpoint = Endpoint::builder(presets::N0)
            .relay_mode(RelayMode::custom(relay_urls))
            .bind()
            .await
            .map_err(|error| JsError::new(&format!("could not start Iroh: {error}")))?;
        let connection = match connect_and_authorize(
            &endpoint,
            &endpoint_addr,
            invite.version,
            invite.invite_id,
            &capability,
        )
        .await
        {
            Ok(connection) => connection,
            Err(error) => {
                endpoint.close().await;
                return Err(error);
            }
        };

        Ok(Self {
            endpoint,
            endpoint_addr,
            connection: Mutex::new(connection),
            invite_id: invite.invite_id,
            invite_version: invite.version,
            capability,
            closed: AtomicBool::new(false),
            entry_path: invite.entry_path,
            site_id: invite.site_id,
        })
    }

    #[wasm_bindgen(getter, js_name = entryPath)]
    pub fn entry_path(&self) -> String {
        self.entry_path.clone()
    }

    #[wasm_bindgen(getter, js_name = siteId)]
    pub fn site_id(&self) -> String {
        self.site_id.clone()
    }

    pub async fn fetch(
        &self,
        method: String,
        path: String,
        headers: JsValue,
        body: Vec<u8>,
    ) -> Result<JsValue, JsError> {
        let method = parse_method(&method)?;
        let headers: Vec<BrowserHeader> = serde_wasm_bindgen::from_value(headers)
            .map_err(|error| JsError::new(&format!("invalid request headers: {error}")))?;
        let (mut send, mut recv) = self.open_stream("request").await?;
        write_frame(
            &mut send,
            &SiteRequest {
                method,
                path,
                headers: headers
                    .into_iter()
                    .map(|header| Header {
                        name: header.name,
                        value: header.value,
                    })
                    .collect(),
                body_length: body.len() as u64,
            },
        )
        .await
        .map_err(|error| JsError::new(&format!("request failed: {error}")))?;
        tokio::io::AsyncWriteExt::write_all(&mut send, &body)
            .await
            .map_err(|error| JsError::new(&format!("request body failed: {error}")))?;
        send.finish()
            .map_err(|error| JsError::new(&format!("request failed: {error}")))?;
        let head: SiteResponseHead = read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("response failed: {error}")))?;
        let body = recv
            .read_to_end(MAX_BOOTSTRAP_RESPONSE_BYTES)
            .await
            .map_err(|error| JsError::new(&format!("response body failed: {error}")))?;
        serde_wasm_bindgen::to_value(&BrowserSiteResponse {
            status: head.status,
            content_type: head.content_type,
            headers: head.headers,
            body,
        })
        .map_err(|error| JsError::new(&format!("response conversion failed: {error}")))
    }

    #[wasm_bindgen(js_name = openSocket)]
    pub async fn open_socket(&self, path: String) -> Result<SiteSocket, JsError> {
        let (mut send, mut recv) = self.open_stream("socket").await?;
        write_frame(
            &mut send,
            &SiteRequest {
                method: RequestMethod::WebSocket,
                path,
                headers: Vec::new(),
                body_length: 0,
            },
        )
        .await
        .map_err(|error| JsError::new(&format!("socket request failed: {error}")))?;
        let head: SiteResponseHead = read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("socket response failed: {error}")))?;
        if head.status != 101 {
            return Err(JsError::new(&format!(
                "site returned socket status {}",
                head.status
            )));
        }
        Ok(SiteSocket {
            send: Mutex::new(send),
            recv: Mutex::new(recv),
        })
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(connection) = self.connection.try_lock() {
            connection.close(0_u8.into(), b"browser closed");
        }
    }
}

impl SiteClient {
    async fn open_stream(
        &self,
        kind: &str,
    ) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream), JsError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(JsError::new("browser session is closed"));
        }
        let connection = self.connection.lock().await.clone();
        if let Ok(stream) = connection.open_bi().await {
            return Ok(stream);
        }

        let mut current = self.connection.lock().await;
        if current.stable_id() != connection.stable_id()
            && let Ok(stream) = current.open_bi().await
        {
            return Ok(stream);
        }
        let replacement = connect_and_authorize(
            &self.endpoint,
            &self.endpoint_addr,
            self.invite_version,
            self.invite_id,
            &self.capability,
        )
        .await?;
        if self.closed.load(Ordering::Acquire) {
            replacement.close(0_u8.into(), b"browser closed");
            return Err(JsError::new("browser session is closed"));
        }
        *current = replacement;
        current
            .open_bi()
            .await
            .map_err(|error| JsError::new(&format!("{kind} stream failed: {error}")))
    }
}

async fn connect_and_authorize(
    endpoint: &Endpoint,
    endpoint_addr: &EndpointAddr,
    invite_version: u8,
    invite_id: Uuid,
    capability: &[u8; 32],
) -> Result<iroh::endpoint::Connection, JsError> {
    let alpn = alpn_for_invite(invite_version)
        .map_err(|error| JsError::new(&format!("unsupported invitation: {error}")))?;
    let connection = match n0_future::time::timeout(
        SITE_CONNECT_TIMEOUT,
        endpoint.connect(endpoint_addr.clone(), alpn),
    )
    .await
    {
        Ok(Ok(connection)) => connection,
        Ok(Err(error)) => return Err(JsError::new(&format!("could not reach site: {error}"))),
        Err(_) => {
            return Err(JsError::new(
                "Iroh relay connection timed out before the site could be reached",
            ));
        }
    };

    let authorize = async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|error| JsError::new(&format!("authorization stream failed: {error}")))?;
        write_frame(
            &mut send,
            &ClientHello {
                version: invite_version,
                invite_id,
                capability: *capability,
            },
        )
        .await
        .map_err(|error| JsError::new(&format!("authorization request failed: {error}")))?;
        send.finish()
            .map_err(|error| JsError::new(&format!("authorization request failed: {error}")))?;
        read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("authorization response failed: {error}")))
    };
    let reply: ServerHello =
        match n0_future::time::timeout(SITE_AUTHORIZATION_TIMEOUT, authorize).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(error)) => {
                connection.close(0_u8.into(), b"authorization failed");
                return Err(error);
            }
            Err(_) => {
                connection.close(0_u8.into(), b"authorization timed out");
                return Err(JsError::new("site authorization timed out"));
            }
        };
    match reply {
        ServerHello::Granted { .. } => Ok(connection),
        ServerHello::Denied { code } => {
            connection.close(0_u8.into(), b"authorization denied");
            Err(JsError::new(&format!("site denied invitation: {code:?}")))
        }
    }
}

fn normalize_relay_hosts(endpoint_addr: &EndpointAddr) -> Result<EndpointAddr, String> {
    let addrs = endpoint_addr
        .addrs
        .iter()
        .map(|addr| match addr {
            TransportAddr::Relay(relay_url) => {
                normalize_relay_host(relay_url).map(TransportAddr::Relay)
            }
            addr => Ok(addr.clone()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EndpointAddr::from_parts(endpoint_addr.id, addrs))
}

fn normalize_relay_host(relay_url: &RelayUrl) -> Result<RelayUrl, String> {
    let mut url: url::Url = relay_url.clone().into();
    let Some(host) = url.host_str().map(ToOwned::to_owned) else {
        return Ok(relay_url.clone());
    };
    let normalized = host.trim_end_matches('.');
    if normalized == host {
        return Ok(relay_url.clone());
    }
    if normalized.is_empty() {
        return Err("relay hostname is empty after normalization".to_string());
    }
    url.set_host(Some(normalized))
        .map_err(|_| "relay hostname could not be normalized".to_string())?;
    Ok(RelayUrl::from(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_terminal_dot_from_relay_hosts_for_webkit() {
        let relay: RelayUrl = "https://usw1-1.relay.n0.iroh.link./".parse().unwrap();
        let endpoint = EndpointAddr::new(iroh::SecretKey::from_bytes(&[7_u8; 32]).public())
            .with_relay_url(relay);

        let normalized = normalize_relay_hosts(&endpoint).unwrap();

        assert_eq!(
            normalized.relay_urls().next().unwrap().to_string(),
            "https://usw1-1.relay.n0.iroh.link/"
        );
    }

    #[test]
    fn leaves_already_normalized_relay_hosts_unchanged() {
        let relay: RelayUrl = "https://relay.example.com/".parse().unwrap();
        assert_eq!(normalize_relay_host(&relay).unwrap(), relay);
    }
}

#[wasm_bindgen]
pub struct SiteSocket {
    send: Mutex<iroh::endpoint::SendStream>,
    recv: Mutex<iroh::endpoint::RecvStream>,
}

#[wasm_bindgen]
impl SiteSocket {
    #[wasm_bindgen(js_name = sendText)]
    pub async fn send_text(&self, text: String) -> Result<(), JsError> {
        self.send_message(SocketMessage::Text(text)).await
    }

    #[wasm_bindgen(js_name = sendBinary)]
    pub async fn send_binary(&self, bytes: Vec<u8>) -> Result<(), JsError> {
        self.send_message(SocketMessage::Binary(bytes)).await
    }

    pub async fn receive(&self) -> Result<JsValue, JsError> {
        let message: SocketMessage = read_frame(&mut *self.recv.lock().await)
            .await
            .map_err(|error| JsError::new(&format!("socket receive failed: {error}")))?;
        serde_wasm_bindgen::to_value(&message)
            .map_err(|error| JsError::new(&format!("socket conversion failed: {error}")))
    }

    pub async fn close(&self, code: Option<u16>, reason: String) -> Result<(), JsError> {
        self.send_message(SocketMessage::Close { code, reason })
            .await
    }
}

impl SiteSocket {
    async fn send_message(&self, message: SocketMessage) -> Result<(), JsError> {
        write_frame(&mut *self.send.lock().await, &message)
            .await
            .map_err(|error| JsError::new(&format!("socket send failed: {error}")))
    }
}

fn parse_method(method: &str) -> Result<RequestMethod, JsError> {
    match method {
        "GET" => Ok(RequestMethod::Get),
        "HEAD" => Ok(RequestMethod::Head),
        "POST" => Ok(RequestMethod::Post),
        "PUT" => Ok(RequestMethod::Put),
        "PATCH" => Ok(RequestMethod::Patch),
        "DELETE" => Ok(RequestMethod::Delete),
        "OPTIONS" => Ok(RequestMethod::Options),
        _ => Err(JsError::new("unsupported request method")),
    }
}
