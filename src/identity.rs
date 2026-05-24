//! Stable durable identity for supervised components.
//!
//! A Persona engine identifies a supervised process by two facts that
//! reinforce each other:
//!
//! 1. **Envelope-declared engine identity** — the parent supervisor names the
//!    long-lived `engine_identifier`, the `component_name`, and the
//!    `parent_authority` (its own kernel process identifier plus Unix user
//!    identifier) at spawn time. The child receives this through the
//!    `SpawnEnvelope` and caches it.
//!
//! 2. **Kernel-verified peer credentials** — when the parent supervisor later
//!    connects to a socket the child bound, the child reads `SO_PEERCRED`
//!    from the accepted stream and obtains the connecting process's
//!    `process_identifier`, `unix_user_identifier`, and
//!    `unix_group_identifier`. These are kernel-supplied and unforgeable by
//!    the connecting peer.
//!
//! [`DurableIdentity`] is the binding: envelope's `engine_identifier`
//! together with the verified peer credentials. The verification step
//! ([`verify_spawn_envelope_origin`]) refuses to mint a [`DurableIdentity`]
//! when the kernel-reported peer process identifier or Unix user identifier
//! disagrees with the envelope-declared `parent_authority`.
//!
//! The identity is *stable* because `engine_identifier` survives restarts.
//! The identity is *durable* because the verification anchors every
//! connection to credentials the kernel grants, not to a token the
//! connecting peer chose.
//!
//! # No unsafe code, no I/O dependency
//!
//! This crate forbids unsafe code AND keeps the contract surface
//! independent of any particular `SO_PEERCRED` I/O strategy. The
//! [`PeerCredentialsSource`] trait expresses "give me the kernel-supplied
//! peer credentials for this stream"; supervised daemons supply the
//! implementation (typically backed by `rustix::net::sockopt::socket_peercred`,
//! the `nix` crate's `getsockopt`, or a future stabilised standard-library
//! `UnixStream::peer_cred`). Tests in this crate substitute a fixture.
//!
//! Keeping the read out of the contract avoids tying the wire schema to a
//! specific operating-system crate and respects the `unsafe_code = "forbid"`
//! lint on `signal-engine-management`.

use std::os::unix::net::UnixStream;

use nota_codec::{NotaRecord, NotaTransparent};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use signal_persona_origin::{EngineIdentifier, UnixUserIdentifier};

/// Kernel process identifier, as reported by `SO_PEERCRED`.
#[derive(
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    NotaTransparent,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
)]
pub struct ProcessIdentifier(u32);

impl ProcessIdentifier {
    /// Creates a process identifier from a raw kernel value.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the raw process identifier.
    pub const fn into_u32(self) -> u32 {
        self.0
    }
}

/// Unix group identifier, as reported by `SO_PEERCRED`.
#[derive(
    Archive,
    RkyvSerialize,
    RkyvDeserialize,
    NotaTransparent,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
)]
pub struct UnixGroupIdentifier(u32);

impl UnixGroupIdentifier {
    /// Creates a Unix group identifier from a raw value.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Returns the raw Unix group identifier.
    pub const fn into_u32(self) -> u32 {
        self.0
    }
}

/// The supervising process's verifiable identity, declared by the spawn
/// envelope and later proved by `SO_PEERCRED` when the supervisor connects.
///
/// The child caches this from the envelope and consults it on every
/// supervisor connection. The kernel-reported peer credentials of the
/// connecting socket MUST match these two fields, otherwise the connection
/// is not the declared supervisor.
#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct ParentAuthority {
    /// The supervisor's kernel process identifier at the moment of spawn.
    /// `SO_PEERCRED` on an accepted connection from the supervisor MUST
    /// report this value.
    pub parent_process_identifier: ProcessIdentifier,
    /// The supervisor's Unix user identifier. `SO_PEERCRED` MUST also
    /// report this value.
    pub parent_unix_user_identifier: UnixUserIdentifier,
}

impl ParentAuthority {
    /// Names a new parent authority for a spawn envelope.
    pub const fn new(
        parent_process_identifier: ProcessIdentifier,
        parent_unix_user_identifier: UnixUserIdentifier,
    ) -> Self {
        Self {
            parent_process_identifier,
            parent_unix_user_identifier,
        }
    }
}

