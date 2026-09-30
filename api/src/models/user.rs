// Copyright © 2026 Jalapeno Labs

//! Users: the people who sign in, and the machines that act without signing in.
//!
//! A person's password, passkeys, authenticators, and sessions live in Ory Kratos; their row
//! here holds what Elysium decides about them (role and status) and a copy of the email and
//! name Kratos reports, so lists and "created by" never ask Kratos. Machines are seeded by
//! the migration with the fixed ids below. See `docs/auth.md`.
//!
//! Every change an admin makes to an account, or a person makes to their own, is recorded in
//! `user_events` in the same transaction.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::BigInt;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::{Uuid, uuid};

use crate::database::schema::{user_events, users, workspace_settings};

/// Owns what nobody in particular did: rows that predate accounts, what the watcher imports,
/// and the mail server's own records.
pub const SYSTEM_USER_ID: Uuid = uuid!("00000000-0000-0000-0000-000000000001");

/// Elysia, Elysium's AI assistant.
pub const ELYSIA_USER_ID: Uuid = uuid!("00000000-0000-0000-0000-000000000002");

/// Every coding session's agent. History names the session as well (`session:<n>`).
pub const CODING_AGENT_USER_ID: Uuid = uuid!("00000000-0000-0000-0000-000000000003");

/// Serializes the first sign-up: two people signing up at once must not both become the
/// first admin. The value is arbitrary but constant; it spells "Users" in ASCII.
const PROVISION_LOCK_ID: i64 = 0x0055_7365_7273;

/// What a principal may do. Machines hold `Agent` or `System`; people hold the rest.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::UserRole"]
#[serde(rename_all = "kebab-case")]
pub enum UserRole {
    /// Everything, including managing users and workspace settings.
    Admin,
    Member,
    Guest,
    Agent,
    System,
}

/// The roles a person can be given. Requests name one of these, so no request can hand a
/// person a machine's role; permission checks match on [`UserRole`] explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PersonRole {
    Admin,
    Member,
    Guest,
}

impl From<PersonRole> for UserRole {
    fn from(role: PersonRole) -> Self {
        match role {
            PersonRole::Admin => Self::Admin,
            PersonRole::Member => Self::Member,
            PersonRole::Guest => Self::Guest,
        }
    }
}

/// Where a person's account stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::UserStatus"]
#[serde(rename_all = "kebab-case")]
pub enum UserStatus {
    /// Signed up, waiting for an admin.
    Pending,
    Active,
    /// Refused on every request, and by Kratos at sign-in.
    Disabled,
}

/// A stored user.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = users, check_for_backend(diesel::pg::Pg))]
pub struct User {
    pub id: Uuid,
    pub kratos_identity_id: Option<Uuid>,
    pub email: Option<String>,
    pub name: String,
    pub role: Option<UserRole>,
    pub status: Option<UserStatus>,
    pub approved_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    /// True for a person who may use the workspace.
    pub fn is_active(&self) -> bool {
        self.status == Some(UserStatus::Active)
    }

    pub fn is_admin(&self) -> bool {
        self.is_active() && self.role == Some(UserRole::Admin)
    }
}

/// A person as Kratos reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub kratos_identity_id: Uuid,
    /// Lowercased.
    pub email: String,
    pub name: String,
}

/// What happened to an account, as `user_events.kind` records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserEventKind {
    SignedUp,
    Approved,
    Rejected,
    RoleChanged,
    Disabled,
    Enabled,
    SessionsRevoked,
    MfaReset,
    RecoveryLinkCreated,
    /// The person changed their email or name in Kratos, and Elysium saw it.
    ProfileChanged,
    WorkspaceSettingsChanged,
}

impl UserEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SignedUp => "signed_up",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::RoleChanged => "role_changed",
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
            Self::SessionsRevoked => "sessions_revoked",
            Self::MfaReset => "mfa_reset",
            Self::RecoveryLinkCreated => "recovery_link_created",
            Self::ProfileChanged => "profile_changed",
            Self::WorkspaceSettingsChanged => "workspace_settings_changed",
        }
    }
}

/// Settings that apply to the whole workspace.
#[derive(Debug, Clone, Copy, Queryable, Selectable)]
#[diesel(table_name = workspace_settings, check_for_backend(diesel::pg::Pg))]
pub struct WorkspaceSettings {
    pub signup_open: bool,
    pub require_mfa: bool,
    pub updated_by: Uuid,
    pub updated_at: DateTime<Utc>,
}

