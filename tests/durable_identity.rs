//! Tests for the stable durable identity binding declared in
//! `signal_engine_management::identity`.
//!
//! The SO_PEERCRED read against a real Unix-stream pair would require two
//! cooperating processes with controlled process and Unix-user identifiers,
//! which is fragile in CI. These tests cover the verification logic with
//! the [`PeerCredentialsSource`] fixture, plus a wire-shape round-trip test
//! that exercises both the encoded NOTA form and the in-memory equality of
//! every relevant identity type.
//!
//! DESIGN-DECISION-REVIEW (second-designer/172 §3.4)

use std::os::unix::net::UnixStream;

use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode};
use signal_engine_management::{
    ComponentKind, DurableIdentity, EngineManagementProtocolVersion, IdentityError,
    ParentAuthority, PeerCredentials, PeerCredentialsSource, PeerSocket, ProcessIdentifier,
    SocketMode, SpawnEnvelope, UnixGroupIdentifier, WirePath,
    verify_peer_credentials_against_envelope, verify_spawn_envelope_origin,
};
use signal_persona_origin::{EngineIdentifier, OwnerIdentity, UnixUserIdentifier};

fn fixture_envelope(parent_process: u32, parent_user: u32) -> SpawnEnvelope {
    SpawnEnvelope {
        engine_identifier: EngineIdentifier::new("default"),
        component_kind: ComponentKind::Mind,
        component_name: signal_persona_origin::ComponentName::Mind,
        owner_identity: OwnerIdentity::UnixUser(UnixUserIdentifier::new(1001)),
        state_dir: WirePath::new("/var/lib/persona/default/mind"),
        domain_socket_path: WirePath::new("/var/run/persona/default/mind.sock"),
        domain_socket_mode: SocketMode::new(0o660),
        engine_management_socket_path: WirePath::new(
            "/var/run/persona/default/mind.engine_management.sock",
        ),
        engine_management_socket_mode: SocketMode::new(0o600),
        peer_sockets: vec![PeerSocket {
            component_name: signal_persona_origin::ComponentName::Router,
            domain_socket_path: WirePath::new("/var/run/persona/default/router.sock"),
        }],
        manager_socket: WirePath::new("/var/run/persona/default/persona.sock"),
        engine_management_protocol_version: EngineManagementProtocolVersion::new(1),
        parent_authority: ParentAuthority::new(
            ProcessIdentifier::new(parent_process),
            UnixUserIdentifier::new(parent_user),
        ),
    }
}

/// Test fixture: a `PeerCredentialsSource` that returns a pre-built set of
/// credentials. Replaces an actual `SO_PEERCRED` read so the verification
/// logic can be exercised deterministically.
struct FixturePeerCredentialsSource(PeerCredentials);

impl PeerCredentialsSource for FixturePeerCredentialsSource {
    fn read_peer_credentials(
        &self,
        _stream: &UnixStream,
    ) -> Result<PeerCredentials, IdentityError> {
        Ok(self.0)
    }
}

/// A `UnixStream` is needed by the verifier signature even though the
/// fixture source ignores it. Constructing an unconnected pair gives us one
/// without depending on real peer-cred behaviour.
fn fixture_unix_stream() -> UnixStream {
    let (stream, _other) = UnixStream::pair().expect("create unconnected Unix stream pair");
    stream
}

#[test]
fn round_trip_envelope_and_peer_to_durable_identity() {
    let envelope = fixture_envelope(100, 1000);
    let peer = PeerCredentials::new(
        ProcessIdentifier::new(100),
        UnixUserIdentifier::new(1000),
        UnixGroupIdentifier::new(1000),
    );
    let source = FixturePeerCredentialsSource(peer);
    let stream = fixture_unix_stream();

    let identity = verify_spawn_envelope_origin(&source, &stream, &envelope)
        .expect("verification passes when envelope and peer agree");

    assert_eq!(
        identity,
        DurableIdentity {
            engine_identifier: EngineIdentifier::new("default"),
            peer_credentials: PeerCredentials::new(
                ProcessIdentifier::new(100),
                UnixUserIdentifier::new(1000),
                UnixGroupIdentifier::new(1000),
            ),
        }
    );
    assert_eq!(identity.engine_identifier.as_str(), "default");
    assert_eq!(identity.peer_credentials.process_identifier.into_u32(), 100);
    assert_eq!(
        identity.peer_credentials.unix_user_identifier.as_u32(),
        1000
    );
    assert_eq!(
        identity.peer_credentials.unix_group_identifier.into_u32(),
        1000
    );
}

#[test]
fn verification_rejects_process_identifier_mismatch() {
    let envelope = fixture_envelope(100, 1000);
    let peer = PeerCredentials::new(
        ProcessIdentifier::new(200),
        UnixUserIdentifier::new(1000),
        UnixGroupIdentifier::new(1000),
    );
    let source = FixturePeerCredentialsSource(peer);
    let stream = fixture_unix_stream();

    let error = verify_spawn_envelope_origin(&source, &stream, &envelope)
        .expect_err("process identifier mismatch rejected");

    assert_eq!(
        error,
        IdentityError::ProcessIdentifierMismatch {
            expected: ProcessIdentifier::new(100),
            actual: ProcessIdentifier::new(200),
        }
    );
}