/// Kernel-supplied peer credentials read from `SO_PEERCRED` at the moment
/// of `accept()`.
#[derive(
    Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, Copy, PartialEq, Eq,
)]
pub struct PeerCredentials {
    /// The connecting peer's kernel process identifier.
    pub process_identifier: ProcessIdentifier,
    /// The connecting peer's Unix user identifier.
    pub unix_user_identifier: UnixUserIdentifier,
    /// The connecting peer's primary Unix group identifier.
    pub unix_group_identifier: UnixGroupIdentifier,
}

impl PeerCredentials {
    /// Constructs a peer-credentials record from raw kernel values.
    pub const fn new(
        process_identifier: ProcessIdentifier,
        unix_user_identifier: UnixUserIdentifier,
        unix_group_identifier: UnixGroupIdentifier,
    ) -> Self {
        Self {
            process_identifier,
            unix_user_identifier,
            unix_group_identifier,
        }
    }
}

/// The bound stable durable identity for one connection.
///
/// Combines the envelope's long-lived `engine_identifier` (which survives
/// restarts) with the per-session peer credentials the kernel reports for
/// the connecting socket. The combination is what makes the identity
/// simultaneously *stable* (engine identifier does not change across
/// restarts) and *verifiable* (every connection re-anchors against
/// kernel-supplied credentials).
#[derive(Archive, RkyvSerialize, RkyvDeserialize, NotaRecord, Debug, Clone, PartialEq, Eq)]
pub struct DurableIdentity {
    /// The long-lived engine identity declared by the spawning supervisor.
    pub engine_identifier: EngineIdentifier,
    /// The kernel-verified credentials of the connected peer.
    pub peer_credentials: PeerCredentials,
}

impl DurableIdentity {
    /// Binds a stable durable identity by combining the envelope-declared
    /// engine identifier with kernel-verified peer credentials read from
    /// `SO_PEERCRED` at the moment of `accept()`.
    ///
    /// The caller MUST have already verified that the peer credentials
    /// match the envelope's [`ParentAuthority`]; this constructor only
    /// records the binding. Use [`verify_spawn_envelope_origin`] for the
    /// combined verification and binding step.
    pub fn from_envelope_and_peer(envelope: &crate::SpawnEnvelope, peer: PeerCredentials) -> Self {
        Self {
            engine_identifier: envelope.engine_identifier.clone(),
            peer_credentials: peer,
        }
    }
}

/// Reason a spawn-envelope verification step refused to mint a
/// [`DurableIdentity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// The kernel-reported peer process identifier disagreed with the
    /// envelope-declared parent process identifier.
    ProcessIdentifierMismatch {
        /// The envelope-declared parent process identifier.
        expected: ProcessIdentifier,
        /// The kernel-reported peer process identifier.
        actual: ProcessIdentifier,
    },
    /// The kernel-reported peer Unix user identifier disagreed with the
    /// envelope-declared parent Unix user identifier.
    UnixUserIdentifierMismatch {
        /// The envelope-declared parent Unix user identifier.
        expected: UnixUserIdentifier,
        /// The kernel-reported peer Unix user identifier.
        actual: UnixUserIdentifier,
    },
    /// The standard library refused to read `SO_PEERCRED` from the
    /// accepted Unix stream. The carried message is the operating-system
    /// error description, captured at the read site.
    PeerCredentialsReadFailed(String),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProcessIdentifierMismatch { expected, actual } => write!(
                formatter,
                "peer process identifier {} disagrees with envelope-declared parent {}",
                actual.into_u32(),
                expected.into_u32(),
            ),
            Self::UnixUserIdentifierMismatch { expected, actual } => write!(
                formatter,
                "peer Unix user identifier {} disagrees with envelope-declared parent {}",
                actual.as_u32(),
                expected.as_u32(),
            ),
            Self::PeerCredentialsReadFailed(message) => {
                write!(formatter, "could not read SO_PEERCRED: {message}")
            }
        }
    }
}

impl std::error::Error for IdentityError {}

