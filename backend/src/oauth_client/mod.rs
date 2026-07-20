//! Elysium as an OAuth *client* for linked services (GitHub/Google/Microsoft).
//!
//! This is the mirror image of the OIDC provider ([`crate::oidc`]): there Elysium is
//! the identity authority a managed service logs in against; here Elysium holds a
//! user's grant to an *external* provider so a member can link their own account and
//! Elysium (and Elysia) act on their behalf, silently refreshing the grant so the
//! user is never re-prompted. See `docs/integrations.md` (Identity: linked services
//! invert the trust direction) and `docs/oauth-client.md`.
//!
//! The subsystem runs the standard Authorization-Code (+ PKCE where supported)
//! *client* flow, split so the fragile parts are pure and unit-testable:
//!
//! - [`providers`] is the hardcoded per-provider metadata table plus authorize-URL,
//!   PKCE, and userinfo-parsing logic (no I/O).
//! - [`store`] is the database layer, sealing access/refresh tokens at rest bound to
//!   each account's identity, and deriving whether a provider is `configured`.
//! - [`tokens`] makes the outbound provider calls behind a mockable seam, and
//!   exposes [`tokens::resolve_connected_token`], the silent-refresh access-token
//!   seam the integration surfaces call.
//! - [`endpoints`] is the HTTP surface (member connect/list/disconnect, the public
//!   callback, and the admin provider-config endpoints).
//!
//! Tokens are sealed with [`crate::crypto`], never returned to a client or logged.

pub mod endpoints;
pub mod providers;
pub mod store;
pub mod tokens;
