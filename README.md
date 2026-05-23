# signal-persona-engine-management

Ordinary Signal contract for Persona engine-management lifecycle traffic.

This crate carries the manager-to-supervised-component relation: announce,
readiness query, health query, graceful stop, and the typed `SpawnEnvelope`
used to launch child components. Privileged Persona owner commands live in
`owner-signal-persona`.
