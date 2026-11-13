// Copyright © 2026 Jalapeno Labs

//! Starts each project's Project Manager when what woke it is due, and records how its turns end.
//!
//! The dispatcher looks when it is signalled ([`Signal::ProjectManagers`]), when a turn it runs
//! ends, when the next project's wakes go quiet ([`super::queue`]), and every [`TICK`] in case a
//! signal was lost. Each look starts a turn for every project that is due, the longest waiting
//! first, while fewer than [`MAX_CONCURRENT_TURNS`] run, each in a task of its own. A project's
//! turns run one at a time, which the database holds too (`project_manager_turns_one_running`).
//!
//! A turn that ends records its outcome: the session it left, kept for the next turn to resume;
//! its reply and summary; and, when it did its work, a record on the project's timeline. A turn
//! refused because every credential is spent holds its project until the limit resets and gives
//! its wakes back; so does one the engine stopped in the middle of, failed at the next start
//! ([`run`]), so no wake is lost to a restart. A turn that failed of itself keeps its wakes, since
//! another try at once would most likely fail the same way; its failure shows beside them.
//!
//! No turn starts without the configuration repository (`crate::agent_configuration`): while it
//! holds Project Managers back, their wakes wait and one inbox item says why.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::prompt::{
    self, PromptAssignment, PromptBatch, PromptBrief, PromptBriefEdit, PromptMessage, PromptPage,
    PromptProject, PromptPromise, PromptRecord, PromptWork, TurnContext, Writes,
};
use super::queue;
use super::runner::{ManagerRunner, ManagerTurn, Ran, TurnLog};
use super::tasks;
use super::timeline;
use super::trackers::{self, TrackerSource};
use crate::agent_configuration;
use crate::agent_models::{self, Agent};
use crate::database::Pool;
use crate::elysia::prompt::read_back;
use crate::elysia::runner::{ElysiaOutcome, RunnerError};
use crate::meetings::attachments;
use crate::models::attribution::ELYSIA_ACTOR;
use crate::models::meeting_reads;
use crate::models::project::Project;
use crate::models::project_manager::{self, Finished, Turn, TurnStatus, Wake, WakeKind};
use crate::models::promise::{self, PromiseState, Work};
use crate::models::user::{self, ELYSIA_USER_ID};
use crate::models::work_batch::worker::Wait;
use crate::models::{
    action_item, action_item_assignment, action_item_link, page_project, project, project_brief,
    timeline_record, work_batch, worker_setup,
};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::tools::grants::Grants;
use crate::tools::{self, CallScope, ProjectManagerScope};

/// How often the dispatcher looks without being signalled.
pub const TICK: Duration = Duration::from_secs(30);

/// The most Project Manager turns running at once, across every project. They share the
/// runner's eight turn slots with coding sessions, Elysia, and summaries, and a meeting about
/// many projects wakes all of their Project Managers together; two keeps most slots for the
/// work people are watching.
pub const MAX_CONCURRENT_TURNS: usize = 2;

/// The most records and batch results one turn takes. Messages are always taken whole. The
/// rest waits for the next turn, which is due as soon as this one ends; forty keeps each
/// record's excerpt in the prompt.
pub const MAX_WAKES_PER_TURN: i64 = 40;

/// The most promises owed a follow-up that one turn carries beyond those that woke it. A promise
/// whose work finished is listed every turn until it is drafted, so a turn that failed never
/// strands one; this bounds the prompt should many come due at once.
const MAX_DUE_PROMISES_PER_TURN: i64 = 20;

/// The error recorded for a turn the engine stopped in the middle of.
const INTERRUPTED_ERROR: &str =
    "Elysium stopped while this turn ran; what woke it waits for the next turn";

/// The longest error, reply, and summary stored, matching their columns.
const MAX_ERROR_CHARACTERS: usize = 2_000;
const MAX_REPLY_CHARACTERS: usize = 20_000;
const MAX_SUMMARY_CHARACTERS: usize = 1_000;

