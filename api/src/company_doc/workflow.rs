// Copyright © 2026 Jalapeno Labs

//! The company doc's review: a workflow that reads what was ingested since its last run and
//! proposes changes to the doc, and the schedule that starts it. See `docs/engine.md`, The
//! company doc.
//!
//! - **The workflow** ([`workflow`]) is one step, [`ReviewCompanyDoc`], on one source, the doc
//!   itself, so it has one live run at a time. A run reads the timeline from where the last one
//!   stopped (`crate::models::company_doc::review_cursor`) up to the instant in its input, in
//!   turns of what fits one prompt (`super::review`), at most [`TURNS_PER_REVIEW`] of them; what
//!   is left waits for the next run. Each turn's proposals and the cursor's move to the last
//!   record it read commit together, so a turn that runs twice, after its engine stopped, reads
//!   its records once and proposes nothing twice.
//! - **The schedule** ([`schedule`]) starts a run every [`REVIEW_INTERVAL`] while something new
//!   was ingested, as Elysia, who proposes what it finds. People start one at once from the
//!   Company doc page (`POST /api/v1/company-doc/review`).
//!
//! Nothing a run proposes changes the doc: each proposal waits for a person to approve or reject
//! it. A run's log names how many records it read and what it proposed to which section, never a
//! word of the records, since every member reads every run's log and a record is its owner's.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use chrono::{DateTime, Utc};
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::review::{self, PromptError, Suggestion};
use crate::database::Pool;
use crate::models::company_doc::{
    self, CompanyDocError, DocSection, NewProposal, Proposal, ProposalState, Proposer,
};
use crate::models::step_model_setting::ModelChoice;
use crate::models::timeline_record::{self, TimelineRecord};
use crate::models::user::ELYSIA_USER_ID;
use crate::models::workflow_run::{self, NewRun};
use crate::realtime::{EventBus, ServerEvent};
use crate::runner::events::{RunnerEvent, TurnStatus};
use crate::runner::{ConversationId, Runner, TurnMode, TurnRequest, WorkspacePath};
use crate::workflows::{self, Done, Start, Step, StepContext, StepFailure, Workflow};

/// What a review's run is about, as `workflow_runs.source_kind` and `source_id` name it: the
/// doc, of which there is one.
pub const SOURCE_KIND: &str = "company_doc";
pub const SOURCE_ID: &str = "company_doc";

/// How long after a review starts the next one may. Each is an Opus turn or three over what
/// people said, so four a day keeps the doc current within hours without reading every message
/// as it lands.
pub const REVIEW_INTERVAL: Duration = Duration::from_hours(6);

/// How often the schedule looks whether a review is due.
const SCHEDULE_CHECK_INTERVAL: Duration = Duration::from_mins(10);

/// How far behind now a review reads. A record is stamped when the transaction that ingests it
/// starts, so one committed a moment later can carry an earlier stamp than a record already
/// read; ingest's transactions last seconds, so two minutes leaves none behind.
const INGESTION_MARGIN: chrono::Duration = chrono::Duration::minutes(2);

/// How far back a review reads at most: all of it for the first review, which has no cursor,
/// and what was ingested while the engine was down for a long time. Older records stay in the
/// knowledge index; the doc is made from what is current.
const LOOKBACK: chrono::Duration = chrono::Duration::days(14);

/// The most turns one run takes. A backlog past it is read by the next run, so one run never
/// spends a day's usage at once.
pub const TURNS_PER_REVIEW: usize = 3;

/// The most records one turn is given to fit into its prompt.
const RECORDS_PER_TURN: i64 = 400;

/// Where review turns run in the workspace. Text turns read and write no files.
const REVIEW_DIRECTORY: &str = ".elysium/company-doc";

/// What a review takes: the instant it reads up to, fixed when the run starts, so every attempt
/// reads the same records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewWindow {
    pub through: DateTime<Utc>,
}

/// What a review did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reviewed {
    /// Records read, in every turn.
    pub records: usize,
    /// Proposals stored; one already pending is not counted again.
    pub proposals: usize,
}

/// The review workflow.
pub fn workflow() -> Workflow<ReviewWindow> {
    workflows::declare("company_doc_review")
        .first::<ReviewCompanyDoc>()
        .build()
}