/// A partial update of the workspace settings. `None` leaves a setting as it is.
#[derive(Debug, Default, Clone, Copy, AsChangeset)]
#[diesel(table_name = workspace_settings)]
pub struct WorkspaceSettingsChanges {
    pub signup_open: Option<bool>,
    pub require_mfa: Option<bool>,
}

/// Why an account change was refused.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    /// The change would leave the workspace without an active admin.
    #[error("the workspace must keep at least one active admin")]
    LastAdmin,
    /// The account is not in a state the change applies to, such as approving someone who
    /// is not pending. The message says which.
    #[error("{0}")]
    Invalid(&'static str),
}

#[derive(Insertable)]
#[diesel(table_name = users)]
struct PersonRow<'a> {
    id: Uuid,
    kratos_identity_id: Uuid,
    email: &'a str,
    name: &'a str,
    role: Option<UserRole>,
    status: UserStatus,
    approved_at: Option<DateTime<Utc>>,
}

#[derive(Insertable)]
#[diesel(table_name = user_events)]
struct EventRow<'a> {
    id: Uuid,
    user_id: Option<Uuid>,
    actor_id: Uuid,
    kind: &'a str,
    data: Value,
}

/// One user by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<User> {
    users::table
        .find(id)
        .select(User::as_select())
        .first(connection)
        .await
}

/// Every user, machines included, people by name. Lists show machines as creators too.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<User>> {
    users::table
        .order((users::name.asc(), users::id.asc()))
        .select(User::as_select())
        .load(connection)
        .await
}

/// Every user's name by id, for showing who wrote a comment or made a change.
///
/// # Errors
/// Propagates any database error.
pub async fn names(connection: &mut AsyncPgConnection) -> QueryResult<HashMap<Uuid, String>> {
    let rows: Vec<(Uuid, String)> = users::table
        .select((users::id, users::name))
        .load(connection)
        .await?;
    Ok(rows.into_iter().collect())
}

/// The user a Kratos identity signs in as, created the first time Elysium sees it, and with
/// its email and name brought up to date.
///
/// The first person ever becomes an active admin; everyone after them waits for approval.
/// Creating the row here, rather than from a Kratos webhook, means a lost webhook can never
/// leave an identity without an account.
///
/// # Errors
/// Propagates any database error.
pub async fn provision(connection: &mut AsyncPgConnection, profile: &Profile) -> QueryResult<User> {
    let existing = users::table
        .filter(users::kratos_identity_id.eq(profile.kratos_identity_id))
        .select(User::as_select())
        .first(connection)
        .await
        .optional()?;
    if let Some(user) = existing {
        return refresh_profile(connection, user, profile).await;
    }

    connection
        .transaction(async move |connection| {
            diesel::sql_query("SELECT pg_advisory_xact_lock($1)")
                .bind::<BigInt, _>(PROVISION_LOCK_ID)
                .execute(connection)
                .await?;

            // Under the lock, a request racing this one for the same identity has either
            // finished (and left the row) or not started.
            let raced = users::table
                .filter(users::kratos_identity_id.eq(profile.kratos_identity_id))
                .select(User::as_select())
                .first(connection)
                .await
                .optional()?;
            if let Some(user) = raced {
                return Ok(user);
            }

            let is_first = !has_people(connection).await?;
            let now = Utc::now();
            let row = if is_first {
                PersonRow {
                    id: Uuid::now_v7(),
                    kratos_identity_id: profile.kratos_identity_id,
                    email: &profile.email,
                    name: &profile.name,
                    role: Some(UserRole::Admin),
                    status: UserStatus::Active,
                    approved_at: Some(now),
                }
            } else {
                PersonRow {
                    id: Uuid::now_v7(),
                    kratos_identity_id: profile.kratos_identity_id,
                    email: &profile.email,
                    name: &profile.name,
                    role: None,
                    status: UserStatus::Pending,
                    approved_at: None,
                }
            };

            let user = diesel::insert_into(users::table)
                .values(row)
                .returning(User::as_returning())
                .get_result(connection)
                .await?;
            record_event(
                connection,
                Some(user.id),
                user.id,
                UserEventKind::SignedUp,
                json!({ "email": profile.email, "firstAdmin": is_first }),
            )
            .await?;
            Ok(user)
        })
        .await
}

