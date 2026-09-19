//! Embed Urspace hosts and clients in Rust applications.
//!
//! The `urspace` CLI remains the hands-off path. This crate is for apps that
//! want to mint invitations and open private sessions from their own code.
//!
//! ```ignore
//! use urspace::{Client, Site, unix_now};
//!
//! let host = Site::serve("localhost:8787").await?;
//! let invite = host.bearer_invite()?;
//! let session = Client::connect(invite.as_str(), unix_now()).await?;
//! ```
//!
//! Subject-bound mint is the embed path for a public page. The browser (or
//! another app) generates a session key, the host mints a token bound to that
//! public key, and only that private key can finish admit.

pub use iroh::SecretKey;
pub use urspace_host::embed::{MintOptions, ShareOptions, Site};
pub use urspace_host::native_client::{NativeResponse, NativeSiteClient as Client, NativeSocket};
pub use urspace_host::{SessionInfo, unix_now};
pub use urspace_protocol::{
    Header, RequestMethod, SubjectInviteIssue, SubjectInvitePayload, sign_subject_invite,
    verify_subject_invite,
};

/// Generate an ephemeral session keypair. Keep the secret in process memory.
pub fn generate_session_key() -> SecretKey {
    SecretKey::generate()
}