/// What the dispatcher needs. Cheap to clone; clones share every connection.
#[derive(Clone)]
pub struct DispatchContext {
    pub database: Pool,
    pub events: EventBus,
    pub runner: ManagerRunner,
    /// Where each turn's grant to its tools is minted.
    pub grants: Grants,
}

/// Runs every Project Manager's turns until `shutdown`.
pub async fn run(context: DispatchContext, shutdown: CancellationToken) {
    if let Err(error) = recover_interrupted(&context).await {
        event!(
            name: "project_managers.recover.failure",
            Level::ERROR,
            error.message = %format!("{error:#}"),
            "could not fail the turns the last stop left running",
        );
    }

    // Taken before the first look, so a wake written during any look wakes the next.
    let mut wake = context
        .events
        .wake_on(|signal| *signal == Signal::ProjectManagers);
    let mut running: JoinSet<()> = JoinSet::new();
    loop {
        let next_due_at = match dispatch(&context, &mut running, &shutdown).await {
            Ok(next_due_at) => next_due_at,
            Err(error) => {
                event!(
                    name: "project_managers.dispatch.failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "the dispatcher failed; it looks again on its next tick",
                );
                None
            }
        };
        let until_due = next_due_at
            .and_then(|at| (at - Utc::now()).to_std().ok())
            .map_or(TICK, |until| until.min(TICK));
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = wake.woken() => {}
            Some(_) = running.join_next(), if !running.is_empty() => {}
            () = tokio::time::sleep(until_due) => {}
        }
    }
    // Each turn's task sees the same cancellation and leaves its turn running, for the next start
    // to fail and give its wakes back.
    while running.join_next().await.is_some() {}
    event!(
        name: "project_managers.dispatcher.stopped",
        Level::INFO,
        "the Project Managers' dispatcher stopped",
    );
}

async fn recover_interrupted(context: &DispatchContext) -> anyhow::Result<()> {
    let mut connection = context.database.get().await?;
    let interrupted =
        project_manager::fail_interrupted_turns(&mut connection, INTERRUPTED_ERROR).await?;
    for turn in &interrupted {
        event!(
            name: "project_managers.turn.interrupted",
            Level::WARN,
            project.id = %turn.project_id,
            project_manager.turn.id = %turn.id,
            "a turn the last stop left running was failed; its wakes wait for the next turn",
        );
        context.events.publish(&ServerEvent::ProjectManagerChanged {
            project_id: turn.project_id,
        });
    }
    Ok(())
}

/// Starts a turn for every project that is due while slots are free, and answers when the next
/// project not due yet will be.
pub(super) async fn dispatch(
    context: &DispatchContext,
    running: &mut JoinSet<()>,
    shutdown: &CancellationToken,
) -> anyhow::Result<Option<chrono::DateTime<Utc>>> {
    let mut connection = context.database.get().await?;
    let now = Utc::now();
    let waiting = project_manager::waiting(&mut connection).await?;
    if waiting.is_empty() {
        return Ok(None);
    }
    let busy: HashSet<Uuid> = project_manager::running_projects(&mut connection)
        .await?
        .into_iter()
        .collect();
    let held: HashMap<Uuid, chrono::DateTime<Utc>> = project_manager::held(&mut connection, now)
        .await?
        .into_iter()
        .collect();
    let plan = queue::plan(&waiting, &busy, &held, now);
    if plan.due.is_empty() {
        return Ok(plan.next_due_at);
    }
    // No Project Manager runs without the configuration repository: its wakes wait, and the
    // configuration keeper signals once it is fetched.
    if let Some(hold) = agent_configuration::current_hold(&mut connection).await? {
        if let Err(error) =
            agent_configuration::raise_hold(&mut connection, &context.events, &hold).await
        {
            event!(
                name: "project_managers.configuration_hold.raise_failure",
                Level::WARN,
                error.message = %format!("{error:#}"),
                "could not raise the inbox item for turns the configuration holds back",
            );
        }
        event!(
            name: "project_managers.configuration_held",
            Level::DEBUG,
            hold = ?hold,
            "no Project Manager starts without the configuration repository",
        );
        return Ok(plan.next_due_at);
    }

    for project_id in plan.due {
        if running.len() >= MAX_CONCURRENT_TURNS || shutdown.is_cancelled() {
            // A turn's end looks again.
            break;
        }
        let Some((turn, wakes)) =
            project_manager::start_turn(&mut connection, project_id, MAX_WAKES_PER_TURN).await?
        else {
            continue;
        };
        event!(
            name: "project_managers.turn.started",
            Level::INFO,
            project.id = %project_id,
            project_manager.turn.id = %turn.id,
            project_manager.turn.wakes = wakes.len(),
            "a Project Manager's turn started",
        );
        context
            .events
            .publish(&ServerEvent::ProjectManagerChanged { project_id });
        running.spawn(follow_turn(context.clone(), turn, wakes, shutdown.clone()));
    }
    Ok(plan.next_due_at)
}

