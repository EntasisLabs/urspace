use aes_gcm::{Aes256Gcm, KeyInit as _, Nonce, aead::Aead as _};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hkdf::Hkdf;
use rand::Rng as _;
use serde::Serialize;
use sha2::Sha256;
use url::Url;
use zeroize::Zeroizing;

pub const DEFAULT_SHORT_ORIGIN: &str = "https://u.urspace.online";

const SHORT_LINK_VERSION: u8 = 1;
const HKDF_SALT: &[u8] = b"urspace-short-link-v1";
const LOOKUP_INFO: &[u8] = b"lookup";
const ENCRYPTION_INFO: &[u8] = b"encryption";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortLinkEnvelope {
    version: u8,
    expires_at_unix: i64,
    nonce: String,
    ciphertext: String,
}

#[derive(Debug, Clone)]
pub struct ShortLinkPublisher {
    client: reqwest::Client,
    origin: Url,
}

impl ShortLinkPublisher {
    pub fn new(origin: &str) -> Result<Self> {
        let mut origin = Url::parse(origin).context("parse short-link origin")?;
        if origin.scheme() != "https"
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            bail!("short-link origin must be a credential-free HTTPS origin");
        }
        origin.set_path("/");
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .context("build short-link client")?,
            origin,
        })
    }

    pub async fn publish(&self, invite: &Url, expires_at_unix: i64) -> Result<Url> {
        self.validate_invite_destination(invite)?;
        let seed = Zeroizing::new(rand::rng().random::<[u8; 32]>());
        let nonce: [u8; 12] = rand::rng().random();
        let (lookup, envelope) = seal_invite(invite.as_str(), expires_at_unix, &seed, nonce)?;
        let endpoint = self
            .origin
            .join(&format!("/api/short-links/{lookup}"))
            .context("build short-link upload URL")?;
        let response = self
            .client
            .post(endpoint)
            .header(reqwest::header::CACHE_CONTROL, "no-store")
            .json(&envelope)
            .send()
            .await
            .context("upload encrypted short link")?;
        if !response.status().is_success() {
            let status = response.status();
            let detail = response.text().await.unwrap_or_default();
            bail!(
                "short-link service returned {status}: {}",
                detail.trim().chars().take(160).collect::<String>()
            );
        }

        let mut short_url = self.origin.clone();
        short_url.set_fragment(Some(&format!(
            "s1={}",
            URL_SAFE_NO_PAD.encode(seed.as_ref())
        )));
        Ok(short_url)
    }

    fn validate_invite_destination(&self, invite: &Url) -> Result<()> {
        let short_host = self.origin.host_str().expect("validated short-link origin");
        let base_domain = short_host
            .strip_prefix("u.")
            .context("short-link hostname must begin with `u.`")?;
        let invite_host = invite
            .host_str()
            .context("invitation URL has no hostname")?;
        let site_label = invite_host
            .strip_suffix(&format!(".{base_domain}"))
            .context("short-link and invitation base domains do not match")?;
        if invite.scheme() != "https"
            || invite.port().is_some()
            || site_label.contains('.')
            || site_label.len() != 52
        {
            bail!("short links require a canonical HTTPS Urspace invitation");
        }
        Ok(())
    }
}

fn seal_invite(
    invite: &str,
    expires_at_unix: i64,
    seed: &[u8; 32],
    nonce: [u8; 12],
) -> Result<(String, ShortLinkEnvelope)> {
    let hkdf = Hkdf::<Sha256>::new(Some(HKDF_SALT), seed);
    let mut lookup = [0_u8; 16];
    let mut encryption_key = Zeroizing::new([0_u8; 32]);
    hkdf.expand(LOOKUP_INFO, &mut lookup)
        .map_err(|_| anyhow::anyhow!("derive short-link lookup key"))?;
    hkdf.expand(ENCRYPTION_INFO, encryption_key.as_mut())
        .map_err(|_| anyhow::anyhow!("derive short-link encryption key"))?;

    let aad = short_link_aad(expires_at_unix);
    let ciphertext = Aes256Gcm::new_from_slice(encryption_key.as_ref())
        .expect("AES-256 key has fixed length")
        .encrypt(
            Nonce::from_slice(&nonce),
            aes_gcm::aead::Payload {
                msg: invite.as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("encrypt invitation"))?;

    Ok((
        URL_SAFE_NO_PAD.encode(lookup),
        ShortLinkEnvelope {
            version: SHORT_LINK_VERSION,
            expires_at_unix,
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        },
    ))
}

fn short_link_aad(expires_at_unix: i64) -> String {
    format!("urspace-short-link-v1:{expires_at_unix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealing_has_a_stable_cross_language_vector() {
        let seed = [7_u8; 32];
        // Fixed public test data is required for this cross-language known-answer vector.
        // codeql[rust/hard-coded-cryptographic-value]
        let nonce = [9_u8; 12];
        let (lookup, envelope) = seal_invite(
            "https://3mied18mppzo5rm16uzw5s6rxakceay3snhimxph3yjqi5tkdy8o.urspace.online/.urspace/open/#u3=fixture",
            2_000_000_000,
            &seed,
            nonce,
        )
        .unwrap();

        assert_eq!(lookup, "cBr_kIlOrtVJaefP5s3yIg");
        assert_eq!(envelope.nonce, "CQkJCQkJCQkJCQkJ");
        assert_eq!(
            envelope.ciphertext,
            "mf0-gJGTa1ppw25HRCQxKBIwIdVgvWUKj-JJosiiYT-RbWfShCxhUweiI4fp4rXwdV64Mm86dyvI7O6jUeuBsKOFeZve7Wv9UauF5yccIpcHJnnjav7maC_oM-7HaK8dWywStrY7WHsuWc8UIvTK6QPIrje-"
        );
    }

    #[test]
    fn rejects_insecure_or_credentialed_short_origins() {
        assert!(ShortLinkPublisher::new("http://u.urspace.online").is_err());
        assert!(ShortLinkPublisher::new("https://user:pass@u.urspace.online").is_err());
        assert!(ShortLinkPublisher::new("https://u.urspace.online/#secret").is_err());
        assert!(ShortLinkPublisher::new(DEFAULT_SHORT_ORIGIN).is_ok());
    }

    #[test]
    fn short_origin_must_match_the_invitation_base_domain() {
        let publisher = ShortLinkPublisher::new(DEFAULT_SHORT_ORIGIN).unwrap();
        assert!(
            publisher
                .validate_invite_destination(
                    &Url::parse(
                        "https://3mied18mppzo5rm16uzw5s6rxakceay3snhimxph3yjqi5tkdy8o.urspace.online/.urspace/open/#u3=fixture"
                    )
                    .unwrap()
                )
                .is_ok()
        );
        assert!(
            publisher
                .validate_invite_destination(
                    &Url::parse(
                        "https://3mied18mppzo5rm16uzw5s6rxakceay3snhimxph3yjqi5tkdy8o.example.com/.urspace/open/#u3=fixture"
                    )
                    .unwrap()
                )
                .is_err()
        );
    }
}
