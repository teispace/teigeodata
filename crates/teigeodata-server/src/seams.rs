//! The hosted seams (ADR-0025): who is asking ([`Identify`]), may they ([`Authorize`]: access,
//! rate limits, quotas) and how much they used ([`Meter`]). Both servers call them on every
//! request, in that order; the defaults are open (anyone, anything, nothing counted), so a
//! self-hosted server needs none, and a hosted tier is a deployment that plugs its own in,
//! not a fork.
//!
//! The seams are synchronous and take no runtime: a hosted implementation that must call out
//! keeps its own cache (a token verified once, a counter flushed in batches) so a request never
//! waits on the network.

use std::net::SocketAddr;
use std::sync::Arc;

use crate::query::{Ask, QueryError};

/// What a request carries that a seam may read: its headers (HTTP; none on MCP's stdio) and
/// where it came from.
pub trait Incoming {
    /// The value of header `name` (ASCII, any case), if sent once.
    fn header(&self, name: &str) -> Option<&[u8]>;

    /// The peer's address, when the transport has one.
    fn peer(&self) -> Option<SocketAddr> {
        None
    }
}

/// A request with nothing to read: MCP over stdio, a test.
#[derive(Clone, Copy, Debug, Default)]
pub struct Bare;

impl Incoming for Bare {
    fn header(&self, _: &str) -> Option<&[u8]> {
        None
    }
}

/// Who is asking, as a seam knows them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Caller {
    /// Their identity; `None` for anyone (the open default).
    pub id: Option<String>,
    /// The tenant whose packs and limits apply; `None` for the server's own.
    pub tenant: Option<String>,
}

/// What a request does, for authorizing and metering. New operations may be added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Operation {
    /// [`Ask::Get`].
    Get,
    /// [`Ask::Lookup`].
    Lookup,
    /// [`Ask::Reverse`].
    Reverse,
    /// [`Ask::Near`].
    Near,
    /// [`Ask::Search`].
    Search,
    /// [`Ask::Hierarchy`].
    Hierarchy,
    /// [`Ask::Children`].
    Children,
    /// [`Ask::Admin`].
    Admin,
    /// [`Ask::TzAt`] and [`Ask::TzOf`].
    Timezone,
    /// [`crate::query::elevation`].
    Elevation,
    /// [`crate::query::postal`].
    Postal,
    /// [`crate::query::boundary`].
    Boundary,
    /// [`crate::query::distance`].
    Distance,
    /// The countries and the catalog's packs at a point (D-318).
    Coverage,
    /// Installing packs into the server's store, and asking how an install goes (D-318).
    Install,
    /// The pack set's description.
    Packs,
}

impl Operation {
    /// The operation `ask` is.
    #[must_use]
    pub const fn of(ask: &Ask) -> Self {
        match ask {
            Ask::Get { .. } => Self::Get,
            Ask::Lookup { .. } => Self::Lookup,
            Ask::Reverse { .. } => Self::Reverse,
            Ask::Near { .. } => Self::Near,
            Ask::Search { .. } => Self::Search,
            Ask::Hierarchy { .. } => Self::Hierarchy,
            Ask::Children { .. } => Self::Children,
            Ask::Admin { .. } => Self::Admin,
            Ask::TzAt { .. } | Ask::TzOf { .. } => Self::Timezone,
        }
    }

    /// Its name, stable: for logs, meters and rate-limit keys.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Lookup => "lookup",
            Self::Reverse => "reverse",
            Self::Near => "near",
            Self::Search => "search",
            Self::Hierarchy => "hierarchy",
            Self::Children => "children",
            Self::Admin => "admin",
            Self::Timezone => "timezone",
            Self::Elevation => "elevation",
            Self::Postal => "postal",
            Self::Boundary => "boundary",
            Self::Distance => "distance",
            Self::Coverage => "coverage",
            Self::Install => "install",
            Self::Packs => "packs",
        }
    }
}

/// What one answered request used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// The results it answered.
    pub results: usize,
    /// The bytes of its body.
    pub bytes: usize,
    /// The time it took, microseconds.
    pub micros: u64,
}

/// Who is asking.
pub trait Identify: Send + Sync {
    /// The caller of `request`.
    ///
    /// # Errors
    /// [`crate::query::Code::Unauthenticated`] for credentials missing or not valid.
    fn identify(&self, request: &dyn Incoming) -> Result<Caller, QueryError>;
}

/// May they.
pub trait Authorize: Send + Sync {
    /// Whether `caller` may do `op` now.
    ///
    /// # Errors
    /// [`crate::query::Code::Forbidden`], or [`crate::query::Code::RateLimited`] past a rate
    /// or a quota.
    fn authorize(&self, caller: &Caller, op: Operation) -> Result<(), QueryError>;
}

/// How much they used.
pub trait Meter: Send + Sync {
    /// Records what `caller` used doing `op`. Must not block.
    fn record(&self, caller: &Caller, op: Operation, usage: Usage);
}

/// The open defaults: anyone, anything, nothing counted.
#[derive(Clone, Copy, Debug, Default)]
pub struct Open;

impl Identify for Open {
    fn identify(&self, _: &dyn Incoming) -> Result<Caller, QueryError> {
        Ok(Caller::default())
    }
}

impl Authorize for Open {
    fn authorize(&self, _: &Caller, _: Operation) -> Result<(), QueryError> {
        Ok(())
    }
}

impl Meter for Open {
    fn record(&self, _: &Caller, _: Operation, _: Usage) {}
}

/// The seams a server calls, each open unless set.
#[derive(Clone)]
pub struct Seams {
    /// Who is asking.
    pub identify: Arc<dyn Identify>,
    /// May they.
    pub authorize: Arc<dyn Authorize>,
    /// How much they used.
    pub meter: Arc<dyn Meter>,
}

impl Default for Seams {
    fn default() -> Self {
        Self {
            identify: Arc::new(Open),
            authorize: Arc::new(Open),
            meter: Arc::new(Open),
        }
    }
}

impl core::fmt::Debug for Seams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Seams").finish_non_exhaustive()
    }
}

impl Seams {
    /// Identifies the caller of `request` and authorizes `op` for them: what both servers do
    /// before running a query.
    ///
    /// # Errors
    /// The seams'.
    pub fn admit(&self, request: &dyn Incoming, op: Operation) -> Result<Caller, QueryError> {
        let caller = self.identify.identify(request)?;
        self.authorize.authorize(&caller, op)?;
        Ok(caller)
    }
}