/// Runs `turn` and records how it ended. A shutdown leaves it running, for the next start.
async fn follow_turn(
    context: DispatchContext,
    turn: Turn,
    wakes: Vec<Wake>,
    shutdown: CancellationToken,
) {
    let carried = tokio::select! {
        biased;
        () = shutdown.cancelled() => return,
        carried = carry_out(&context, &turn, &wakes) => carried,
    };
    if let Err(error) = carried {
        event!(
            name: "project_managers.turn.failure",
            Level::ERROR,
            project.id = %turn.project_id,
            project_manager.turn.id = %turn.id,
            error.message = %format!("{error:#}"),
            "a Project Manager's turn could not be carried out",
        );
        let error = format!("Elysium could not carry out this turn: {error:#}");
        if let Err(recording) = fail(&context, &turn, &error).await {
            event!(
                name: "project_managers.turn.record_failure",
                Level::ERROR,
                project_manager.turn.id = %turn.id,
                error.message = %format!("{recording:#}"),
                "a failed turn could not be recorded; the next start fails it",
            );
        }
    }
}

/// Builds the turn's prompt, runs it, and records its outcome.
async fn carry_out(context: &DispatchContext, turn: &Turn, wakes: &[Wake]) -> anyhow::Result<()> {
    let project_id = turn.project_id;
    let mut connection = context.database.get().await?;
    let turn_context = load_context(&mut connection, turn, wakes).await?;
    // Read as the turn starts, so a change to the setting applies from the next turn.
    let model = agent_models::for_agent(&mut connection, Agent::ProjectManager, Some(project_id))
        .await?
        .choice;
    let resume = project_manager::find(&mut connection, project_id)
        .await?
        .and_then(|manager| manager.runner_session_id);
    drop(connection);

    let scope = CallScope::ProjectManager(ProjectManagerScope {
        project_id,
        turn_id: turn.id,
    });
    let grant = match context.grants.grant(&scope, &tool_servers()).await {
        Ok(grant) => grant,
        Err(error) => {
            let error = format!("Its tools could not be granted: {error:#}");
            return fail(context, turn, &error).await;
        }
    };
    let ran = context
        .runner
        .run(ManagerTurn {
            project_id,
            turn_id: turn.id,
            prompt: prompt::render(&turn_context, Utc::now()),
            instructions: prompt::INSTRUCTIONS.to_owned(),
            resume,
            mcp_servers: grant.mcp_servers(),
            model,
        })
        .await;
    grant.revoke().await;
    record_outcome(context, turn, ran).await
}

/// A Project Manager's tool servers: its own, its project's work, and the knowledge base.
fn tool_servers() -> [&'static tools::ToolServer; 3] {
    [
        &tools::project_manager::SERVER,
        &tools::work::SERVER,
        &tools::knowledge::SERVER,
    ]
}