/// A run of the review up to `now`, less [`INGESTION_MARGIN`], started by `created_by`.
pub fn run_for(now: DateTime<Utc>, created_by: Uuid) -> NewRun {
    workflow().new_run(Start {
        source_kind: SOURCE_KIND.to_owned(),
        source_id: SOURCE_ID.to_owned(),
        input: ReviewWindow {
            through: now - INGESTION_MARGIN,
        },
        created_by,
    })
}

/// Reads what was ingested since the last review and proposes changes to the doc. See the
/// module's docs.
#[derive(Clone)]
pub struct ReviewCompanyDoc {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    runner: Runner,
}

impl std::fmt::Debug for ReviewCompanyDoc {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReviewCompanyDoc")
            .finish_non_exhaustive()
    }
}

/// What [`ReviewCompanyDoc`] is built from.
#[derive(Clone)]
pub struct ReviewCompanyDocDeps {
    pub database: Pool,
    pub events: EventBus,
    pub runner: Runner,
}

impl ReviewCompanyDoc {
    pub fn new(deps: ReviewCompanyDocDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                runner: deps.runner,
            }),
        }
    }

    async fn connection(
        &self,
    ) -> Result<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>, StepFailure>
    {
        self.inner
            .database
            .get()
            .await
            .map_err(|error| StepFailure::retryable(format!("No database connection: {error}")))
    }

    /// One turn over `read`: the records after the cursor that fit one prompt, asked about and
    /// proposed from.
    async fn turn(
        &self,
        read: ForTurn,
        model: &ModelChoice,
        context: &StepContext,
    ) -> Result<TurnRead, StepFailure> {
        let prompt = review::prompt(&read.sections, &read.pending, &read.rejected, &read.records)
            .map_err(|error| match error {
            PromptError::DocTooLong | PromptError::NoRecords => StepFailure::permanent(error),
        })?;
        let included = &read.records[..prompt.records_included];
        context.log.line(&format!(
            "Reading {} records ingested since the last review",
            included.len()
        ));

        let answer = self.ask(prompt.text, model).await?;
        let suggestions = review::parse_answer(&answer, included)
            .map_err(|error| StepFailure::retryable(format!("The review's answer: {error}")))?;

        let last = included.last().expect("a prompt holds at least one record");
        let stored = {
            let mut connection = self.connection().await?;
            store(
                &mut connection,
                &read,
                (last.created_at, last.id),
                suggestions,
                context,
            )
            .await
            .map_err(StepFailure::retryable)?
        };
        if stored.proposed > 0 {
            self.inner.events.publish(&ServerEvent::CompanyDocChanged);
        }
        Ok(TurnRead {
            records: included.len(),
            proposed: stored.proposed,
            more: included.len() < read.records.len() || read.records_full,
        })
    }

    /// The review turn, on the step's model, answering the model's final text.
    async fn ask(&self, prompt: String, model: &ModelChoice) -> Result<String, StepFailure> {
        let request = TurnRequest {
            conversation: ConversationId::new(Uuid::now_v7()),
            label: "company_doc:review".to_owned(),
            prompt,
            working_directory: WorkspacePath::new(REVIEW_DIRECTORY)
                .expect("the review directory is a plain relative path"),
            resume: None,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: Some(model.model.clone()),
            effort: Some(model.effort),
            mode: TurnMode::Text {
                system_prompt: review::SYSTEM_PROMPT.to_owned(),
            },
        };
        let mut ticket = self
            .inner
            .runner
            .submit(request)
            .map_err(|_shutting_down| StepFailure::retryable("Elysium is shutting down"))?;
        let outcome = loop {
            match ticket.events.recv().await {
                Some(RunnerEvent::Finished(outcome)) => break outcome,
                Some(_) => {}
                None => {
                    return Err(StepFailure::retryable(
                        "The review turn ended without an outcome",
                    ));
                }
            }
        };
        if outcome.status != TurnStatus::Completed {
            return Err(StepFailure::retryable(anyhow!(
                "The review turn ended {:?}: {}",
                outcome.status,
                outcome.error.unwrap_or_default()
            )));
        }
        Ok(outcome.result.unwrap_or_default())
    }
}

impl Step for ReviewCompanyDoc {
    const NAME: &'static str = "review_company_doc";
    type Input = ReviewWindow;
    type Output = Reviewed;