/// Copies a changed email or name from Kratos, recording the change.
async fn refresh_profile(
    connection: &mut AsyncPgConnection,
    user: User,
    profile: &Profile,
) -> QueryResult<User> {
    let email_changed = user.email.as_deref() != Some(profile.email.as_str());
    if !email_changed && user.name == profile.name {
        return Ok(user);
    }

    connection
        .transaction(async move |connection| {
            let updated = diesel::update(users::table.find(user.id))
                .set((
                    users::email.eq(&profile.email),
                    users::name.eq(&profile.name),
                ))
                .returning(User::as_returning())
                .get_result(connection)
                .await?;
            record_event(
                connection,
                Some(user.id),
                user.id,
                UserEventKind::ProfileChanged,
                json!({
                    "before": { "email": user.email, "name": user.name },
                    "after": { "email": profile.email, "name": profile.name },
                }),
            )
            .await?;
            Ok(updated)
        })
        .await
}

/// Notes that the user made a request, at most once per `granularity`, so a busy user costs
/// one write every few minutes rather than one per request.
///
/// # Errors
/// Propagates any database error.
pub async fn touch_last_seen(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    now: DateTime<Utc>,
    granularity: chrono::Duration,
) -> QueryResult<()> {
    diesel::update(
        users::table.find(id).filter(
            users::last_seen_at
                .is_null()
                .or(users::last_seen_at.lt(now - granularity)),
        ),
    )
    .set(users::last_seen_at.eq(now))
    .execute(connection)
    .await?;
    Ok(())
}

/// Lets a pending person in with `role`.
///
/// # Errors
/// Returns [`AccountError::Invalid`] unless the user is a pending person.
pub async fn approve(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    role: PersonRole,
    actor_id: Uuid,
) -> Result<User, AccountError> {
    connection
        .transaction(async move |connection| {
            let user = lock(connection, id).await?;
            if user.status != Some(UserStatus::Pending) {
                return Err(AccountError::Invalid(
                    "only a pending sign-up can be approved",
                ));
            }

            let approved = diesel::update(users::table.find(id))
                .set((
                    users::role.eq(Some(UserRole::from(role))),
                    users::status.eq(Some(UserStatus::Active)),
                    users::approved_at.eq(Some(Utc::now())),
                ))
                .returning(User::as_returning())
                .get_result(connection)
                .await?;
            record_event(
                connection,
                Some(id),
                actor_id,
                UserEventKind::Approved,
                json!({ "role": role }),
            )
            .await?;
            Ok(approved)
        })
        .await
}

/// Removes a pending person's account. They created nothing, so nothing names them; the
/// caller deletes their Kratos identity so the address can sign up again.
///
/// # Errors
/// Returns [`AccountError::Invalid`] unless the user is a pending person.
pub async fn reject(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    actor_id: Uuid,
) -> Result<User, AccountError> {
    connection
        .transaction(async move |connection| {
            let user = lock(connection, id).await?;
            if user.status != Some(UserStatus::Pending) {
                return Err(AccountError::Invalid(
                    "only a pending sign-up can be rejected",
                ));
            }

            // Recorded first: the event outlives the row, with its user id cleared.
            record_event(
                connection,
                Some(id),
                actor_id,
                UserEventKind::Rejected,
                json!({ "email": user.email, "name": user.name }),
            )
            .await?;
            diesel::delete(users::table.find(id))
                .execute(connection)
                .await?;
            Ok(user)
        })
        .await
}

/// Gives an active or disabled person another role.
///
/// # Errors
/// Returns [`AccountError::LastAdmin`] when it would demote the last active admin, and
/// [`AccountError::Invalid`] for a machine or a pending person.
pub async fn change_role(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    role: PersonRole,
    actor_id: Uuid,
) -> Result<User, AccountError> {
    connection
        .transaction(async move |connection| {
            let user = lock(connection, id).await?;
            let Some(before) = user.role.filter(|_| user.kratos_identity_id.is_some()) else {
                return Err(AccountError::Invalid(
                    "only an approved person's role can change",
                ));
            };
            let after = UserRole::from(role);
            if before == after {
                return Ok(user);
            }
            if user.is_admin() {
                ensure_another_active_admin(connection, id).await?;
            }

            let changed = diesel::update(users::table.find(id))
                .set(users::role.eq(Some(after)))
                .returning(User::as_returning())
                .get_result(connection)
                .await?;
            record_event(
                connection,
                Some(id),
                actor_id,
                UserEventKind::RoleChanged,
                json!({ "before": before, "after": after }),
            )
            .await?;
            Ok(changed)
        })
        .await
}

