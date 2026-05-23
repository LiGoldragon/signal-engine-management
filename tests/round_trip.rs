use nota_codec::{Decoder, Encoder, NotaDecode, NotaEncode};
use signal_frame::{
    ExchangeIdentifier, ExchangeLane, LaneSequence, NonEmpty, Reply as FrameReply, RequestPayload,
    SessionEpoch, SubReply,
};
use signal_persona_engine_management::{
    ComponentHealth, ComponentHealthReport, ComponentIdentity, ComponentKind, ComponentName,
    ComponentNotReady, ComponentNotReadyReason, ComponentReady, ComponentStartupError, Frame,
    FrameBody, Operation, OperationKind, Presence, Protocol, Query, Reply, RequestUnimplemented,
    StopAcknowledgement, TimestampNanos, UnimplementedReason,
};

fn exchange() -> ExchangeIdentifier {
    ExchangeIdentifier::new(
        SessionEpoch::new(1),
        ExchangeLane::Connector,
        LaneSequence::first(),
    )
}

fn completed_reply(payload: Reply) -> FrameReply<Reply> {
    FrameReply::committed(NonEmpty::single(SubReply::Ok(payload)))
}

fn router_name() -> ComponentName {
    ComponentName::new("persona-router")
}

fn round_trip_operation(operation: Operation) -> Operation {
    let frame = Frame::new(FrameBody::Request {
        exchange: exchange(),
        request: operation.clone().into_request(),
    });
    let bytes = frame.encode_length_prefixed().expect("encode operation");
    let decoded = Frame::decode_length_prefixed(&bytes).expect("decode operation");

    match decoded.into_body() {
        FrameBody::Request { request, .. } => request.payloads().head().clone(),
        other => panic!("expected request, got {other:?}"),
    }
}

fn round_trip_reply(reply: Reply) -> Reply {
    let frame = Frame::new(FrameBody::Reply {
        exchange: exchange(),
        reply: completed_reply(reply.clone()),
    });
    let bytes = frame.encode_length_prefixed().expect("encode reply");
    let decoded = Frame::decode_length_prefixed(&bytes).expect("decode reply");

    match decoded.into_body() {
        FrameBody::Reply { reply, .. } => match reply {
            FrameReply::Accepted { per_operation, .. } => match per_operation.into_head() {
                SubReply::Ok(payload) => payload,
                other => panic!("expected accepted reply payload, got {other:?}"),
            },
            other => panic!("expected accepted reply, got {other:?}"),
        },
        other => panic!("expected reply, got {other:?}"),
    }
}

#[test]
fn operations_round_trip_through_length_prefixed_frames() {
    let announce = Operation::Announce(Presence {
        expected_component: router_name(),
        expected_kind: ComponentKind::Router,
        protocol: Protocol::new(1),
    });
    assert_eq!(round_trip_operation(announce.clone()), announce);

    for query in [
        Query::ReadinessStatus(router_name()),
        Query::HealthStatus(router_name()),
    ] {
        let operation = Operation::Query(query);
        assert_eq!(round_trip_operation(operation.clone()), operation);
    }

    let stop = Operation::Stop(router_name());
    assert_eq!(round_trip_operation(stop.clone()), stop);
}

#[test]
fn replies_round_trip_through_length_prefixed_frames() {
    let replies = [
        Reply::Identified(ComponentIdentity {
            name: router_name(),
            kind: ComponentKind::Router,
            protocol: Protocol::new(1),
            last_fatal_startup_error: None,
        }),
        Reply::Ready(ComponentReady {
            component_started_at: Some(TimestampNanos::new(100)),
        }),
        Reply::NotReady(ComponentNotReady {
            reason: ComponentNotReadyReason::AwaitingDependency,
        }),
        Reply::HealthReport(ComponentHealthReport {
            health: ComponentHealth::Running,
        }),
        Reply::StopAcknowledged(StopAcknowledgement {
            drain_completed_at: Some(TimestampNanos::new(200)),
        }),
        Reply::Unimplemented(RequestUnimplemented {
            reason: UnimplementedReason::NotInPrototypeScope,
        }),
    ];

    for reply in replies {
        assert_eq!(round_trip_reply(reply.clone()), reply);
    }
}

#[test]
fn nota_text_shape_stays_canonical() {
    let operation = Operation::Announce(Presence {
        expected_component: router_name(),
        expected_kind: ComponentKind::Router,
        protocol: Protocol::new(1),
    });
    let mut encoder = Encoder::new();
    operation.encode(&mut encoder).expect("encode");
    let text = encoder.into_string();
    let mut decoder = Decoder::new(&text);
    let recovered = Operation::decode(&mut decoder).expect("decode");
    assert_eq!(recovered, operation);
    assert_eq!(text, "(Announce (persona-router Router 1))");

    let reply = Reply::Identified(ComponentIdentity {
        name: router_name(),
        kind: ComponentKind::Router,
        protocol: Protocol::new(1),
        last_fatal_startup_error: Some(ComponentStartupError::StoreOpenFailed),
    });
    let mut encoder = Encoder::new();
    reply.encode(&mut encoder).expect("encode");
    let text = encoder.into_string();
    let mut decoder = Decoder::new(&text);
    let recovered = Reply::decode(&mut decoder).expect("decode");
    assert_eq!(recovered, reply);
    assert_eq!(
        text,
        "(Identified (persona-router Router 1 (Some StoreOpenFailed)))"
    );
}

#[test]
fn operation_kind_is_generated_by_macro() {
    let cases = [
        (
            Operation::Announce(Presence {
                expected_component: router_name(),
                expected_kind: ComponentKind::Router,
                protocol: Protocol::new(1),
            }),
            OperationKind::Announce,
        ),
        (
            Operation::Query(Query::ReadinessStatus(router_name())),
            OperationKind::Query,
        ),
        (Operation::Stop(router_name()), OperationKind::Stop),
    ];

    for (operation, expected_kind) in cases {
        assert_eq!(operation.kind(), expected_kind);
    }
}