    async fn run(
        &self,
        input: ReviewWindow,
        context: StepContext,
    ) -> Result<Done<Reviewed>, StepFailure> {
        let window = input;
        let mut reviewed = Reviewed {
            records: 0,
            proposals: 0,
        };
        // Read once there is something to review, then kept for the run's every turn.
        let mut model: Option<ModelChoice> = None;
        for _turn in 0..TURNS_PER_REVIEW {
            let read = {
                let mut connection = self.connection().await?;
                read_for_turn(&mut connection, window)
                    .await
                    .map_err(StepFailure::retryable)?
            };
            let Some(read) = read else {
                break;
            };
            let choice = match model.take() {
                Some(choice) => choice,
                None => context.model().await?,
            };
            let turn = self.turn(read, &choice, &context).await?;
            model = Some(choice);
            reviewed.records += turn.records;
            reviewed.proposals += turn.proposed;
            if !turn.more {
                break;
            }
        }

        if reviewed.records == 0 {
            context
                .log
                .line("Nothing was ingested since the last review");
            return Ok(Done::Skipped {
                reason: "Nothing was ingested since the last review".to_owned(),
            });
        }
        let summary = format!(
            "Read {} records and proposed {} changes",
            reviewed.records, reviewed.proposals
        );
        context.log.line(&summary);
        Ok(Done::Succeeded {
            output: reviewed,
            summary: Some(summary),
        })
    }
}

/// What one turn read and did.
#[derive(Debug, Clone, Copy)]
struct TurnRead {
    records: usize,
    proposed: usize,
    /// Whether records were left for another turn.
    more: bool,
}

/// What a turn reads before it asks: the doc and its proposals, and the records after the
/// cursor, which it started from.
#[derive(Debug)]
struct ForTurn {
    cursor: Option<(DateTime<Utc>, Uuid)>,
    sections: Vec<DocSection>,
    pending: Vec<Proposal>,
    rejected: Vec<Proposal>,
    records: Vec<TimelineRecord>,
    /// Whether the read stopped at [`RECORDS_PER_TURN`], so more may wait.
    records_full: bool,
}

async fn read_for_turn(
    connection: &mut AsyncPgConnection,
    window: ReviewWindow,
) -> diesel::QueryResult<Option<ForTurn>> {
    let cursor = company_doc::review_cursor(connection).await?;
    let records = timeline_record::ingested_between(
        connection,
        cursor,
        window.through - LOOKBACK,
        window.through,
        RECORDS_PER_TURN,
    )
    .await?;
    if records.is_empty() {
        return Ok(None);
    }
    let records_full = i64::try_from(records.len()).is_ok_and(|read| read == RECORDS_PER_TURN);
    Ok(Some(ForTurn {
        cursor,
        sections: company_doc::sections(connection).await?,
        pending: company_doc::proposals(
            connection,
            ProposalState::Pending,
            Some(review::PROPOSALS_SHOWN),
        )
        .await?,
        rejected: company_doc::proposals(
            connection,
            ProposalState::Rejected,
            Some(review::PROPOSALS_SHOWN),
        )
        .await?,
        records,
        records_full,
    }))
}

/// What storing a turn's proposals did.
#[derive(Debug, Clone, Copy)]
struct Stored {
    proposed: usize,
}

/// Stores `suggestions` and moves the cursor from where the turn started to `through`, in one
/// transaction. Another attempt that moved the cursor first already stored this turn's
/// proposals, so nothing is stored again.
async fn store(
    connection: &mut AsyncPgConnection,
    read: &ForTurn,
    through: (DateTime<Utc>, Uuid),
    suggestions: Vec<Suggestion>,
    context: &StepContext,
) -> anyhow::Result<Stored> {
    let log = context.log.clone();
    let run_id = context.run_id;
    let cursor = read.cursor;
    let refused = rejected_changes(read, &suggestions);
    connection
        .transaction(async move |connection| {
            if !company_doc::advance_review_cursor(connection, cursor, through).await? {
                log.line("Another attempt already read these records; nothing was proposed");
                return Ok(Stored { proposed: 0 });
            }
            let mut proposed = 0;
            for (index, suggestion) in suggestions.into_iter().enumerate() {
                let section = suggestion.section;
                if refused.contains(&index) {
                    log.line(&format!(
                        "Left out a change to {} that people already rejected",
                        section.as_str()
                    ));
                    continue;
                }
                let new = NewProposal {
                    section,
                    edits: suggestion.edits,
                    reason: suggestion.reason,
                    quote: suggestion.quote,
                    source: suggestion.source,
                    timeline_record_id: suggestion.timeline_record_id,
                };
                let elysia = Proposer::Elysia {
                    workflow_run_id: Some(run_id),
                };
                match company_doc::propose(connection, new, elysia, Utc::now()).await {
                    Ok(stored) if stored.created => {
                        proposed += 1;
                        log.line(&format!("Proposed a change to {}", section.as_str()));
                    }
                    Ok(_already) => log.line(&format!(
                        "The change to {} is already waiting for review",
                        section.as_str()
                    )),
                    Err(CompanyDocError::Database(error)) => return Err(error.into()),
                    // The model's edit missed the text, changed nothing, or ran long: its
                    // proposal is left out, and the reason is safe to log.
                    Err(refusal) => log.line(&format!(
                        "Left out a change to {}: {refusal}",
                        section.as_str()
                    )),
                }
            }
            Ok(Stored { proposed })
        })
        .await
}

