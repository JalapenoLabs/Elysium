// Copyright © 2026 Jalapeno Labs

//! Who is making a request, and whether they may.
//!
//! Ory Kratos owns identities: passwords, passkeys, authenticator apps, lookup codes,
//! sessions, and recovery. Browsers talk to it directly, through nginx at `/api/identity/`,
//! for every sign-in and account change. The API reads the session cookie Kratos sets, asks
//! Kratos whose it is ([`sessions`]), and maps the identity to a row in `users`
//! (`crate::models::user`), which holds the role and status Elysium decides. Ending a session is
//! the one thing browsers ask the API for instead, so the API can forget what it cached about
//! it at once. See `docs/auth.md` for the whole design.
//!
//! Requests pass through two layers ([`middleware`]):
//!
//! 1. [`middleware::authenticate`] resolves the session and the user, and refuses requests
//!    without one. It covers `/api/v1/me`, which a pending person may reach.
//! 2. [`middleware::require_access`] refuses pending and disabled people, and people who
//!    must enroll an authenticator first. It covers every other `/api/v1` route.
//!
//! Handlers then name who they act for with [`CurrentUser`], [`MemberUser`] for the knowledge
//! base, which guests may not reach, or [`AdminUser`] for what only admins may do.

pub mod authenticator_change;
pub mod hook_key;
pub mod identity_schemas;
pub mod kratos;
pub mod middleware;
pub mod revocable_cache;
#[cfg(test)]
mod revocation_tests;
pub mod sessions;

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use secrecy::SecretString;
use url::Url;
use uuid::Uuid;

use self::kratos::{AssuranceLevel, Kratos};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::user::User;
use crate::oauth::hydra::Hydra;
use crate::oauth::registration::Registration;

/// What the auth layers need. Cloning is cheap.
#[derive(Debug, Clone)]
pub struct Auth {
    pub kratos: Kratos,
    /// OAuth for MCP clients. See `crate::oauth`.
    pub hydra: Hydra,
    /// MCP clients' own registrations, relayed to Hydra's public API. See
    /// `crate::oauth::registration`.
    pub registration: Registration,
    /// The key Kratos's webhooks and courier messages carry. See [`hook_key`].
    pub hook_key: SecretString,
    /// The origin browsers reach Elysium at. Unsafe requests must come from it.
    pub public_url: Url,
}

/// The person a request is for, and how they signed in. Inserted by
/// [`middleware::authenticate`].
#[derive(Debug, Clone)]
pub struct Principal {
    pub user: User,
    pub assurance: AssuranceLevel,
    /// The Kratos session the request came with.
    pub session_id: Uuid,
    /// The Kratos identity that session signs in.
    pub identity_id: Uuid,
}

/// The person a handler acts for: signed in, approved, and active.
#[derive(Debug, Clone)]
pub struct CurrentUser(pub User);

impl CurrentUser {
    pub const fn id(&self) -> uuid::Uuid {
        self.0.id
    }

    /// The actor history records for what this person does.
    pub const fn actor(&self) -> Actor {
        Actor::User(self.0.id)
    }
}

impl<State: Send + Sync> FromRequestParts<State> for CurrentUser {
    type Rejection = ApiError;

    fn from_request_parts(
        parts: &mut Parts,
        _state: &State,
    ) -> impl Future<Output = Result<Self, ApiError>> + Send {
        let user = match parts.extensions.get::<Principal>() {
            Some(principal) => Ok(Self(principal.user.clone())),
            // A route mounted outside the auth layers: a programming error, never a client's.
            None => Err(ApiError::Internal(anyhow::anyhow!(
                "{} {} has no authenticated principal; mount it behind the auth middleware",
                parts.method,
                parts.uri.path(),
            ))),
        };
        std::future::ready(user)
    }
}

/// The person a handler acts for, who must be an active admin.
#[derive(Debug, Clone)]
pub struct AdminUser(pub User);

impl AdminUser {
    pub const fn id(&self) -> uuid::Uuid {
        self.0.id
    }
}