/// Everything the turn's wakes carry, read as it starts.
async fn load_context(
    connection: &mut AsyncPgConnection,
    turn: &Turn,
    wakes: &[Wake],
) -> anyhow::Result<TurnContext> {
    let project = project::find(connection, turn.project_id).await?;
    let names = user::names(connection).await?;
    let SortedWakes {
        messages,
        brief_edits,
        record_ids,
        batch_ids,
        promise_ids,
        assigned_ids,
        task_ids,
    } = sort_wakes(wakes, &names);
    let assignments = load_assignments(connection, turn.project_id, &assigned_ids, &names).await?;
    let tasks = tasks::load(connection, turn.project_id, &task_ids, &names).await?;
    // Every meeting whose words the prompt carries, which everything the turn writes is held to.
    let mut read: Vec<Uuid> = tasks.iter().map(|task| task.meeting_id).collect();

    // A record taken off the project since it woke it is left out: it is no longer news here.
    let attributed =
        timeline_record::attributed_among(connection, turn.project_id, &record_ids).await?;
    let attributed =
        meeting_reads::for_agent(connection, attributed, |(record, _attribution)| record).await?;
    let mut records: Vec<PromptRecord> = Vec::with_capacity(attributed.len());
    for ((record, attribution), meetings) in attributed {
        read.extend(meetings);
        // What is pinned in a meeting's excerpt travels with it here.
        let pinned = attachments::pinned_in(connection, &record)
            .await?
            .iter()
            .map(attachments::prompt_line)
            .collect();
        records.push(PromptRecord {
            id: record.id,
            summary: record.metadata["summary"].as_str().map(str::to_owned),
            source_kind: record.source_kind,
            occurred_at: record.occurred_at,
            actor: record.actor,
            title: record.title,
            body: record.body,
            confidence: attribution.confidence,
            attributed_by_person: attribution.attributed_by != ELYSIA_ACTOR,
            pinned,
        });
    }
    records.sort_by_key(|record| record.occurred_at);
    let mut waiting_by_batch: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for waiting in promise::waiting_on_batches(connection, &batch_ids).await? {
        if let Some(batch_id) = waiting.work_batch_id {
            waiting_by_batch
                .entry(batch_id)
                .or_default()
                .push(waiting.id);
        }
    }
    let batches = work_batch::find_many(connection, &batch_ids)
        .await?
        .into_iter()
        .map(|batch| PromptBatch {
            waiting_promises: waiting_by_batch.remove(&batch.id).unwrap_or_default(),
            question: batch
                .waiting_detail
                .filter(|_detail| batch.waiting_on == Some(Wait::Question)),
            id: batch.id,
            title: batch.title,
            state: batch.state,
            result: batch.result,
        })
        .collect();
    let (promises, promised_in) =
        load_promises(connection, turn.project_id, promise_ids, &names).await?;
    read.extend(promised_in);
    let reading = meeting_reads::Turn::ProjectManager(turn.id);
    meeting_reads::keep(connection, reading, &read).await?;

    // Wakes from before the project's previous turn started, which that turn failed without
    // doing, were handed back to this one.
    let previous = project_manager::list_turns(connection, turn.project_id, 2)
        .await?
        .into_iter()
        .find(|candidate| candidate.id != turn.id);
    let retried = previous.is_some_and(|previous| {
        previous.status == TurnStatus::Failed
            && wakes
                .iter()
                .any(|wake| wake.created_at < previous.started_at)
    });

    Ok(TurnContext {
        writes: Writes {
            autonomy: project.autonomy,
            tracker: tracker_brief(connection, turn.project_id).await?,
        },
        project: prompt_project(connection, project).await?,
        messages,
        assignments,
        tasks,
        brief_edits,
        records,
        batches,
        promises,
        retried,
        still_waiting: project_manager::waiting_count(connection, turn.project_id).await?,
    })
}

/// The items a turn was woken to plan, each with who assigned it and what it is linked to. An item
/// unassigned since, or whose lead moved to another project, is left out: it is no longer this
/// Project Manager's to plan.
async fn load_assignments(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    item_ids: &[Uuid],
    names: &HashMap<Uuid, String>,
) -> anyhow::Result<Vec<PromptAssignment>> {
    let items = action_item_assignment::assigned_among(connection, project_id, item_ids).await?;
    let mut assignments = Vec::with_capacity(items.len());
    for item in items {
        let links = action_item_link::list_for_item(connection, item.id)
            .await?
            .into_iter()
            .map(|link| format!("{} ({})", link.external_key, link.url))
            .collect();
        let (Some(assigned_at), Some(assigned_by)) =
            (item.manager_assigned_at, item.manager_assigned_by)
        else {
            event!(
                name: "project_managers.assignment.unreadable",
                Level::WARN,
                project_id = %project_id,
                item_id = %item.id,
                "an item read as assigned to {{project_id}} holds no assignment; its turn leaves it out",
            );
            continue;
        };
        assignments.push(PromptAssignment {
            item_id: item.id,
            title: item.title,
            notes: item.notes,
            state: item.state,
            assigned_by: names
                .get(&assigned_by)
                .cloned()
                .unwrap_or_else(|| "Someone".to_owned()),
            assigned_at,
            links,
        });
    }
    Ok(assignments)
}

