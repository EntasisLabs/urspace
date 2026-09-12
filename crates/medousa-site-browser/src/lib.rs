#![cfg_attr(not(target_family = "wasm"), allow(dead_code))]

use std::str::FromStr;

use iroh::{Endpoint, endpoint::presets};
use iroh_tickets::endpoint::EndpointTicket;
use medousa_site_protocol::{
    ALPN, ClientHello, INVITE_VERSION, RequestMethod, ServerHello, SiteRequest, SiteResponseHead,
    read_frame, verify_invite_url, write_frame,
};
use serde::Serialize;
use wasm_bindgen::{JsError, JsValue, prelude::wasm_bindgen};
use zeroize::Zeroize;

const MAX_BOOTSTRAP_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Serialize)]
struct BrowserSiteResponse {
    status: u16,
    content_type: Option<String>,
    body: Vec<u8>,
}

#[wasm_bindgen]
pub struct SiteClient {
    _endpoint: Endpoint,
    connection: iroh::endpoint::Connection,
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

        let ticket = EndpointTicket::from_str(&invite.endpoint_ticket)
            .map_err(|error| JsError::new(&format!("invalid endpoint ticket: {error}")))?;
        let endpoint = Endpoint::bind(presets::N0)
            .await
            .map_err(|error| JsError::new(&format!("could not start Iroh: {error}")))?;
        let connection = endpoint
            .connect(ticket.endpoint_addr().clone(), ALPN)
            .await
            .map_err(|error| JsError::new(&format!("could not reach site: {error}")))?;

        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|error| JsError::new(&format!("authorization stream failed: {error}")))?;
        write_frame(
            &mut send,
            &ClientHello {
                version: INVITE_VERSION,
                invite_id: invite.invite_id,
                capability: invite.capability,
            },
        )
        .await
        .map_err(|error| JsError::new(&format!("authorization request failed: {error}")))?;
        invite.capability.zeroize();
        send.finish()
            .map_err(|error| JsError::new(&format!("authorization request failed: {error}")))?;
        let reply: ServerHello = read_frame(&mut recv)
            .await
            .map_err(|error| JsError::new(&format!("authorization response failed: {error}")))?;
        if let ServerHello::Denied { code } = reply {
            return Err(JsError::new(&format!("site denied invitation: {code:?}")));
        }

        Ok(Self {
            _endpoint: endpoint,
            connection,
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

    pub async fn fetch(&self, path: String) -> Result<JsValue, JsError> {
        let (mut send, mut recv) = self
            .connection
            .open_bi()
            .await
            .map_err(|error| JsError::new(&format!("request stream failed: {error}")))?;
        write_frame(
            &mut send,
            &SiteRequest {
                method: RequestMethod::Get,
                path,
            },
        )
        .await
        .map_err(|error| JsError::new(&format!("request failed: {error}")))?;
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
            body,
        })
        .map_err(|error| JsError::new(&format!("response conversion failed: {error}")))
    }

    pub fn close(&self) {
        self.connection.close(0_u8.into(), b"browser closed");
    }
}