/// Source of `SO_PEERCRED` peer credentials for an already-accepted Unix
/// stream.
///
/// Supervised daemons supply the implementation; the contract crate does
/// not pick a particular `SO_PEERCRED` read mechanism because it forbids
/// unsafe code and would otherwise have to depend on an operating-system
/// crate purely for one socket option. Typical production implementations
/// wrap `rustix::net::sockopt::socket_peercred` or `nix::sys::socket::getsockopt`
/// with the `PeerCredentials` option.
///
/// Tests pass a fixture so the verification logic can be exercised
/// without spawning a second process with a controlled process and Unix
/// user identifier.
///
/// # Example (downstream daemon, sketch only)
///
/// ```ignore
/// use signal_engine_management::{IdentityError, PeerCredentials, PeerCredentialsSource};
/// use signal_engine_management::{ProcessIdentifier, UnixGroupIdentifier};
/// use signal_persona_origin::UnixUserIdentifier;
/// use std::os::unix::net::UnixStream;
///
/// struct RustixPeerCredentialsSource;
///
/// impl PeerCredentialsSource for RustixPeerCredentialsSource {
///     fn read_peer_credentials(&self, stream: &UnixStream)
///         -> Result<PeerCredentials, IdentityError>
///     {
///         let raw = rustix::net::sockopt::socket_peercred(stream)
///             .map_err(|error| IdentityError::PeerCredentialsReadFailed(error.to_string()))?;
///         Ok(PeerCredentials::new(
///             ProcessIdentifier::new(raw.pid.as_raw_nonzero().get() as u32),
///             UnixUserIdentifier::new(raw.uid.as_raw()),
///             UnixGroupIdentifier::new(raw.gid.as_raw()),
///         ))
///     }
/// }
/// ```
pub trait PeerCredentialsSource {
    /// Reads kernel-supplied peer credentials from the accepted stream.
    fn read_peer_credentials(&self, stream: &UnixStream) -> Result<PeerCredentials, IdentityError>;
}

/// Verifies that the peer of an accepted Unix stream is the supervising
/// process declared by the spawn envelope, and returns the bound
/// [`DurableIdentity`] when the check passes.
///
/// The check compares the kernel-reported peer credentials against the
/// envelope's [`ParentAuthority`]:
///
/// - process identifier MUST match `parent_process_identifier`
/// - Unix user identifier MUST match `parent_unix_user_identifier`
///
/// On either mismatch the function returns the matching [`IdentityError`]
/// variant and does NOT mint a [`DurableIdentity`]. The caller MUST close
/// the connection — the peer is not the supervisor.
///
/// Production callers supply a [`PeerCredentialsSource`] backed by
/// `rustix::net::sockopt::socket_peercred` or another `SO_PEERCRED` reader.
/// Tests pass a fixture [`PeerCredentialsSource`] so that the verification
/// logic can be exercised without an actual Unix-stream pair from two
/// different processes.
pub fn verify_spawn_envelope_origin<S: PeerCredentialsSource>(
    source: &S,
    stream: &UnixStream,
    envelope: &crate::SpawnEnvelope,
) -> Result<DurableIdentity, IdentityError> {
    let peer = source.read_peer_credentials(stream)?;
    verify_peer_credentials_against_envelope(envelope, peer)?;
    Ok(DurableIdentity::from_envelope_and_peer(envelope, peer))
}

/// Pure verification step: compares kernel-supplied peer credentials
/// against the envelope's parent authority, without performing the
/// `SO_PEERCRED` read. Exposed so callers that already hold the peer
/// credentials (for example, those that read them through a different
/// abstraction) can reuse the matching logic.
pub fn verify_peer_credentials_against_envelope(
    envelope: &crate::SpawnEnvelope,
    peer: PeerCredentials,
) -> Result<(), IdentityError> {
    let authority = &envelope.parent_authority;
    if peer.process_identifier != authority.parent_process_identifier {
        return Err(IdentityError::ProcessIdentifierMismatch {
            expected: authority.parent_process_identifier,
            actual: peer.process_identifier,
        });
    }
    if peer.unix_user_identifier != authority.parent_unix_user_identifier {
        return Err(IdentityError::UnixUserIdentifierMismatch {
            expected: authority.parent_unix_user_identifier,
            actual: peer.unix_user_identifier,
        });
    }
    Ok(())
}