/// A turn's wakes, sorted by kind: the messages and brief edits it is told whole, and the ids of
/// the records, batches, promises, assigned items, and meetings' tasks it reads as it starts.
struct SortedWakes {
    messages: Vec<PromptMessage>,
    brief_edits: Vec<PromptBriefEdit>,
    record_ids: Vec<Uuid>,
    batch_ids: Vec<Uuid>,
    promise_ids: Vec<Uuid>,
    assigned_ids: Vec<Uuid>,
    task_ids: Vec<Uuid>,
}

/// Sorts `wakes` by kind in one pass, naming each message's and brief edit's author from `names`.
fn sort_wakes(wakes: &[Wake], names: &HashMap<Uuid, String>) -> SortedWakes {
    let mut sorted = SortedWakes {
        messages: Vec::new(),
        brief_edits: Vec::new(),
        record_ids: Vec::new(),
        batch_ids: Vec::new(),
        promise_ids: Vec::new(),
        assigned_ids: Vec::new(),
        task_ids: Vec::new(),
    };
    for wake in wakes {
        let author = || {
            names
                .get(&wake.created_by)
                .cloned()
                .unwrap_or_else(|| "Someone".to_owned())
        };
        match wake.kind {
            WakeKind::Message => sorted.messages.push(PromptMessage {
                id: wake.id,
                author: author(),
                from_elysia: wake.created_by == ELYSIA_USER_ID,
                body: wake.body.clone().unwrap_or_default(),
                sent_at: wake.created_at,
            }),
            WakeKind::Record => sorted.record_ids.extend(wake.timeline_record_id),
            WakeKind::Batch => sorted.batch_ids.extend(wake.work_batch_id),
            WakeKind::Promise => sorted.promise_ids.extend(wake.promise_id),
            WakeKind::Brief => {
                let edit = wake.page_version.map(|version| PromptBriefEdit {
                    author: author(),
                    version,
                    published_at: wake.created_at,
                });
                sorted.brief_edits.extend(edit);
            }
            WakeKind::Assignment => sorted.assigned_ids.extend(wake.action_item_id),
            WakeKind::Task => sorted.task_ids.extend(wake.meeting_task_id),
        }
    }
    sorted
}

/// `project` as a turn is told it: what it is, the repositories it declares, its brief, and its
/// other docs.
async fn prompt_project(
    connection: &mut AsyncPgConnection,
    project: Project,
) -> anyhow::Result<PromptProject> {
    let repositories = worker_setup::find(connection, project.id)
        .await?
        .repositories
        .into_iter()
        .map(|repository| repository.url)
        .collect();
    let brief = project_brief::find(connection, project.id).await?;
    // The brief is carried whole, so it is not listed again among the docs.
    let pages = page_project::pages_of(connection, project.id)
        .await?
        .into_iter()
        .filter(|linked| linked.page.id != brief.id)
        .map(|linked| PromptPage {
            id: linked.page.id,
            title: linked.page.title,
            published_at: linked.published_at,
        })
        .collect();
    Ok(PromptProject {
        id: project.id,
        name: project.name,
        description: project.description,
        repositories,
        brief: PromptBrief {
            page_id: brief.id,
            version: brief.current_version,
            goals: project_brief::goals(&brief.body),
            body: brief.body,
        },
        pages,
    })
}

