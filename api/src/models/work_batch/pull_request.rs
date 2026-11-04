// Copyright © 2026 Jalapeno Labs

//! The pull requests a batch's worker opened, as it recorded them, and where each stands. See
//! `docs/engine.md`, Merging.
//!
//! A worker records each pull request it opens with its `worker_pull_request` tool
//! ([`record`]), which first checks with GitHub that the project's account opened it for this
//! batch. One pull request is one batch's: a second batch naming it is refused. The engine
//! looks at every open one as its worker's wait ends and before its batch finishes
//! (`crate::worker_pool::merging`): it merges one whose repository lets Elysium and whose checks
//! all pass, records why it left one open ([`hold`]), and records a merge whoever made it
//! ([`mark_merged`]), exactly once, since only an `open` row moves to `merged`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::schema::work_batch_pull_requests;
use crate::models::user::CODING_AGENT_USER_ID;

diesel::define_sql_function! {
    /// Postgres's `lower`, by which `work_batch_pull_requests_once` names a pull request,
    /// since GitHub names repositories ignoring case.
    fn lower(text: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

/// The longest reason a pull request was left open, matching
/// `work_batch_pull_requests.held_because`.
pub const HELD_MAX_CHARACTERS: usize = 4_000;

/// The longest title kept, matching `work_batch_pull_requests.title`. GitHub's own limit is
/// lower; this only guards the column.
pub const TITLE_MAX_CHARACTERS: usize = 1_000;

/// Where a recorded pull request stands, as Elysium last read it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::PullRequestState"]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState {
    Open,
    Merged,
    /// Closed without merging.
    Closed,
}

/// How Elysium merged a pull request, among the ways GitHub offers.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::PullRequestMergeMethod"]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    /// The method as GitHub's merge call names it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Squash => "squash",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
        }
    }
}

/// One pull request a batch's worker opened.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Serialize)]
#[diesel(table_name = work_batch_pull_requests, check_for_backend(diesel::pg::Pg))]
#[serde(rename_all = "camelCase")]
pub struct BatchPullRequest {
    pub id: Uuid,
    pub work_batch_id: Uuid,
    /// `owner/name`, as GitHub spells it.
    pub repository: String,
    pub number: i32,
    pub url: String,
    pub title: String,
    pub state: PullRequestState,
    pub merged_at: Option<DateTime<Utc>>,
    /// The login GitHub says merged it.
    pub merged_by: Option<String>,
    /// Whether Elysium merged it, rather than a person on GitHub.
    pub merged_by_elysium: bool,
    /// How Elysium merged it, when it did.
    pub merge_method: Option<MergeMethod>,
    /// Why the last look left it open.
    pub held_because: Option<String>,
    pub looked_at: Option<DateTime<Utc>>,
    /// What its merge put on the project's timeline.
    pub timeline_record_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl BatchPullRequest {
    /// `owner/name#12`.
    pub fn reference(&self) -> String {
        format!("{}#{}", self.repository, self.number)
    }

    /// Its repository's URL on github.com, as a project declares repositories.
    pub fn github_url(&self) -> String {
        format!("https://github.com/{}", self.repository)
    }
}

/// A pull request a worker opened, as GitHub reported it when it was recorded.
#[derive(Debug, Clone, Copy)]
pub struct NewPullRequest<'a> {
    pub work_batch_id: Uuid,
    /// `owner/name`, as GitHub spells it.
    pub repository: &'a str,
    pub number: i32,
    pub url: &'a str,
    pub title: &'a str,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = work_batch_pull_requests)]
struct NewRow<'a> {
    id: Uuid,
    work_batch_id: Uuid,
    repository: &'a str,
    number: i32,
    url: &'a str,
    title: String,
    created_by: Uuid,
}

/// Why a pull request could not be recorded for a batch.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// Another batch recorded it first: a pull request is one batch's work.
    #[error("batch {0} recorded this pull request")]
    OtherBatch(Uuid),
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Records `new` for its batch, as the coding agent, and answers the row: the one just stored,
/// or the one the batch recorded before, since a worker may name a pull request again.
///
/// # Errors
/// [`RecordError::OtherBatch`] for a pull request another batch recorded; any database error.
pub async fn record(
    connection: &mut AsyncPgConnection,
    new: NewPullRequest<'_>,
) -> Result<BatchPullRequest, RecordError> {
    let row = NewRow {
        id: Uuid::now_v7(),
        work_batch_id: new.work_batch_id,
        repository: new.repository,
        number: new.number,
        url: new.url,
        title: new.title.chars().take(TITLE_MAX_CHARACTERS).collect(),
        created_by: CODING_AGENT_USER_ID,
    };
    // The one conflict there can be is `work_batch_pull_requests_once`, an expression index
    // Diesel cannot name as a target, so any conflict is read back as that one.
    let inserted = diesel::insert_into(work_batch_pull_requests::table)
        .values(&row)
        .on_conflict_do_nothing()
        .returning(BatchPullRequest::as_returning())
        .get_result(connection)
        .await
        .optional()?;
    if let Some(inserted) = inserted {
        return Ok(inserted);
    }
    let existing: BatchPullRequest = work_batch_pull_requests::table
        .filter(lower(work_batch_pull_requests::repository).eq(new.repository.to_lowercase()))
        .filter(work_batch_pull_requests::number.eq(new.number))
        .select(BatchPullRequest::as_select())
        .first(connection)
        .await?;
    if existing.work_batch_id != new.work_batch_id {
        return Err(RecordError::OtherBatch(existing.work_batch_id));
    }
    Ok(existing)
}

