//! Per-kind member-linking policy: the admin control plane over `integration_policies`.
//!
//! Governs **personal linked** integration creation only (see
//! `docs/integrations.md`): a member may link their own account of a kind (their
//! Gmail, their GitHub) exactly while that kind's `members_may_link` toggle is on.
//! The default is off, per kind, so a fresh org starts fully closed and an admin
//! opens kinds deliberately. Managed and org-linked creation are gated by the
//! `manage_integrations` capability and never consult these toggles.
//!
//! Reads and writes mirror the org auth-policy control plane
//! ([`crate::policies`]): admin-gated per org, here through
//! [`Capability::ManageIntegrations`] so the same delegated grant that manages
//! integrations also manages their policies. Enforcement lives in the creation
//! path: [`crate::orchestration::integrations::member_linking_permitted`] reads
//! through [`members_may_link`].

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use uuid::Uuid;

use crate::audit;
use crate::authz::{self, Capability};
use crate::orchestration::integrations::IntegrationKind;
use crate::proto::integration::v1::{
    GetIntegrationPoliciesResponse, IntegrationKind as ProtoKind, IntegrationPolicy,
    UpdateIntegrationPolicyRequest,
};
use crate::protobuf::Protobuf;
use crate::session::SessionUser;
use crate::state::AppState;
use crate::users;

/// Whether `org_id` lets members link personal integrations of `kind`.
///
/// The stored toggle when a row exists; the documented default (**off**) when an
/// admin never touched the kind.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn members_may_link(
    executor: impl sqlx::PgExecutor<'_>,
    org_id: Uuid,
    kind: IntegrationKind,
) -> sqlx::Result<bool> {
    let stored = sqlx::query_scalar::<_, bool>(
        "SELECT members_may_link FROM integration_policies WHERE org_id = $1 AND kind = $2",
    )
    .bind(org_id)
    .bind(kind.as_db())
    .fetch_optional(executor)
    .await?;
    Ok(stored.unwrap_or(false))
}

/// Merges stored rows into the full per-kind policy set, in [`IntegrationKind::ALL`] order.
///
/// Every known kind appears exactly once; a kind without a row carries the
/// default (`members_may_link = false`). A row with an unknown kind smallint
/// (from a future schema) is ignored rather than invented into the set.
fn merge_policy_set(rows: &[(i16, bool)]) -> Vec<IntegrationPolicy> {
    IntegrationKind::ALL
        .into_iter()
        .map(|kind| IntegrationPolicy {
            kind: kind.to_proto() as i32,
            members_may_link: rows
                .iter()
                .find(|(stored_kind, _)| *stored_kind == kind.as_db())
                .is_some_and(|(_, enabled)| *enabled),
        })
        .collect()
}