/// The promises a turn acts on: those that woke it, and every one of the project's still owed a
/// follow-up, each with where it was made and the work it waits on, and the meetings the records
/// they were made in name. A promise the person already decided on, one taken off the project,
/// and one made in an excerpt naming no meeting are left out.
async fn load_promises(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    mut ids: Vec<Uuid>,
    names: &HashMap<Uuid, String>,
) -> anyhow::Result<(Vec<PromptPromise>, Vec<Uuid>)> {
    let owed = promise::list_for_project(
        connection,
        project_id,
        &[PromiseState::Due],
        MAX_DUE_PROMISES_PER_TURN,
    )
    .await?;
    ids.extend(owed.iter().map(|due| due.id));
    ids.sort_unstable();
    ids.dedup();

    let mut found = promise::find_many(connection, &ids).await?;
    found.retain(|candidate| {
        candidate.project_id == project_id
            && matches!(candidate.state, PromiseState::Waiting | PromiseState::Due)
    });
    found.sort_by_key(|candidate| candidate.id);

    let record_ids: Vec<Uuid> = found.iter().map(|made| made.timeline_record_id).collect();
    let made_in = timeline_record::find_many(connection, &record_ids).await?;
    let made_in = meeting_reads::for_agent(connection, made_in, |record| record).await?;
    let mut read = Vec::new();
    let mut records: HashMap<Uuid, timeline_record::TimelineRecord> = HashMap::new();
    for (record, meetings) in made_in {
        read.extend(meetings);
        records.insert(record.id, record);
    }
    let mut batch_ids = Vec::new();
    let mut item_ids = Vec::new();
    for made in &found {
        match made.work() {
            Some(Work::Batch(batch_id)) => batch_ids.push(batch_id),
            Some(Work::Item(item_id)) => item_ids.push(item_id),
            None => {}
        }
    }
    let mut works: HashMap<Uuid, PromptWork> = HashMap::new();
    for batch in work_batch::find_many(connection, &batch_ids).await? {
        works.insert(
            batch.id,
            PromptWork::Batch {
                id: batch.id,
                title: batch.title,
                state: batch.state,
                result: batch.result,
            },
        );
    }
    for item_id in item_ids {
        let item = action_item::find(connection, item_id).await?;
        works.insert(
            item.id,
            PromptWork::Item {
                id: item.id,
                title: item.title,
                state: item.state,
            },
        );
    }

    let mut promises = Vec::with_capacity(found.len());
    for made in found {
        let Some(record) = records.get(&made.timeline_record_id) else {
            continue;
        };
        let work_id = match made.work() {
            Some(Work::Batch(id) | Work::Item(id)) => Some(id),
            None => None,
        };
        promises.push(PromptPromise {
            id: made.id,
            person: names
                .get(&record.created_by)
                .cloned()
                .unwrap_or_else(|| "The person".to_owned()),
            recipient: made.recipient,
            summary: made.summary,
            record_id: record.id,
            record_title: record.title.clone(),
            said: record.body.clone(),
            state: made.state,
            work: work_id.and_then(|id| works.get(&id).cloned()),
        });
    }
    Ok((promises, read))
}