/// Batch `batch_id`'s pull requests, in the order its worker recorded them.
///
/// # Errors
/// Propagates any database error.
pub async fn of_batch(
    connection: &mut AsyncPgConnection,
    batch_id: Uuid,
) -> QueryResult<Vec<BatchPullRequest>> {
    work_batch_pull_requests::table
        .filter(work_batch_pull_requests::work_batch_id.eq(batch_id))
        .order(work_batch_pull_requests::id.asc())
        .select(BatchPullRequest::as_select())
        .load(connection)
        .await
}

/// Records that the look at `at` left open pull request `id` open, saying `reason`, cut to
/// [`HELD_MAX_CHARACTERS`]. A pull request that moved on meanwhile is left as it is.
///
/// # Errors
/// Propagates any database error.
pub async fn hold(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    reason: &str,
    at: DateTime<Utc>,
) -> QueryResult<()> {
    let reason: String = reason.trim().chars().take(HELD_MAX_CHARACTERS).collect();
    diesel::update(
        work_batch_pull_requests::table
            .find(id)
            .filter(work_batch_pull_requests::state.eq(PullRequestState::Open)),
    )
    .set((
        work_batch_pull_requests::held_because.eq(Some(reason).filter(|text| !text.is_empty())),
        work_batch_pull_requests::looked_at.eq(at),
    ))
    .execute(connection)
    .await
    .map(drop)
}

/// Records that open pull request `id` was closed without merging, as the look at `at` read it.
///
/// # Errors
/// Propagates any database error.
pub async fn close(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    at: DateTime<Utc>,
) -> QueryResult<()> {
    diesel::update(
        work_batch_pull_requests::table
            .find(id)
            .filter(work_batch_pull_requests::state.eq(PullRequestState::Open)),
    )
    .set((
        work_batch_pull_requests::state.eq(PullRequestState::Closed),
        work_batch_pull_requests::held_because.eq(None::<String>),
        work_batch_pull_requests::looked_at.eq(at),
    ))
    .execute(connection)
    .await
    .map(drop)
}

/// A merge, as GitHub reported it.
#[derive(Debug, Clone, Copy)]
pub struct Merge<'a> {
    pub at: DateTime<Utc>,
    /// The login GitHub says merged it.
    pub by: &'a str,
    /// How Elysium merged it; `None` for a merge someone made on GitHub.
    pub method: Option<MergeMethod>,
}

/// Records that open pull request `id` merged, answering the row as it now stands, or `None`
/// when it was no longer open: an earlier look recorded the merge already. Call it in the
/// transaction that puts the merge on the timeline, so a merge is recorded once.
///
/// # Errors
/// Propagates any database error.
pub async fn mark_merged(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    merge: Merge<'_>,
) -> QueryResult<Option<BatchPullRequest>> {
    diesel::update(
        work_batch_pull_requests::table
            .find(id)
            .filter(work_batch_pull_requests::state.eq(PullRequestState::Open)),
    )
    .set((
        work_batch_pull_requests::state.eq(PullRequestState::Merged),
        work_batch_pull_requests::merged_at.eq(merge.at),
        work_batch_pull_requests::merged_by.eq(merge.by),
        work_batch_pull_requests::merged_by_elysium.eq(merge.method.is_some()),
        work_batch_pull_requests::merge_method.eq(merge.method),
        work_batch_pull_requests::held_because.eq(None::<String>),
        work_batch_pull_requests::looked_at.eq(Utc::now()),
    ))
    .returning(BatchPullRequest::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Names `record_id` as what pull request `id`'s merge put on the timeline.
///
/// # Errors
/// Propagates any database error.
pub async fn set_timeline_record(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    record_id: Uuid,
) -> QueryResult<()> {
    diesel::update(work_batch_pull_requests::table.find(id))
        .set(work_batch_pull_requests::timeline_record_id.eq(record_id))
        .execute(connection)
        .await
        .map(drop)
}
