# signal-engine-management — Architecture

`signal-engine-management` is the ordinary Signal contract for
Persona manager-to-supervised-component lifecycle traffic.

## Boundary

This crate carries the lifecycle relation that makes a process a Persona
component:

| Operation | Meaning |
|---|---|
| `Announce(Presence)` | child identifies itself to the manager |
| `Query(Query::ReadinessStatus(ComponentName))` | manager asks whether the child is ready |
| `Query(Query::HealthStatus(ComponentName))` | manager asks for the child health state |
| `Stop(ComponentName)` | manager asks the child to drain and stop |

The privileged engine-manager command surface lives in
`owner-signal-persona`. This crate is not an owner socket and not a generic
domain command bus.

## Spawn Envelope

`SpawnEnvelope` is the typed launch record the Persona manager gives to a
child process. It names the engine identity, component kind, component
principal, owner identity, state directory, ordinary domain socket, engine
management socket, peer sockets, manager socket, engine-management
protocol, and the supervising process's verifiable parent authority.

Domain sockets speak the component's ordinary `signal-persona-*` contract.
Engine-management sockets speak this crate's `Operation` / `Reply`.

## Stable Durable Identity (`identity` module)

The `parent_authority: ParentAuthority` field on `SpawnEnvelope` carries
the supervisor's kernel process identifier and Unix user identifier as
declared at spawn time. The child caches the envelope and, on every
supervisor connection, reads `SO_PEERCRED` from the accepted Unix stream
via a `PeerCredentialsSource` implementation and calls
`verify_spawn_envelope_origin(source, &stream, &envelope)`.

That function returns a `DurableIdentity { engine_identifier,
peer_credentials }` only when the kernel-reported peer credentials match
the envelope's parent authority. The result is the binding the psyche
asked for:

- *Stable*: `engine_identifier` survives restarts of either side.
- *Durable*: every connection re-anchors against credentials the kernel
  grants, not against a token the connecting peer chose.

The contract crate forbids unsafe code and stays I/O-strategy agnostic:
the `PeerCredentialsSource` trait abstracts the `SO_PEERCRED` read so
supervised daemons can choose `rustix`, `nix`, or a future stabilised
`UnixStream::peer_cred()` without touching the contract surface.
Mismatches surface as `IdentityError::ProcessIdentifierMismatch`,
`IdentityError::UnixUserIdentifierMismatch`, or
`IdentityError::PeerCredentialsReadFailed`; the caller MUST close the
connection on any of them — the peer is not the supervisor.

## Skeleton Honesty

Every supervised daemon decodes every operation variant in this crate. Built
variants reply with their typed result. Future unbuilt-but-decodable variants
must reply with `Reply::Unimplemented(RequestUnimplemented { ... })`, not
panic, silently drop, or invent an untyped error.

The prototype variants `Announce`, `Query(ReadinessStatus)`,
`Query(HealthStatus)`, and `Stop` are not optional. A process that cannot
answer them is not yet a Persona component.

## Invariants

- This channel has no observability stream; the manager already owns this
  infrastructure traffic.
- Request payloads do not carry caller identity, timestamps, or authorization
  proof. Those facts are infrastructure-owned.
- `SpawnEnvelope` is a closed typed record; new launch facts are schema bumps.
- Wire enums are closed. There is no `Unknown` escape hatch.

## See Also

- `/git/github.com/LiGoldragon/owner-signal-persona/ARCHITECTURE.md`
- `/git/github.com/LiGoldragon/signal-persona-origin/ARCHITECTURE.md`