/// Where the project's issues file, as its prompt says it, or why there is nowhere.
async fn tracker_brief(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> diesel::QueryResult<Result<String, String>> {
    let resolved = trackers::resolve(connection, project_id).await?;
    Ok(resolved
        .map(|tracker| {
            let source = match tracker.source {
                TrackerSource::Named => "which this project names",
                TrackerSource::Precedent => "where this project's issues already are",
            };
            format!(
                "{} {}, {source}",
                tracker.place.provider.name(),
                tracker.place.destination
            )
        })
        .map_err(str::to_owned))
}

/// Records how the turn ended, and announces it.
async fn record_outcome(
    context: &DispatchContext,
    turn: &Turn,
    ran: Result<Ran, RunnerError>,
) -> anyhow::Result<()> {
    let Ran { outcome, log } = match ran {
        Ok(ran) => ran,
        Err(error) => return fail(context, turn, &error.to_string()).await,
    };
    let mut connection = context.database.get().await?;
    let (ElysiaOutcome::Completed { session_id, .. }
    | ElysiaOutcome::UsageLimited { session_id, .. }
    | ElysiaOutcome::Failed { session_id, .. }) = &outcome;
    if let Some(session_id) = session_id {
        project_manager::set_runner_session_id(&mut connection, turn.project_id, session_id)
            .await?;
    }

    match outcome {
        ElysiaOutcome::Completed { result, .. } => {
            record_completed(context, &mut connection, turn, result.as_deref(), &log).await?;
        }
        ElysiaOutcome::UsageLimited { resets_at, .. } => {
            project_manager::hold_until(&mut connection, turn.project_id, resets_at).await?;
            // The page names when the hold lifts, in the reader's own time zone.
            let error = "Every Claude credential reached its usage limit. What woke this turn \
                         waits for the next one, once the limit resets.";
            project_manager::finish_turn(
                &mut connection,
                turn.id,
                Finished::Failed { error, retry: true },
                log.as_str(),
            )
            .await?;
        }
        ElysiaOutcome::Failed { error, .. } => {
            let error = truncate(&error, MAX_ERROR_CHARACTERS);
            project_manager::finish_turn(
                &mut connection,
                turn.id,
                Finished::Failed {
                    error: &error,
                    retry: false,
                },
                log.as_str(),
            )
            .await?;
        }
    }
    context.events.publish(&ServerEvent::ProjectManagerChanged {
        project_id: turn.project_id,
    });
    Ok(())
}

/// Records a turn that did its work: its reply and summary, and its record on the project's
/// timeline, in one transaction, then announces the record.
async fn record_completed(
    context: &DispatchContext,
    connection: &mut AsyncPgConnection,
    turn: &Turn,
    result: Option<&str>,
    log: &TurnLog,
) -> anyhow::Result<()> {
    let read = read_back(result.unwrap_or_default());
    let reply = read
        .reply
        .map(|reply| truncate(&reply, MAX_REPLY_CHARACTERS));
    // A turn that wrote no summary line is headed by its reply's first line.
    let summary = read
        .summary
        .or_else(|| {
            reply
                .as_deref()
                .and_then(|reply| reply.lines().next())
                .map(str::to_owned)
        })
        .map(|summary| truncate(&summary, MAX_SUMMARY_CHARACTERS));
    let record = connection
        .transaction(async |connection| {
            let record = match &summary {
                Some(summary) => Some(
                    timeline::record_turn(
                        connection,
                        turn.project_id,
                        turn.id,
                        summary,
                        reply.as_deref(),
                        Utc::now(),
                    )
                    .await?,
                ),
                None => None,
            };
            project_manager::finish_turn(
                connection,
                turn.id,
                Finished::Succeeded {
                    summary: summary.as_deref(),
                    reply: reply.as_deref(),
                    timeline_record_id: record.as_ref().map(|record| record.id),
                },
                log.as_str(),
            )
            .await?;
            diesel::QueryResult::Ok(record)
        })
        .await?;
    if let Some(record) = record {
        context.events.publish(&ServerEvent::TimelineRecordCreated {
            id: record.id,
            source_kind: record.source_kind,
            created_by: record.created_by,
            action_item_id: None,
            conversation_id: None,
        });
    }
    event!(
        name: "project_managers.turn.finished",
        Level::INFO,
        project.id = %turn.project_id,
        project_manager.turn.id = %turn.id,
        "a Project Manager's turn finished",
    );
    Ok(())
}

/// Fails `turn` with `error`, and announces it. Its wakes stay with it: trying again at once
/// would most likely fail the same way, and the failure shows on the project's page beside what
/// woke it.
async fn fail(context: &DispatchContext, turn: &Turn, error: &str) -> anyhow::Result<()> {
    let mut connection = context.database.get().await?;
    let error = truncate(error, MAX_ERROR_CHARACTERS);
    project_manager::finish_turn(
        &mut connection,
        turn.id,
        Finished::Failed {
            error: &error,
            retry: false,
        },
        "",
    )
    .await?;
    context.events.publish(&ServerEvent::ProjectManagerChanged {
        project_id: turn.project_id,
    });
    Ok(())
}

fn truncate(text: &str, characters: usize) -> String {
    text.chars().take(characters).collect()
}