/// Loads the org's full per-kind policy set, missing kinds defaulting to off.
///
/// # Errors
/// Returns an error if the query fails.
async fn policy_set(
    executor: impl sqlx::PgExecutor<'_>,
    org_id: Uuid,
) -> sqlx::Result<Vec<IntegrationPolicy>> {
    let rows = sqlx::query_as::<_, (i16, bool)>(
        "SELECT kind, members_may_link FROM integration_policies WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_all(executor)
    .await?;
    Ok(merge_policy_set(&rows))
}

/// `GET /api/v1/orgs/{id}/integration-policies`: the full per-kind toggle set.
///
/// Admin-gated through [`Capability::ManageIntegrations`]. Always the complete
/// set, one entry per known kind, so the console renders the whole board without
/// guessing at defaults.
pub async fn get_policies(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
) -> Response {
    if let Err(response) =
        authz::require_capability(&state, &principal, org_id, Capability::ManageIntegrations).await
    {
        return response;
    }
    match policy_set(&state.pool, org_id).await {
        Ok(policies) => Protobuf(GetIntegrationPoliciesResponse { policies }).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "integration-policy read failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `PATCH /api/v1/orgs/{id}/integration-policies`: set one kind's toggle.
///
/// Admin-gated through [`Capability::ManageIntegrations`]. Upserts the row so
/// the first flip of a kind needs no prior state, records an audit entry, and
/// answers with the full refreshed set, the same shape the GET returns.
pub async fn update_policy(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(org_id): Path<Uuid>,
    Protobuf(request): Protobuf<UpdateIntegrationPolicyRequest>,
) -> Response {
    if let Err(response) =
        authz::require_capability(&state, &principal, org_id, Capability::ManageIntegrations).await
    {
        return response;
    }
    let Some(kind) = ProtoKind::try_from(request.kind)
        .ok()
        .and_then(IntegrationKind::from_proto)
    else {
        return (StatusCode::BAD_REQUEST, "kind is required").into_response();
    };

    let written = sqlx::query(
        "INSERT INTO integration_policies (org_id, kind, members_may_link)
         VALUES ($1, $2, $3)
         ON CONFLICT (org_id, kind)
         DO UPDATE SET members_may_link = EXCLUDED.members_may_link",
    )
    .bind(org_id)
    .bind(kind.as_db())
    .bind(request.members_may_link)
    .execute(&state.pool)
    .await;
    if let Err(error) = written {
        tracing::error!(error = %error, "integration-policy update failed");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    if let Ok(Some(actor_id)) = users::id_for_subject(&state.pool, &principal.sub).await {
        audit::record(
            &state.pool,
            org_id,
            actor_id,
            "org.integration_policy_updated",
            None,
            json!({
                "kind": kind.slug(),
                "members_may_link": request.members_may_link,
            }),
        )
        .await;
    }

    match policy_set(&state.pool, org_id).await {
        Ok(policies) => Protobuf(GetIntegrationPoliciesResponse { policies }).into_response(),
        Err(error) => {
            tracing::error!(error = %error, "integration-policy read failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- The merge: full set, defaults, unknown rows -------------------------

    #[test]
    fn an_empty_policy_table_merges_to_every_kind_off() {
        let set = merge_policy_set(&[]);
        assert_eq!(set.len(), IntegrationKind::ALL.len());
        for (policy, kind) in set.iter().zip(IntegrationKind::ALL) {
            assert_eq!(policy.kind, kind.to_proto() as i32);
            assert!(!policy.members_may_link, "{kind:?} defaults to off");
        }
    }

    #[test]
    fn a_stored_toggle_flips_exactly_its_own_kind() {
        let set = merge_policy_set(&[(IntegrationKind::Mail.as_db(), true)]);
        for policy in &set {
            let expected = policy.kind == ProtoKind::Mail as i32;
            assert_eq!(policy.members_may_link, expected);
        }
        // An explicit off row reads the same as no row at all.
        let set = merge_policy_set(&[(IntegrationKind::Mail.as_db(), false)]);
        assert!(set.iter().all(|policy| !policy.members_may_link));
    }

    #[test]
    fn an_unknown_stored_kind_is_ignored_not_invented() {
        let set = merge_policy_set(&[(9, true)]);
        assert_eq!(set.len(), IntegrationKind::ALL.len());
        assert!(set.iter().all(|policy| !policy.members_may_link));
    }

    // --- DB-backed toggle behavior + the handler gate ------------------------
    //
    // Ignored by default (needs a reachable postgres), like the matrices in
    // `integrations.rs`. The pure lookup runs inside a rolled-back transaction;
    // the handler test drives the real endpoints through the pool, so it commits
    // under unique ids and deletes the org (cascading its rows) at the end.

    use std::sync::Arc;

    use axum::body::to_bytes;
    use opaque_ke::ServerSetup;
    use prost::Message;
    use rand::rngs::OsRng;
    use sqlx::{PgConnection, PgPool};

    use crate::crypto::Crypto;
    use crate::opaque::Suite;
    use crate::orchestration::integrations::{member_linking_permitted, IntegrationRow};
    use crate::orchestration::provisioner::ProvisionerHandle;
    use crate::proto::integration::v1::{
        CreateIntegrationRequest, IntegrationMode as ProtoMode, OwnerScope as ProtoScope,
    };
    use crate::session::Principal;
    use crate::sse::SseRegistry;

    async fn connect_test_pool() -> PgPool {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");
        pool
    }

    /// Builds an [`AppState`] around a real pool, as `storage::files` does.
    fn test_state_with_pool(pool: PgPool) -> AppState {
        AppState {
            pool,
            sse: SseRegistry::default(),
            webauthn: crate::webauthn::build("localhost", "http://localhost:5173")
                .expect("the test WebAuthn RP builds"),
            opaque_setup: Arc::new(ServerSetup::<Suite>::new(&mut OsRng)),
            crypto: Crypto::new(&[0u8; 32]).expect("a 32-byte test key is valid"),
            redis_url: None,
            redis: None,
            http: reqwest::Client::new(),
            geoip_endpoint: None,
            rabbitmq_url: None,
            app_origin: "http://localhost:5173".to_string(),
            data_dir: "/srv/elysium".to_string(),
            provisioner: ProvisionerHandle::in_memory(),
        }
    }

    async fn insert_org(conn: &mut PgConnection, tag: &str) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>("INSERT INTO orgs (name, slug) VALUES ($1, $2) RETURNING id")
            .bind(format!("{tag} Org"))
            .bind(format!("{tag}-{unique}"))
            .fetch_one(conn)
            .await
            .expect("insert org")
    }

    /// Inserts a user and returns its id and subject (the session's `sub`).
    async fn insert_user(conn: &mut PgConnection, tag: &str) -> (Uuid, String) {
        let subject = format!("{tag}-{}", Uuid::new_v4());
        let id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(&subject)
        .bind(format!("{subject}@example.test"))
        .fetch_one(conn)
        .await
        .expect("insert user");
        (id, subject)
    }

    async fn add_org_member(conn: &mut PgConnection, org: Uuid, user: Uuid, role: &str) {
        sqlx::query("INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, $3)")
            .bind(user)
            .bind(org)
            .bind(role)
            .execute(conn)
            .await
            .expect("add org member");
    }

    fn principal(subject: &str, active_org_id: Option<Uuid>) -> Principal {
        Principal {
            sub: subject.to_string(),
            email: String::new(),
            username: String::new(),
            active_org_id,
        }
    }

    /// Decodes a handler response body as the full policy-set message.
    async fn decode_policy_set(response: Response) -> GetIntegrationPoliciesResponse {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a readable response body");
        GetIntegrationPoliciesResponse::decode(body).expect("a policy-set body")
    }

    /// The stored toggle for one kind out of a decoded policy set.
    fn toggle_for(set: &GetIntegrationPoliciesResponse, kind: IntegrationKind) -> bool {
        set.policies
            .iter()
            .find(|policy| policy.kind == kind.to_proto() as i32)
            .expect("the set covers every kind")
            .members_may_link
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn member_linking_defaults_off_and_follows_the_toggle() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");

        let org = insert_org(&mut tx, "pol").await;

        // No rows at all: every kind is closed, through both the raw lookup and
        // the enforcement seam the create handler uses.
        for kind in IntegrationKind::ALL {
            assert!(!members_may_link(&mut *tx, org, kind).await.unwrap());
            assert!(!member_linking_permitted(&mut tx, org, kind).await.unwrap());
        }

        // Turning mail on opens exactly mail; git stays closed.
        sqlx::query(
            "INSERT INTO integration_policies (org_id, kind, members_may_link)
             VALUES ($1, $2, TRUE)",
        )
        .bind(org)
        .bind(IntegrationKind::Mail.as_db())
        .execute(&mut *tx)
        .await
        .expect("enable mail linking");
        assert!(
            member_linking_permitted(&mut tx, org, IntegrationKind::Mail)
                .await
                .unwrap()
        );
        assert!(
            !member_linking_permitted(&mut tx, org, IntegrationKind::Git)
                .await
                .unwrap()
        );

        // Another org's toggle never leaks across org boundaries.
        let other = insert_org(&mut tx, "other").await;
        assert!(
            !member_linking_permitted(&mut tx, other, IntegrationKind::Mail)
                .await
                .unwrap()
        );

        tx.rollback().await.expect("rollback transaction");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn policy_endpoints_gate_admins_and_open_member_linking() {
        let pool = connect_test_pool().await;
        let state = test_state_with_pool(pool.clone());
        let mut conn = pool.acquire().await.expect("acquire");

        let org = insert_org(&mut conn, "polgate").await;
        let (admin_id, admin_sub) = insert_user(&mut conn, "admin").await;
        add_org_member(&mut conn, org, admin_id, "admin").await;
        let (member_id, member_sub) = insert_user(&mut conn, "member").await;
        add_org_member(&mut conn, org, member_id, "member").await;
        // Release the setup connection: the create handler holds one of the test
        // pool's two connections while it queries through the pool for authz.
        drop(conn);

        // A plain member may neither read nor write the policies: 403 on both.
        let denied = get_policies(
            State(state.clone()),
            SessionUser(principal(&member_sub, Some(org))),
            Path(org),
        )
        .await;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        let denied = update_policy(
            State(state.clone()),
            SessionUser(principal(&member_sub, Some(org))),
            Path(org),
            Protobuf(UpdateIntegrationPolicyRequest {
                kind: ProtoKind::Mail as i32,
                members_may_link: true,
            }),
        )
        .await;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        // The admin reads the full default set: one entry per kind, all off.
        let response = get_policies(
            State(state.clone()),
            SessionUser(principal(&admin_sub, Some(org))),
            Path(org),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let set = decode_policy_set(response).await;
        assert_eq!(set.policies.len(), IntegrationKind::ALL.len());
        assert!(set.policies.iter().all(|policy| !policy.members_may_link));

        // With every kind closed, the member's personal-linked create is refused.
        let personal_mail_create = || {
            Protobuf(CreateIntegrationRequest {
                project_id: None,
                kind: ProtoKind::Mail as i32,
                mode: ProtoMode::Linked as i32,
                owner_scope: ProtoScope::Personal as i32,
                name: "My Gmail".to_string(),
                visibility: 0,
                config_json: String::new(),
                credential: String::new(),
            })
        };
        let refused = crate::orchestration::integrations::create(
            State(state.clone()),
            SessionUser(principal(&member_sub, Some(org))),
            personal_mail_create(),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);

        // The admin flips mail on; the response is the refreshed full set.
        let response = update_policy(
            State(state.clone()),
            SessionUser(principal(&admin_sub, Some(org))),
            Path(org),
            Protobuf(UpdateIntegrationPolicyRequest {
                kind: ProtoKind::Mail as i32,
                members_may_link: true,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let set = decode_policy_set(response).await;
        assert!(toggle_for(&set, IntegrationKind::Mail));
        assert!(!toggle_for(&set, IntegrationKind::Git));

        // The very same create now succeeds, and only for the opened kind.
        let permitted = crate::orchestration::integrations::create(
            State(state.clone()),
            SessionUser(principal(&member_sub, Some(org))),
            personal_mail_create(),
        )
        .await;
        assert_eq!(permitted.status(), StatusCode::OK);
        let created = sqlx::query_as::<_, IntegrationRow>(
            "SELECT * FROM integrations WHERE org_id = $1 AND owner_id = $2",
        )
        .bind(org)
        .bind(member_id)
        .fetch_one(&pool)
        .await
        .expect("the personal integration was created");
        assert_eq!(created.kind, IntegrationKind::Mail);

        // An UNSPECIFIED kind in the update is a clean 400.
        let rejected = update_policy(
            State(state.clone()),
            SessionUser(principal(&admin_sub, Some(org))),
            Path(org),
            Protobuf(UpdateIntegrationPolicyRequest {
                kind: ProtoKind::Unspecified as i32,
                members_may_link: true,
            }),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

        // Clean up: deleting the org cascades to its members, policies, and rows.
        sqlx::query("DELETE FROM orgs WHERE id = $1")
            .bind(org)
            .execute(&pool)
            .await
            .expect("cleanup org");
    }
}