/// The positions of `suggestions` whose change is one people already rejected: the section as
/// the edits would leave it is the text of a rejected proposal to it.
fn rejected_changes(read: &ForTurn, suggestions: &[Suggestion]) -> BTreeSet<usize> {
    let mut refused = BTreeSet::new();
    for (index, suggestion) in suggestions.iter().enumerate() {
        let Some(current) = read
            .sections
            .iter()
            .find(|section| section.section == suggestion.section)
        else {
            continue;
        };
        let Ok(content) = company_doc::apply_edits(&current.content, &suggestion.edits) else {
            continue;
        };
        let was_rejected = read.rejected.iter().any(|rejected| {
            rejected.section == suggestion.section.as_str() && rejected.content == content
        });
        if was_rejected {
            refused.insert(index);
        }
    }
    refused
}

/// Starts a review whenever one is due, until shutdown: every [`REVIEW_INTERVAL`] after the
/// last one started, while something was ingested since it read. A run still live, waiting on
/// a person after it failed, holds the next one back, as every workflow's live run does.
pub fn schedule(database: Pool, events: EventBus, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut checks = tokio::time::interval(SCHEDULE_CHECK_INTERVAL);
        checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return,
                _tick = checks.tick() => {}
            }
            if let Err(error) = start_when_due(&database, &events).await {
                event!(
                    name: "company_doc.review.schedule_failed",
                    Level::WARN,
                    error.message = %format!("{error:#}"),
                    "could not check whether a company doc review is due; checking again later",
                );
            }
        }
    })
}

async fn start_when_due(database: &Pool, events: &EventBus) -> anyhow::Result<()> {
    let mut connection = database.get().await?;
    let now = Utc::now();
    let newest =
        workflow_run::newest(&mut connection, workflow().name(), SOURCE_KIND, SOURCE_ID).await?;
    let interval = chrono::Duration::from_std(REVIEW_INTERVAL)?;
    if !is_due(newest.as_ref(), now, interval) {
        return Ok(());
    }
    let window = ReviewWindow {
        through: now - INGESTION_MARGIN,
    };
    let cursor = company_doc::review_cursor(&mut connection).await?;
    let unread = timeline_record::ingested_between(
        &mut connection,
        cursor,
        window.through - LOOKBACK,
        window.through,
        1,
    )
    .await?;
    if unread.is_empty() {
        return Ok(());
    }
    let started = workflows::start(&mut connection, events, run_for(now, ELYSIA_USER_ID)).await?;
    event!(
        name: "company_doc.review.started",
        Level::INFO,
        workflow_run.id = %started.settled.run.id,
        "started a company doc review",
    );
    Ok(())
}