#[test]
fn verification_rejects_unix_user_identifier_mismatch() {
    let envelope = fixture_envelope(100, 1000);
    let peer = PeerCredentials::new(
        ProcessIdentifier::new(100),
        UnixUserIdentifier::new(1001),
        UnixGroupIdentifier::new(1000),
    );
    let source = FixturePeerCredentialsSource(peer);
    let stream = fixture_unix_stream();

    let error = verify_spawn_envelope_origin(&source, &stream, &envelope)
        .expect_err("Unix user identifier mismatch rejected");

    assert_eq!(
        error,
        IdentityError::UnixUserIdentifierMismatch {
            expected: UnixUserIdentifier::new(1000),
            actual: UnixUserIdentifier::new(1001),
        }
    );
}

#[test]
fn process_identifier_mismatch_takes_precedence_over_user_mismatch() {
    // When both the process identifier and the Unix user identifier
    // disagree, the verifier reports the process identifier mismatch first.
    // This is documented by the implementation; the test pins the order so
    // future refactors do not silently reshuffle the diagnostic.
    let envelope = fixture_envelope(100, 1000);
    let peer = PeerCredentials::new(
        ProcessIdentifier::new(999),
        UnixUserIdentifier::new(9999),
        UnixGroupIdentifier::new(1000),
    );
    let source = FixturePeerCredentialsSource(peer);
    let stream = fixture_unix_stream();

    let error = verify_spawn_envelope_origin(&source, &stream, &envelope)
        .expect_err("dual mismatch rejected");

    assert!(matches!(
        error,
        IdentityError::ProcessIdentifierMismatch { .. },
    ));
}

#[test]
fn pure_verification_step_does_not_require_a_stream() {
    // `verify_peer_credentials_against_envelope` exposes the matching logic
    // for callers that already hold a `PeerCredentials` value (for example,
    // those that read SO_PEERCRED through a different abstraction layer).
    let envelope = fixture_envelope(7000, 0);
    let matching = PeerCredentials::new(
        ProcessIdentifier::new(7000),
        UnixUserIdentifier::new(0),
        UnixGroupIdentifier::new(0),
    );
    let process_mismatch = PeerCredentials::new(
        ProcessIdentifier::new(7001),
        UnixUserIdentifier::new(0),
        UnixGroupIdentifier::new(0),
    );
    let user_mismatch = PeerCredentials::new(
        ProcessIdentifier::new(7000),
        UnixUserIdentifier::new(1),
        UnixGroupIdentifier::new(0),
    );

    verify_peer_credentials_against_envelope(&envelope, matching).expect("match accepted");
    assert!(matches!(
        verify_peer_credentials_against_envelope(&envelope, process_mismatch).unwrap_err(),
        IdentityError::ProcessIdentifierMismatch { .. },
    ));
    assert!(matches!(
        verify_peer_credentials_against_envelope(&envelope, user_mismatch).unwrap_err(),
        IdentityError::UnixUserIdentifierMismatch { .. },
    ));
}

#[test]
fn parent_authority_round_trips_through_nota_text() {
    let authority = ParentAuthority::new(ProcessIdentifier::new(4242), UnixUserIdentifier::new(0));
    let mut encoder = Encoder::new();
    authority.encode(&mut encoder).expect("encode authority");
    let text = encoder.into_string();

    assert_eq!(text, "(4242 0)");

    let mut decoder = Decoder::new(&text);
    let recovered = ParentAuthority::decode(&mut decoder).expect("decode authority");
    assert_eq!(recovered, authority);
}

#[test]
fn peer_credentials_round_trip_through_nota_text() {
    let credentials = PeerCredentials::new(
        ProcessIdentifier::new(100),
        UnixUserIdentifier::new(1000),
        UnixGroupIdentifier::new(1000),
    );
    let mut encoder = Encoder::new();
    credentials
        .encode(&mut encoder)
        .expect("encode credentials");
    let text = encoder.into_string();

    assert_eq!(text, "(100 1000 1000)");

    let mut decoder = Decoder::new(&text);
    let recovered = PeerCredentials::decode(&mut decoder).expect("decode credentials");
    assert_eq!(recovered, credentials);
}

#[test]
fn durable_identity_round_trips_through_nota_text() {
    let identity = DurableIdentity {
        engine_identifier: EngineIdentifier::new("default"),
        peer_credentials: PeerCredentials::new(
            ProcessIdentifier::new(100),
            UnixUserIdentifier::new(1000),
            UnixGroupIdentifier::new(1000),
        ),
    };
    let mut encoder = Encoder::new();
    identity.encode(&mut encoder).expect("encode identity");
    let text = encoder.into_string();

    assert_eq!(text, "(default (100 1000 1000))");

    let mut decoder = Decoder::new(&text);
    let recovered = DurableIdentity::decode(&mut decoder).expect("decode identity");
    assert_eq!(recovered, identity);
}

#[test]
fn spawn_envelope_with_parent_authority_round_trips() {
    // Verifies that the new `parent_authority` field plays nicely with the
    // existing NOTA encoding for `SpawnEnvelope`. The wire shape extends
    // the previous tuple by one trailing element.
    let envelope = fixture_envelope(4242, 0);
    let mut encoder = Encoder::new();
    envelope.encode(&mut encoder).expect("encode envelope");
    let text = encoder.into_string();

    // The tail of the encoded form ends with `1 (4242 0))`: protocol
    // version then the parent_authority record.
    assert!(
        text.ends_with(" 1 (4242 0))"),
        "envelope wire form should end with protocol version followed by parent authority; got {text}",
    );

    let mut decoder = Decoder::new(&text);
    let recovered = SpawnEnvelope::decode(&mut decoder).expect("decode envelope");
    assert_eq!(recovered, envelope);
    assert_eq!(
        recovered.parent_authority.parent_process_identifier,
        ProcessIdentifier::new(4242),
    );
    assert_eq!(
        recovered.parent_authority.parent_unix_user_identifier,
        UnixUserIdentifier::new(0),
    );
}
