// SPDX-License-Identifier: MIT OR Apache-2.0
//! Self-signed TLS for the QUIC transport. TLS here is wire encryption and connection setup
//! only, never identity -- real sender identity is the sealed and signed `SecureMessage`
//! envelope, exactly as for TCP/WebSocket/HTTP/NAT traversal. See
//! `docs/superpowers/specs/2026-09-22-quic-transport-design.md` §3.

use crate::error::{Result, SynapseError};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::sync::Arc;

/// One ephemeral, self-signed certificate and key pair. Generated fresh by every call --
/// this transport never stores, rotates, or compares certificates across connections (spec §3).
pub fn generate_self_signed_cert() -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["synapse-quic".to_string()]).map_err(|e| {
            SynapseError::TransportError(format!("Failed to generate self-signed cert: {e}"))
        })?;
    let cert_der = cert.der().clone();
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    Ok((cert_der, key_der))
}

/// This transport's one `rustls` `CryptoProvider`, chosen explicitly rather than relying on
/// process-wide install order. This crate's own dependency graph already pulls in both `ring`
/// and `aws-lc-rs` transitively (via other, unrelated dependencies), so relying on
/// `CryptoProvider::install_default()` would make this transport's TLS behavior depend on
/// whichever unrelated code happens to install a default first -- fragile and non-obvious.
/// `ring` is chosen because it is what `quinn`'s and `rcgen`'s own default features already use.
fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A `rustls::ServerConfig` presenting the given self-signed certificate, restricted to TLS 1.3
/// (the only version QUIC ever negotiates). ALPN is set to `synapse-quic` so a peer speaking
/// anything else is refused at the handshake.
pub fn server_config(
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
) -> Result<Arc<rustls::ServerConfig>> {
    let mut config = rustls::ServerConfig::builder_with_provider(crypto_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| {
            SynapseError::TransportError(format!("Failed to select TLS 1.3 for QUIC server: {e}"))
        })?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|e| {
            SynapseError::TransportError(format!("Failed to build QUIC server TLS config: {e}"))
        })?;
    config.alpn_protocols = vec![b"synapse-quic".to_vec()];
    Ok(Arc::new(config))
}

/// A `rustls::ClientConfig` that accepts any server certificate (see [`DangerAcceptAnyServerCert`]
/// for why that is safe in this one place and nowhere else), restricted to TLS 1.3.
pub fn client_config() -> Result<Arc<rustls::ClientConfig>> {
    let mut config = rustls::ClientConfig::builder_with_provider(crypto_provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| {
            SynapseError::TransportError(format!("Failed to select TLS 1.3 for QUIC client: {e}"))
        })?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DangerAcceptAnyServerCert))
        .with_no_client_auth();
    config.alpn_protocols = vec![b"synapse-quic".to_vec()];
    Ok(Arc::new(config))
}

/// Accepts any server certificate without inspection. **Never reuse this outside the QUIC
/// transport's handshake.** It is sound *here* only because QUIC's TLS layer is wire encryption
/// and connection setup, never identity (spec §3, condition 1): real sender authentication is
/// the sealed and signed `SecureMessage` envelope, verified by `TransportManager` the same way
/// regardless of which transport carried it -- exactly as TCP and WebSocket already assume no
/// transport-level identity, and strictly better than TCP's fully plaintext connection.
/// Anywhere else in this codebase that TLS identity actually matters, this type is the wrong
/// answer; it is deliberately not named `Verifier` or anything a search for "how do we verify
/// TLS certs here" would surface as a general-purpose default.
///
/// **Private, not `pub(crate)` or `pub`, on purpose.** Its only caller is `client_config` in this
/// same file. A `pub` type here would be reachable as `synapse::transport::quic_tls::
/// DangerAcceptAnyServerCert` by every downstream consumer of this crate and would appear in
/// synapse's published docs.rs -- an "accept any certificate" verifier is an attractive nuisance
/// as public API, and this crate is queued for a 2.0.0 publish that cannot be un-published, only
/// yanked. If something legitimately outside this file ever needs it, that is a decision to make
/// deliberately, not a visibility level to widen back without discussion.
#[derive(Debug)]
struct DangerAcceptAnyServerCert;

impl rustls::client::danger::ServerCertVerifier for DangerAcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        crypto_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_self_signed_cert_is_generated_each_call() {
        let (cert_a, _key_a) = generate_self_signed_cert().expect("cert a");
        let (cert_b, _key_b) = generate_self_signed_cert().expect("cert b");
        assert_ne!(
            cert_a.as_ref(),
            cert_b.as_ref(),
            "two calls must not reuse one certificate"
        );
    }
}