/// Whether a review may start, given the newest run: none ran yet, or the last one finished and
/// started at least `interval` ago.
fn is_due(
    newest: Option<&workflow_run::WorkflowRun>,
    now: DateTime<Utc>,
    interval: chrono::Duration,
) -> bool {
    let Some(newest) = newest else {
        return true;
    };
    newest.state == workflow_run::WorkflowRunState::Succeeded && newest.created_at + interval <= now
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::models::company_doc::{Edit, Section};
    use crate::models::workflow_run::{WorkflowRun, WorkflowRunState};

    #[test]
    fn a_review_run_is_about_the_doc_and_reads_up_to_a_moment_ago() {
        let now = Utc::now();
        let run = run_for(now, ELYSIA_USER_ID);

        assert_eq!(run.workflow, "company_doc_review");
        assert_eq!(run.steps, vec!["review_company_doc".to_owned()]);
        assert_eq!(
            (run.source_kind.as_str(), run.source_id.as_str()),
            (SOURCE_KIND, SOURCE_ID)
        );
        let window: ReviewWindow = serde_json::from_value(run.input).expect("reads");
        assert_eq!(window.through, now - INGESTION_MARGIN);
    }

    fn run(state: WorkflowRunState, created_at: DateTime<Utc>) -> WorkflowRun {
        WorkflowRun {
            id: Uuid::now_v7(),
            workflow: "company_doc_review".to_owned(),
            source_kind: SOURCE_KIND.to_owned(),
            source_id: SOURCE_ID.to_owned(),
            state,
            input: json!({}),
            error: None,
            created_by: ELYSIA_USER_ID,
            created_at,
            started_at: None,
            finished_at: None,
            updated_at: created_at,
        }
    }

    #[test]
    fn a_review_is_due_an_interval_after_the_last_one_finished_and_never_while_one_is_live() {
        let now = Utc::now();
        let interval = chrono::Duration::hours(6);

        assert!(
            is_due(None, now, interval),
            "the first review is due at once"
        );
        let earlier = run(
            WorkflowRunState::Succeeded,
            now - chrono::Duration::hours(7),
        );
        assert!(is_due(Some(&earlier), now, interval));
        let recent = run(
            WorkflowRunState::Succeeded,
            now - chrono::Duration::hours(1),
        );
        assert!(!is_due(Some(&recent), now, interval));
        let failed = run(WorkflowRunState::Failed, now - chrono::Duration::days(2));
        assert!(
            !is_due(Some(&failed), now, interval),
            "a failed run waits for a person"
        );
    }

    #[test]
    fn a_change_people_rejected_is_left_out() {
        let now = Utc::now();
        let sections = Section::ALL
            .into_iter()
            .map(|section| DocSection {
                section,
                content: "- ACME".to_owned(),
                updated_at: now,
            })
            .collect();
        let rejected = Proposal {
            id: Uuid::now_v7(),
            section: "glossary".to_owned(),
            base_content: "- ACME".to_owned(),
            content: "- ACME\n- Acme Corp".to_owned(),
            reason: "Seen in mail".to_owned(),
            quote: None,
            source: None,
            timeline_record_id: None,
            workflow_run_id: None,
            state: "rejected".to_owned(),
            decided_by: Some(Uuid::now_v7()),
            decided_at: Some(now),
            created_by: ELYSIA_USER_ID,
            agent: None,
            agent_name: None,
            created_at: now,
        };
        let read = ForTurn {
            cursor: None,
            sections,
            pending: Vec::new(),
            rejected: vec![rejected],
            records: Vec::new(),
            records_full: false,
        };
        let suggestion = |section, replace: &str| Suggestion {
            section,
            edits: vec![Edit {
                find: String::new(),
                replace: replace.to_owned(),
            }],
            reason: "Why".to_owned(),
            quote: None,
            source: None,
            timeline_record_id: None,
        };

        let refused = rejected_changes(
            &read,
            &[
                suggestion(Section::Glossary, "- Acme Corp"),
                suggestion(Section::Glossary, "- Beta"),
                suggestion(Section::People, "- Acme Corp"),
            ],
        );

        assert_eq!(refused, BTreeSet::from([0]));
    }

    #[test]
    fn the_engine_registers_the_step_the_review_names() {
        let manager = diesel_async::pooled_connection::AsyncDieselConnectionManager::new(
            "postgres://unused.invalid/none",
        );
        let unconnected = Pool::builder(manager)
            .build()
            .expect("a pool builds without connecting");
        let docker = bollard::Docker::connect_with_http(
            "http://127.0.0.1:9",
            5,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("a static address parses");
        let runner = Runner::new(
            crate::runner::workspace::WorkspaceContainer::new(
                docker,
                "elysium-workspace:test".to_owned(),
            ),
            unconnected.clone(),
            Arc::new(crate::test_support::cipher()),
            crate::activity::Activities::new(),
            crate::claude_accounts::TurnSignal::default(),
            &CancellationToken::new(),
        );
        let mut steps = crate::workflows::Steps::new();
        steps.register(ReviewCompanyDoc::new(ReviewCompanyDocDeps {
            database: unconnected,
            events: EventBus::in_process(),
            runner,
        }));
        // Panics, as the engine would at startup, if the two drift apart.
        steps.assert_runs(&workflow());
    }
}