impl<State: Send + Sync> FromRequestParts<State> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, ApiError> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;
        if user.is_admin() {
            return Ok(Self(user));
        }
        Err(ApiError::AccessRefused(AccessRefusal::AdminOnly))
    }
}

/// The person a handler acts for, who must be an active admin or member, as everything in the
/// knowledge base requires: a guest is outside the company and reaches none of it
/// (`docs/knowledge.md`, Who sees what).
///
/// The MCP server checks its caller with [`MemberUser::check`], so a function taking one has
/// had its caller's role checked whichever way the request arrived.
#[derive(Debug, Clone)]
pub struct MemberUser(pub User);

impl MemberUser {
    /// `user`, once they are found to be one of the company's members.
    ///
    /// # Errors
    /// [`AccessRefusal::MembersOnly`] for a guest, or anyone else who is not an active admin or
    /// member.
    pub fn check(user: User) -> Result<Self, ApiError> {
        if user.is_company_member() {
            return Ok(Self(user));
        }
        Err(ApiError::AccessRefused(AccessRefusal::MembersOnly))
    }

    pub const fn id(&self) -> uuid::Uuid {
        self.0.id
    }
}

impl<State: Send + Sync> FromRequestParts<State> for MemberUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, ApiError> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;
        Self::check(user)
    }
}

/// Why a signed-in request was refused. Each has a stable code the frontend acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessRefusal {
    /// No session, or one that ended.
    Unauthenticated,
    /// Signed in with a password, and the account has an authenticator app to use too.
    SecondFactorRequired,
    /// Signed up, waiting for an admin.
    AccountPending,
    AccountDisabled,
    /// The workspace requires an authenticator app, and this person has not set one up.
    MfaEnrollmentRequired,
    /// A change to how the person signs in, asked of a session that signed in too long ago. They
    /// sign in again, then retry. Kratos asks the same of its own changes.
    RecentSignInRequired,
    AdminOnly,
    /// A guest, asking for what only the company's own people may reach, such as the knowledge
    /// base.
    MembersOnly,
    /// An unsafe request from a page on another origin.
    CrossOrigin,
    /// Someone who may read what they asked to change, and no more: a guest on a meeting shared
    /// with them.
    ReadOnly,
    /// A change only its owner or an admin may make, such as who sees a meeting.
    OwnerOrAdmin,
    /// A change only its owner may make, such as running a meeting's processing.
    OwnerOnly,
}

impl AccessRefusal {
    pub const fn status(self) -> StatusCode {
        match self {
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            _ => StatusCode::FORBIDDEN,
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Unauthenticated => "unauthenticated",
            Self::SecondFactorRequired => "second_factor_required",
            Self::AccountPending => "account_pending",
            Self::AccountDisabled => "account_disabled",
            Self::MfaEnrollmentRequired => "mfa_enrollment_required",
            Self::RecentSignInRequired => "recent_sign_in_required",
            Self::AdminOnly => "admin_only",
            Self::MembersOnly => "members_only",
            Self::CrossOrigin => "cross_origin",
            Self::ReadOnly => "read_only",
            Self::OwnerOrAdmin => "owner_or_admin",
            Self::OwnerOnly => "owner_only",
        }
    }

    pub const fn message(self) -> &'static str {
        match self {
            Self::Unauthenticated => "sign in to continue",
            Self::SecondFactorRequired => "enter the code from your authenticator app to continue",
            Self::AccountPending => "your account is waiting for an admin's approval",
            Self::AccountDisabled => "your account is disabled",
            Self::MfaEnrollmentRequired => {
                "this workspace requires an authenticator app; set one up to continue"
            }
            Self::RecentSignInRequired => "sign in again to change how you sign in",
            Self::AdminOnly => "only an admin can do this",
            Self::MembersOnly => "only the company's members can reach its knowledge base",
            Self::CrossOrigin => "the request came from another site",
            Self::ReadOnly => "you can read this but not change it",
            Self::OwnerOrAdmin => "only its owner or an admin can do this",
            Self::OwnerOnly => "only its owner can do this",
        }
    }
}