/// Disables or re-enables an approved person. The caller also changes their Kratos
/// identity's state and, when disabling, revokes their sessions.
///
/// # Errors
/// Returns [`AccountError::LastAdmin`] when it would disable the last active admin, and
/// [`AccountError::Invalid`] for a machine or a pending person.
pub async fn set_disabled(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    disabled: bool,
    actor_id: Uuid,
) -> Result<User, AccountError> {
    connection
        .transaction(async move |connection| {
            let user = lock(connection, id).await?;
            let status = match (user.status, disabled) {
                (Some(UserStatus::Active | UserStatus::Disabled), true) => UserStatus::Disabled,
                (Some(UserStatus::Active | UserStatus::Disabled), false) => UserStatus::Active,
                _ => {
                    return Err(AccountError::Invalid(
                        "only an approved person can be disabled or enabled",
                    ));
                }
            };
            if user.status == Some(status) {
                return Ok(user);
            }
            if user.is_admin() {
                ensure_another_active_admin(connection, id).await?;
            }

            let changed = diesel::update(users::table.find(id))
                .set(users::status.eq(Some(status)))
                .returning(User::as_returning())
                .get_result(connection)
                .await?;
            let kind = if disabled {
                UserEventKind::Disabled
            } else {
                UserEventKind::Enabled
            };
            record_event(connection, Some(id), actor_id, kind, json!({})).await?;
            Ok(changed)
        })
        .await
}

/// Records an admin's action on an account that only Kratos carries out, such as revoking
/// sessions or resetting an authenticator.
///
/// # Errors
/// Propagates any database error.
pub async fn record_account_action(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    actor_id: Uuid,
    kind: UserEventKind,
) -> QueryResult<()> {
    record_event(connection, Some(id), actor_id, kind, json!({})).await
}

/// The workspace settings.
///
/// # Errors
/// Propagates any database error.
pub async fn workspace_settings(
    connection: &mut AsyncPgConnection,
) -> QueryResult<WorkspaceSettings> {
    workspace_settings::table
        .select(WorkspaceSettings::as_select())
        .first(connection)
        .await
}

/// Applies `changes` to the workspace settings, recording the values before and after.
///
/// # Errors
/// Propagates any database error.
pub async fn update_workspace_settings(
    connection: &mut AsyncPgConnection,
    changes: WorkspaceSettingsChanges,
    actor_id: Uuid,
) -> QueryResult<WorkspaceSettings> {
    connection
        .transaction(async move |connection| {
            let before = workspace_settings::table
                .select(WorkspaceSettings::as_select())
                .for_update()
                .first(connection)
                .await?;
            if changes.signup_open.is_none() && changes.require_mfa.is_none() {
                return Ok(before);
            }

            let after = diesel::update(workspace_settings::table)
                .set((changes, workspace_settings::updated_by.eq(actor_id)))
                .returning(WorkspaceSettings::as_returning())
                .get_result(connection)
                .await?;
            record_event(
                connection,
                None,
                actor_id,
                UserEventKind::WorkspaceSettingsChanged,
                json!({
                    "before": { "signupOpen": before.signup_open, "requireMfa": before.require_mfa },
                    "after": { "signupOpen": after.signup_open, "requireMfa": after.require_mfa },
                }),
            )
            .await?;
            Ok(after)
        })
        .await
}

/// Whether anyone has signed up yet. Until someone has, sign-up is open whatever the setting
/// says, so a new deployment can never lock itself out.
///
/// # Errors
/// Propagates any database error.
pub async fn has_people(connection: &mut AsyncPgConnection) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        users::table.filter(users::kratos_identity_id.is_not_null()),
    ))
    .get_result(connection)
    .await
}

/// Locks one user's row for the rest of the transaction.
async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<User> {
    users::table
        .find(id)
        .select(User::as_select())
        .for_update()
        .first(connection)
        .await
}

