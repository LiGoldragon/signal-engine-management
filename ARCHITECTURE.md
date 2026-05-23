# signal-persona-engine-management — Architecture

`signal-persona-engine-management` is the ordinary Signal contract for
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
management socket, peer sockets, manager socket, and engine-management
protocol.

Domain sockets speak the component's ordinary `signal-persona-*` contract.
Engine-management sockets speak this crate's `Operation` / `Reply`.

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
