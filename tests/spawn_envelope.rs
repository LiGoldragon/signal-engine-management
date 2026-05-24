use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode};
use signal_engine_management::{
    ComponentKind, EngineManagementProtocolVersion, PeerSocket, SocketMode, SpawnEnvelope, WirePath,
};

fn fixture_spawn_envelope() -> SpawnEnvelope {
    SpawnEnvelope {
        engine_identifier: signal_persona_origin::EngineIdentifier::new("default"),
        component_kind: ComponentKind::Message,
        component_name: signal_persona_origin::ComponentName::Message,
        owner_identity: signal_persona_origin::OwnerIdentity::UnixUser(
            signal_persona_origin::UnixUserIdentifier::new(1001),
        ),
        state_dir: WirePath::new("/var/lib/persona/default/message"),
        domain_socket_path: WirePath::new("/var/run/persona/default/message.sock"),
        domain_socket_mode: SocketMode::new(0o660),
        engine_management_socket_path: WirePath::new(
            "/var/run/persona/default/message.engine_management.sock",
        ),
        engine_management_socket_mode: SocketMode::new(0o600),
        peer_sockets: vec![PeerSocket {
            component_name: signal_persona_origin::ComponentName::Router,
            domain_socket_path: WirePath::new("/var/run/persona/default/router.sock"),
        }],
        manager_socket: WirePath::new("/var/run/persona/default/persona.sock"),
        engine_management_protocol_version: EngineManagementProtocolVersion::new(1),
    }
}

#[test]
fn spawn_envelope_round_trips_through_nota_text() {
    let envelope = fixture_spawn_envelope();
    let mut encoder = Encoder::new();
    envelope
        .encode(&mut encoder)
        .expect("encode spawn envelope");
    let text = encoder.into_string();
    let mut decoder = Decoder::new(&text);
    let recovered = SpawnEnvelope::decode(&mut decoder).expect("decode spawn envelope");

    assert_eq!(recovered, envelope);
    assert_eq!(
        text,
        "(default Message Message (UnixUser 1001) [/var/lib/persona/default/message] [/var/run/persona/default/message.sock] 432 [/var/run/persona/default/message.engine_management.sock] 384 [(Router [/var/run/persona/default/router.sock])] [/var/run/persona/default/persona.sock] 1)"
    );
}

#[test]
fn spawn_envelope_carries_closed_component_principals() {
    let envelope = fixture_spawn_envelope();

    assert_eq!(
        envelope.component_name,
        signal_persona_origin::ComponentName::Message
    );
    assert_eq!(
        envelope.peer_sockets[0].component_name,
        signal_persona_origin::ComponentName::Router
    );
}

#[test]
fn spawn_envelope_separates_domain_and_engine_management_sockets() {
    let envelope = fixture_spawn_envelope();

    assert_eq!(
        envelope.domain_socket_path.as_str(),
        "/var/run/persona/default/message.sock"
    );
    assert_eq!(envelope.domain_socket_mode.into_u32(), 0o660);
    assert_eq!(
        envelope.engine_management_socket_path.as_str(),
        "/var/run/persona/default/message.engine_management.sock"
    );
    assert_eq!(envelope.engine_management_socket_mode.into_u32(), 0o600);
}