/// Refuses a change that would leave no active admin but `id`. Locks every active admin's
/// row, so two admins demoting each other at once cannot both succeed.
async fn ensure_another_active_admin(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> Result<(), AccountError> {
    let admins: Vec<Uuid> = users::table
        .filter(users::role.eq(Some(UserRole::Admin)))
        .filter(users::status.eq(Some(UserStatus::Active)))
        .select(users::id)
        .for_update()
        .load(connection)
        .await?;
    if admins.iter().any(|admin| *admin != id) {
        return Ok(());
    }
    Err(AccountError::LastAdmin)
}

async fn record_event(
    connection: &mut AsyncPgConnection,
    user_id: Option<Uuid>,
    actor_id: Uuid,
    kind: UserEventKind,
    data: Value,
) -> QueryResult<()> {
    diesel::insert_into(user_events::table)
        .values(EventRow {
            id: Uuid::now_v7(),
            user_id,
            actor_id,
            kind: kind.as_str(),
            data,
        })
        .execute(connection)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use secrecy::{ExposeSecret, SecretString};

    use super::*;
    use crate::database::migrations::{self, MigrationCommand};
    use crate::test_support::empty_database;

    /// A migrated database with nobody signed up yet: `migrated_database` adds a test person.
    async fn workspace_without_people() -> (SecretString, AsyncPgConnection) {
        let url = empty_database().await;
        migrations::execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("migrations apply");
        let connection = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("connects");
        (url, connection)
    }

    fn profile(name: &str) -> Profile {
        Profile {
            kratos_identity_id: Uuid::now_v7(),
            email: format!("{}@example.com", name.to_lowercase()),
            name: name.to_owned(),
        }
    }

    async fn event_kinds(connection: &mut AsyncPgConnection, id: Uuid) -> Vec<String> {
        user_events::table
            .filter(user_events::user_id.eq(id))
            .order(user_events::id.asc())
            .select(user_events::kind)
            .load(connection)
            .await
            .expect("events load")
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_first_person_is_the_admin_and_everyone_after_waits() {
        let (_url, mut connection) = workspace_without_people().await;
        assert!(!has_people(&mut connection).await.expect("counts"));

        let ada = profile("Ada");
        let first = provision(&mut connection, &ada).await.expect("first");
        assert_eq!(first.role, Some(UserRole::Admin));
        assert_eq!(first.status, Some(UserStatus::Active));
        assert!(first.approved_at.is_some());

        let second = provision(&mut connection, &profile("Grace"))
            .await
            .expect("second");
        assert_eq!(second.role, None);
        assert_eq!(second.status, Some(UserStatus::Pending));

        let again = provision(&mut connection, &ada).await.expect("again");
        assert_eq!(again.id, first.id, "an identity always maps to one account");
        assert_eq!(event_kinds(&mut connection, first.id).await, ["signed_up"]);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn two_first_sign_ups_at_once_make_one_admin() {
        let (url, mut connection) = workspace_without_people().await;
        let mut other = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("connects");

        let (ada, grace) = (profile("Ada"), profile("Grace"));
        let (left, right) = tokio::join!(
            provision(&mut connection, &ada),
            provision(&mut other, &grace),
        );
        let roles = [left.expect("left").role, right.expect("right").role];
        assert_eq!(
            roles
                .iter()
                .filter(|role| **role == Some(UserRole::Admin))
                .count(),
            1,
            "exactly one becomes the admin: {roles:?}"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_changed_email_or_name_is_copied_and_recorded() {
        let (_url, mut connection) = workspace_without_people().await;
        let mut ada = profile("Ada");
        let first = provision(&mut connection, &ada).await.expect("first");

        ada.email = "ada@lovelace.example".to_owned();
        ada.name = "Ada Lovelace".to_owned();
        let renamed = provision(&mut connection, &ada).await.expect("renamed");
        assert_eq!(renamed.email.as_deref(), Some("ada@lovelace.example"));
        assert_eq!(renamed.name, "Ada Lovelace");
        assert_eq!(
            event_kinds(&mut connection, first.id).await,
            ["signed_up", "profile_changed"]
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn pending_people_are_approved_with_a_role_or_rejected_and_removed() {
        let (_url, mut connection) = workspace_without_people().await;
        let admin = provision(&mut connection, &profile("Ada"))
            .await
            .expect("admin");
        let grace = provision(&mut connection, &profile("Grace"))
            .await
            .expect("grace");
        let mallory = provision(&mut connection, &profile("Mallory"))
            .await
            .expect("mallory");

        let approved = approve(&mut connection, grace.id, PersonRole::Member, admin.id)
            .await
            .expect("approved");
        assert_eq!(approved.role, Some(UserRole::Member));
        assert_eq!(approved.status, Some(UserStatus::Active));
        assert!(matches!(
            approve(&mut connection, grace.id, PersonRole::Admin, admin.id).await,
            Err(AccountError::Invalid(_))
        ));
        assert!(
            matches!(
                reject(&mut connection, grace.id, admin.id).await,
                Err(AccountError::Invalid(_))
            ),
            "an approved person is disabled, never deleted"
        );

        reject(&mut connection, mallory.id, admin.id)
            .await
            .expect("rejected");
        let gone = find(&mut connection, mallory.id).await;
        assert!(
            matches!(gone, Err(diesel::result::Error::NotFound)),
            "{gone:?}"
        );
        let orphaned: Vec<(Option<Uuid>, String)> = user_events::table
            .filter(user_events::kind.eq("rejected"))
            .select((user_events::user_id, user_events::kind))
            .load(&mut connection)
            .await
            .expect("events load");
        assert_eq!(
            orphaned,
            [(None, "rejected".to_owned())],
            "the rejection outlives the account it removed"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_last_active_admin_can_be_neither_demoted_nor_disabled() {
        let (_url, mut connection) = workspace_without_people().await;
        let ada = provision(&mut connection, &profile("Ada"))
            .await
            .expect("ada");
        let grace = provision(&mut connection, &profile("Grace"))
            .await
            .expect("grace");

        assert!(matches!(
            change_role(&mut connection, ada.id, PersonRole::Member, ada.id).await,
            Err(AccountError::LastAdmin)
        ));
        assert!(matches!(
            set_disabled(&mut connection, ada.id, true, ada.id).await,
            Err(AccountError::LastAdmin)
        ));

        approve(&mut connection, grace.id, PersonRole::Admin, ada.id)
            .await
            .expect("a second admin");
        let demoted = change_role(&mut connection, ada.id, PersonRole::Member, grace.id)
            .await
            .expect("with another admin, the first may step down");
        assert_eq!(demoted.role, Some(UserRole::Member));

        let disabled = set_disabled(&mut connection, ada.id, true, grace.id)
            .await
            .expect("a member can be disabled");
        assert_eq!(disabled.status, Some(UserStatus::Disabled));
        let enabled = set_disabled(&mut connection, ada.id, false, grace.id)
            .await
            .expect("and enabled again");
        assert_eq!(enabled.status, Some(UserStatus::Active));
        assert_eq!(
            event_kinds(&mut connection, ada.id).await,
            ["signed_up", "role_changed", "disabled", "enabled"]
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn machines_cannot_be_approved_promoted_or_disabled() {
        let (_url, mut connection) = workspace_without_people().await;
        let ada = provision(&mut connection, &profile("Ada"))
            .await
            .expect("ada");

        for machine in [SYSTEM_USER_ID, ELYSIA_USER_ID, CODING_AGENT_USER_ID] {
            assert!(matches!(
                approve(&mut connection, machine, PersonRole::Admin, ada.id).await,
                Err(AccountError::Invalid(_))
            ));
            assert!(matches!(
                change_role(&mut connection, machine, PersonRole::Admin, ada.id).await,
                Err(AccountError::Invalid(_))
            ));
            assert!(matches!(
                set_disabled(&mut connection, machine, true, ada.id).await,
                Err(AccountError::Invalid(_))
            ));
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn workspace_settings_changes_are_recorded_with_before_and_after() {
        let (_url, mut connection) = workspace_without_people().await;
        let ada = provision(&mut connection, &profile("Ada"))
            .await
            .expect("ada");
        let before = workspace_settings(&mut connection).await.expect("settings");
        assert!(before.signup_open);
        assert!(!before.require_mfa);

        let closing = WorkspaceSettingsChanges {
            signup_open: Some(false),
            require_mfa: None,
        };
        let after = update_workspace_settings(&mut connection, closing, ada.id)
            .await
            .expect("changed");
        assert!(!after.signup_open);
        assert_eq!(after.updated_by, ada.id);

        let recorded: Vec<Value> = user_events::table
            .filter(user_events::kind.eq("workspace_settings_changed"))
            .select(user_events::data)
            .load(&mut connection)
            .await
            .expect("events load");
        assert_eq!(
            recorded,
            [json!({
                "before": { "signupOpen": true, "requireMfa": false },
                "after": { "signupOpen": false, "requireMfa": false },
            })]
        );
    }
}
