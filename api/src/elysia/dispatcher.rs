// Copyright © 2026 Jalapeno Labs

//! Runs Elysia's queued turns, one at a time, and queues idle turns when nothing else is.
//!
//! The dispatcher looks at the queue whenever it is woken ([`super::Elysia::wake`]) and on a
//! [`TICK`]. A turn starts only when nothing holds her back: not paused, no usage limit in
//! force, and a runner available. A person's own message or answer starts whatever her hours
//! say; turns she would start herself (`event`, `idle`) wait for her hours. People go first:
//! a queued message or answer starts before any queued event.
//!
//! When a turn ends, its outcome is recorded: her reply joins the conversation (for messages
//! and answers), its summary is stored for her activity log, and the session she left is kept
//! for the next turn to resume. A turn refused because every credential is spent is queued
//! again for after the limit resets, and nothing else starts before then.
//!
//! With nothing queued for [`IDLE_AFTER`], she revisits her own work ([`revisit_candidates`]):
//! what she gave up on or was blocked on, after [`REVISIT_COOLDOWN`], and what is past the
//! time she said it would be ready.
//!
//! A turn left running when the API stopped is failed at the next start, since nothing is
//! running it any more; its trigger is not lost, because the person's message or the work it
//! was about is still there for the next turn.

use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use diesel_async::AsyncPgConnection;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::availability;
use super::prompt::{self, IdleWork, PromptMessage, TurnContext};
use super::runner::{ElysiaOutcome, ElysiaTurn, RunnerError};
use super::{Elysia, stored_availability, trigger};
use crate::database::Pool;
use crate::models::elysia::{self, NewTurn, Turn, TurnStatus, Work, WorkStatus};
use crate::models::user::{self, ELYSIA_USER_ID, SYSTEM_USER_ID};
use crate::passwords::Passwords;
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::elysia::{MessageResponse, TurnResponse, publish_snapshot};
use crate::sessions::scrub::Scrubber;
use crate::tools::{self, CallScope, ElysiaScope, ToolServer};

/// How often the queue is looked at without being woken. Holds lift on their own (a usage
/// limit resets, her hours begin), and this is how soon she notices.
pub const TICK: Duration = Duration::from_secs(60);

/// How long the queue must have been empty before she revisits her own work.
pub const IDLE_AFTER: TimeDelta = TimeDelta::minutes(30);

/// How long after giving up on, or being blocked on, a piece of work she tries it again.
pub const REVISIT_COOLDOWN: TimeDelta = TimeDelta::hours(2);

/// The error recorded for a turn the API stopped in the middle of.
const INTERRUPTED_ERROR: &str = "Elysium stopped while this turn ran";

/// The longest error stored, matching `elysia_turns.error`'s limit.
const MAX_ERROR_CHARACTERS: usize = 2000;

/// The queued turn to start next, if any may start.
///
/// Nothing starts while one runs. A person's turn goes before turns she would start herself,
/// which wait while she is outside her hours.
pub fn next_turn(active_turns: &[Turn], available: bool) -> Option<&Turn> {
    if active_turns
        .iter()
        .any(|turn| turn.status == TurnStatus::Running)
    {
        return None;
    }
    let mut first_own: Option<&Turn> = None;
    for turn in active_turns {
        if turn.status != TurnStatus::Queued {
            continue;
        }
        if trigger::is_from_person(&turn.trigger_kind) {
            return Some(turn);
        }
        if available && first_own.is_none() {
            first_own = Some(turn);
        }
    }
    first_own
}

/// The work worth revisiting at `now`: what she gave up on or was blocked on and nobody has
/// touched for [`REVISIT_COOLDOWN`], and what is past her estimate of when it would be ready
/// and was not revisited within the cooldown. A revisit touches the work too, so each piece
/// comes back at most once per cooldown.
pub fn revisit_candidates(work: &[Work], now: DateTime<Utc>) -> Vec<&Work> {
    work.iter()
        .filter(|item| match item.status {
            WorkStatus::GaveUp | WorkStatus::Blocked => now - item.updated_at >= REVISIT_COOLDOWN,
            WorkStatus::Working => {
                item.ready_at.is_some_and(|ready_at| ready_at <= now)
                    && item
                        .revisited_at
                        .is_none_or(|revisited_at| now - revisited_at >= REVISIT_COOLDOWN)
            }
            WorkStatus::Ready | WorkStatus::Done => false,
        })
        .collect()
}

/// What the dispatcher needs.
#[derive(Clone)]
pub struct DispatchContext {
    pub database: Pool,
    pub events: EventBus,
    pub elysia: Elysia,
    /// The password vault: its tools are granted to her turns while it is ready, and what she
    /// says is scrubbed of every secret it handed out.
    pub passwords: Passwords,
}

/// Runs the queue until `shutdown`.
pub async fn run(context: DispatchContext, shutdown: CancellationToken) {
    if let Err(error) = recover_interrupted(&context).await {
        event!(
            name: "elysia.dispatcher.recover.failure",
            Level::ERROR,
            error.message = %error,
            "could not fail the turns left running by the last stop",
        );
    }

    loop {
        if let Err(error) = dispatch(&context, &shutdown).await {
            event!(
                name: "elysia.dispatcher.dispatch.failure",
                Level::ERROR,
                error.message = %error,
                "the dispatcher failed; it tries again on its next tick",
            );
        }
        tokio::select! {
            () = shutdown.cancelled() => break,
            () = context.elysia.woken() => {}
            () = tokio::time::sleep(TICK) => {}
        }
    }
    event!(name: "elysia.dispatcher.stopped", Level::INFO, "the dispatcher stopped");
}

async fn recover_interrupted(context: &DispatchContext) -> anyhow::Result<()> {
    let mut connection = context.database.get().await?;
    let interrupted = elysia::fail_interrupted_turns(&mut connection, INTERRUPTED_ERROR).await?;
    for turn in &interrupted {
        event!(
            name: "elysia.turn.interrupted",
            Level::WARN,
            elysia.turn.id = %turn.id,
            "a turn left running by the last stop was failed",
        );
    }
    if !interrupted.is_empty() {
        publish_snapshot(&context.events, &mut connection).await?;
    }
    Ok(())
}

/// Starts queued turns until none may start, then queues an idle turn if one is due.
async fn dispatch(context: &DispatchContext, shutdown: &CancellationToken) -> anyhow::Result<()> {
    while !shutdown.is_cancelled() {
        let mut connection = context.database.get().await?;
        let state = elysia::state(&mut connection).await?;
        let now = Utc::now();
        let usage_limited = state.usage_paused_until.is_some_and(|until| until > now);
        if state.paused || usage_limited {
            return Ok(());
        }

        let available = availability::is_available(&stored_availability(&state), now);
        let active = elysia::active_turns(&mut connection).await?;
        let Some(turn) = next_turn(&active, available) else {
            if active.is_empty() && available {
                queue_idle_turn(context, &mut connection, now).await?;
            }
            return Ok(());
        };
        let turn_id = turn.id;
        drop(connection);

        tokio::select! {
            result = run_turn(context, turn_id, state.runner_session_id) => result?,
            // The turn is left running; the next start fails it (see `recover_interrupted`).
            () = shutdown.cancelled() => return Ok(()),
        }
    }
    Ok(())
}

/// Runs the queued turn `turn_id` and records how it ended.
async fn run_turn(
    context: &DispatchContext,
    turn_id: Uuid,
    resume: Option<String>,
) -> anyhow::Result<()> {
    let mut connection = context.database.get().await?;
    let Some(turn) = elysia::start_turn(&mut connection, turn_id).await? else {
        // Cancelled or started elsewhere since it was read.
        return Ok(());
    };
    publish_turn(context, &turn);
    publish_snapshot(&context.events, &mut connection).await?;

    let Some(turn_context) = load_context(&mut connection, &turn).await? else {
        // A message turn whose messages an earlier turn already answered.
        let finished = elysia::finish_turn(
            &mut connection,
            turn.id,
            Ok(Some("Answered in an earlier turn.")),
        )
        .await?;
        publish_turn(context, &finished);
        publish_snapshot(&context.events, &mut connection).await?;
        return Ok(());
    };
    // The runner may take minutes; no connection is held meanwhile.
    drop(connection);

    // Her tools, for this turn alone: dropping the grant when the turn ends revokes it.
    let grant = context.elysia.grants().grant(
        CallScope::Elysia(ElysiaScope { turn_id: turn.id }),
        &tool_servers(context.passwords.is_ready()),
    );

    event!(
        name: "elysia.turn.started",
        Level::INFO,
        elysia.turn.id = %turn.id,
        elysia.turn.trigger = turn.trigger_kind,
        "elysia turn started",
    );
    let outcome = context
        .elysia
        .runner()
        .run(ElysiaTurn {
            turn_id: turn.id,
            prompt: prompt::render(&turn_context, Utc::now()),
            instructions: prompt::INSTRUCTIONS.to_owned(),
            resume,
            mcp_servers: grant.mcp_servers(),
        })
        .await;
    drop(grant);

    let mut connection = context.database.get().await?;
    let finished = record_outcome(context, &mut connection, &turn, outcome).await?;
    event!(
        name: "elysia.turn.finished",
        Level::INFO,
        elysia.turn.id = %finished.id,
        elysia.turn.status = ?finished.status,
        "elysia turn finished",
    );
    publish_turn(context, &finished);
    publish_snapshot(&context.events, &mut connection).await?;
    Ok(())
}

/// Her tool servers: her own, and the password vault's while it is ready. A vault that becomes
/// ready mid-turn is granted from her next turn.
fn tool_servers(passwords_ready: bool) -> Vec<&'static ToolServer> {
    let mut servers = vec![&tools::elysia::SERVER];
    if passwords_ready {
        servers.push(&tools::passwords::SERVER);
    }
    servers
}

/// Records how a turn ended, and returns it as stored.
async fn record_outcome(
    context: &DispatchContext,
    connection: &mut AsyncPgConnection,
    turn: &Turn,
    outcome: Result<ElysiaOutcome, RunnerError>,
) -> anyhow::Result<Turn> {
    // Nothing she says keeps a password or code the vault handed her in the clear.
    let scrubber = Scrubber::new([]).with_revealed(context.passwords.revealed());
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            let error = truncate(&scrubber.scrub_text(&error.to_string()));
            return Ok(elysia::finish_turn(connection, turn.id, Err(&error)).await?);
        }
    };

    match outcome {
        ElysiaOutcome::Completed { result, session_id } => {
            keep_session(connection, session_id.as_deref()).await?;
            let result = scrubber.scrub_text(result.as_deref().unwrap_or_default());
            let read_back = prompt::read_back(&result);
            if trigger::is_from_person(&turn.trigger_kind)
                && let Some(reply) = &read_back.reply
            {
                let message = elysia::insert_message(connection, reply, ELYSIA_USER_ID).await?;
                context
                    .events
                    .publish(&ServerEvent::ElysiaMessageCreated(MessageResponse::from(
                        message,
                    )));
            }
            Ok(elysia::finish_turn(connection, turn.id, Ok(read_back.summary.as_deref())).await?)
        }
        ElysiaOutcome::UsageLimited {
            resets_at,
            session_id,
        } => {
            keep_session(connection, session_id.as_deref()).await?;
            elysia::set_usage_paused_until(connection, Some(resets_at)).await?;
            let again = elysia::insert_turn(
                connection,
                NewTurn {
                    id: Uuid::now_v7(),
                    trigger_kind: turn.trigger_kind.clone(),
                    trigger_detail: turn.trigger_detail.clone(),
                    created_by: turn.created_by,
                },
            )
            .await?;
            publish_turn(context, &again);
            let error = format!(
                "Every Claude credential reached its usage limit. Queued again for after {}.",
                resets_at.to_rfc3339()
            );
            Ok(elysia::finish_turn(connection, turn.id, Err(&error)).await?)
        }
        ElysiaOutcome::Failed { error, session_id } => {
            keep_session(connection, session_id.as_deref()).await?;
            let error = truncate(&scrubber.scrub_text(&error));
            Ok(elysia::finish_turn(connection, turn.id, Err(&error)).await?)
        }
    }
}

/// Keeps the session a turn left, so the next one resumes it. A turn that reported none
/// leaves the last one in place.
async fn keep_session(
    connection: &mut AsyncPgConnection,
    session_id: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(session_id) = session_id {
        elysia::set_runner_session_id(connection, Some(session_id)).await?;
    }
    Ok(())
}

/// What the prompt carries for `turn`. `None` for a message turn with nothing left to answer.
async fn load_context(
    connection: &mut AsyncPgConnection,
    turn: &Turn,
) -> anyhow::Result<Option<TurnContext>> {
    let names = user::names(connection).await?;
    let name_of = |id: Uuid| {
        names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "Someone".to_owned())
    };

    match turn.trigger_kind.as_str() {
        trigger::MESSAGE => {
            let messages = elysia::messages_since_last_reply(connection).await?;
            if messages.is_empty() {
                return Ok(None);
            }
            Ok(Some(TurnContext::Messages(
                messages
                    .into_iter()
                    .map(|message| PromptMessage {
                        author: name_of(message.created_by),
                        body: message.body,
                        sent_at: message.created_at,
                    })
                    .collect(),
            )))
        }
        trigger::ANSWER => {
            let question_id = detail_id(&turn.trigger_detail, "questionId")?;
            let question = elysia::find_question(connection, question_id).await?;
            Ok(Some(TurnContext::Answer {
                question: question.prompt,
                answer: question.answer.unwrap_or_default(),
                answered_by: question
                    .answered_by
                    .map_or_else(|| "Someone".to_owned(), name_of),
            }))
        }
        trigger::IDLE => {
            let work = elysia::unfinished_work(connection).await?;
            let wanted = detail_ids(&turn.trigger_detail, "workIds");
            Ok(Some(TurnContext::Idle(
                work.into_iter()
                    .filter(|item| wanted.contains(&item.id))
                    .map(|item| IdleWork {
                        title: item.title,
                        status: serde_json::to_value(item.status)
                            .ok()
                            .and_then(|status| status.as_str().map(str::to_owned))
                            .unwrap_or_default(),
                        detail: item.detail,
                        ready_at: item.ready_at,
                    })
                    .collect(),
            )))
        }
        // `event`, and any kind a later version queued: the detail carries the changes.
        _ => Ok(Some(TurnContext::Events(
            turn.trigger_detail
                .get("events")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        ))),
    }
}

/// Queues an idle turn when the queue has been empty for [`IDLE_AFTER`] and some of her work
/// is worth revisiting.
async fn queue_idle_turn(
    context: &DispatchContext,
    connection: &mut AsyncPgConnection,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let last = elysia::last_finished_turn(connection).await?;
    if last
        .and_then(|turn| turn.finished_at)
        .is_some_and(|finished_at| now - finished_at < IDLE_AFTER)
    {
        return Ok(());
    }

    let work = elysia::unfinished_work(connection).await?;
    let candidates: Vec<Uuid> = revisit_candidates(&work, now)
        .into_iter()
        .map(|item| item.id)
        .collect();
    if candidates.is_empty() {
        return Ok(());
    }

    for id in &candidates {
        elysia::record_revisit(connection, *id).await?;
    }
    let turn = elysia::insert_turn(
        connection,
        NewTurn {
            id: Uuid::now_v7(),
            trigger_kind: trigger::IDLE.to_owned(),
            trigger_detail: json!({ "workIds": candidates }),
            created_by: SYSTEM_USER_ID,
        },
    )
    .await?;
    publish_turn(context, &turn);
    publish_snapshot(&context.events, connection).await?;
    context.elysia.wake();
    Ok(())
}

fn publish_turn(context: &DispatchContext, turn: &Turn) {
    context
        .events
        .publish(&ServerEvent::ElysiaTurnUpserted(TurnResponse::from(
            turn.clone(),
        )));
}

fn detail_id(detail: &Value, key: &str) -> anyhow::Result<Uuid> {
    let id = detail
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("the turn's trigger detail has no {key}"))?;
    Ok(id.parse()?)
}

fn detail_ids(detail: &Value, key: &str) -> Vec<Uuid> {
    detail
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str()?.parse().ok())
        .collect()
}

fn truncate(error: &str) -> String {
    error.chars().take(MAX_ERROR_CHARACTERS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(kind: &str, status: TurnStatus) -> Turn {
        let started = (status != TurnStatus::Queued).then(Utc::now);
        Turn {
            id: Uuid::now_v7(),
            trigger_kind: kind.to_owned(),
            trigger_detail: json!({}),
            status,
            summary: None,
            error: None,
            created_by: SYSTEM_USER_ID,
            created_at: Utc::now(),
            started_at: started,
            finished_at: None,
        }
    }

    fn work(status: WorkStatus, updated_hours_ago: i64, ready_in_minutes: Option<i64>) -> Work {
        let now = Utc::now();
        Work {
            id: Uuid::now_v7(),
            title: "Quarterly deck".to_owned(),
            detail: String::new(),
            status,
            ready_at: ready_in_minutes.map(|minutes| now + TimeDelta::minutes(minutes)),
            action_item_id: None,
            revisits: 0,
            revisited_at: None,
            created_by: ELYSIA_USER_ID,
            created_at: now - TimeDelta::hours(updated_hours_ago),
            updated_at: now - TimeDelta::hours(updated_hours_ago),
        }
    }

    #[test]
    fn nothing_starts_while_a_turn_runs() {
        let active = [
            turn(trigger::MESSAGE, TurnStatus::Running),
            turn(trigger::MESSAGE, TurnStatus::Queued),
        ];
        assert!(next_turn(&active, true).is_none());
    }

    #[test]
    fn a_person_goes_before_her_own_turns() {
        let active = [
            turn(trigger::EVENT, TurnStatus::Queued),
            turn(trigger::ANSWER, TurnStatus::Queued),
        ];
        assert_eq!(
            next_turn(&active, true).map(|turn| turn.trigger_kind.as_str()),
            Some(trigger::ANSWER)
        );
    }

    #[test]
    fn outside_her_hours_only_a_person_starts_a_turn() {
        let events_only = [turn(trigger::EVENT, TurnStatus::Queued)];
        assert!(next_turn(&events_only, false).is_none());

        let with_message = [
            turn(trigger::IDLE, TurnStatus::Queued),
            turn(trigger::MESSAGE, TurnStatus::Queued),
        ];
        assert_eq!(
            next_turn(&with_message, false).map(|turn| turn.trigger_kind.as_str()),
            Some(trigger::MESSAGE)
        );
    }

    #[test]
    fn she_revisits_after_the_cooldown_and_past_her_estimates() {
        let now = Utc::now();
        let fresh = work(WorkStatus::GaveUp, 1, None);
        let stale = work(WorkStatus::GaveUp, 3, None);
        let blocked = work(WorkStatus::Blocked, 3, None);
        let overdue = work(WorkStatus::Working, 0, Some(-5));
        let on_time = work(WorkStatus::Working, 0, Some(30));
        let ready = work(WorkStatus::Ready, 10, None);
        let items = [
            fresh,
            stale.clone(),
            blocked.clone(),
            overdue.clone(),
            on_time,
            ready,
        ];

        let ids: Vec<Uuid> = revisit_candidates(&items, now)
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert_eq!(ids, vec![stale.id, blocked.id, overdue.id]);
    }

    /// A dispatcher against a migrated database whose runner plays back `outcomes`.
    async fn scripted(outcomes: Vec<ElysiaOutcome>) -> (DispatchContext, AsyncPgConnection) {
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};

        use crate::elysia::runner::TurnRunner;

        let (url, connection) = crate::test_support::migrated_database().await;
        let database = crate::connections::connect_postgres(&url, 2)
            .await
            .expect("Postgres accepts connections");
        let runner = TurnRunner::Scripted(Arc::new(Mutex::new(VecDeque::from(outcomes))));
        let events = EventBus::new();
        let context = DispatchContext {
            passwords: crate::server::build_passwords(
                database.clone(),
                Arc::new(crate::test_support::cipher()),
                events.clone(),
            ),
            database,
            events,
            elysia: Elysia::new(runner, crate::tools::grants::Grants::new()),
        };
        (context, connection)
    }

    async fn queue_message(connection: &mut AsyncPgConnection, body: &str) -> Turn {
        use crate::test_support::TEST_PERSON_ID;

        let message = elysia::insert_message(connection, body, TEST_PERSON_ID)
            .await
            .expect("the message is stored");
        elysia::insert_turn(
            connection,
            NewTurn {
                id: Uuid::now_v7(),
                trigger_kind: trigger::MESSAGE.to_owned(),
                trigger_detail: json!({ "messageId": message.id }),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("the turn is queued")
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn a_message_is_answered_and_the_session_is_kept() {
        let (context, mut connection) = scripted(vec![ElysiaOutcome::Completed {
            result: Some("On it: the deck is drafted.\nSummary: Drafted the deck.".to_owned()),
            session_id: Some("session-1".to_owned()),
        }])
        .await;
        let turn = queue_message(&mut connection, "Draft the quarterly deck.").await;

        dispatch(&context, &CancellationToken::new())
            .await
            .expect("the queue runs");

        let finished = elysia::list_turns(&mut connection, 10)
            .await
            .expect("turns load");
        let finished = finished
            .iter()
            .find(|candidate| candidate.id == turn.id)
            .expect("the turn is listed");
        assert_eq!(finished.status, TurnStatus::Succeeded);
        assert_eq!(finished.summary.as_deref(), Some("Drafted the deck."));

        let messages = elysia::list_messages(&mut connection, None, 10)
            .await
            .expect("messages load");
        assert!(messages[0].is_from_elysia());
        assert_eq!(messages[0].body, "On it: the deck is drafted.");

        let state = elysia::state(&mut connection).await.expect("state loads");
        assert_eq!(state.runner_session_id.as_deref(), Some("session-1"));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn her_reply_and_summary_never_keep_a_secret_the_vault_handed_out() {
        let (context, mut connection) = scripted(vec![ElysiaOutcome::Completed {
            result: Some(
                "Signed in with pizza-password-123 and code 492039.\nSummary: Used pizza-password-123."
                    .to_owned(),
            ),
            session_id: None,
        }])
        .await;
        let revealed = context.passwords.revealed();
        revealed.add(&secrecy::SecretString::from("pizza-password-123"));
        revealed.add(&secrecy::SecretString::from("492039"));
        let turn = queue_message(&mut connection, "Order the pizza.").await;

        dispatch(&context, &CancellationToken::new())
            .await
            .expect("the queue runs");

        let messages = elysia::list_messages(&mut connection, None, 10)
            .await
            .expect("messages load");
        assert_eq!(
            messages[0].body,
            "Signed in with [secret] and code [secret]."
        );
        let turns = elysia::list_turns(&mut connection, 10)
            .await
            .expect("turns load");
        let finished = turns
            .iter()
            .find(|candidate| candidate.id == turn.id)
            .expect("the turn is listed");
        assert_eq!(finished.summary.as_deref(), Some("Used [secret]."));
    }

    #[test]
    fn the_password_tools_are_granted_only_while_the_vault_is_ready() {
        let names = |ready| -> Vec<&'static str> {
            tool_servers(ready)
                .iter()
                .map(|server| server.name)
                .collect()
        };
        assert_eq!(names(false), ["elysia"]);
        assert_eq!(names(true), ["elysia", "elysium_passwords"]);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn a_usage_limit_holds_the_queue_and_queues_the_turn_again() {
        let resets_at = Utc::now() + TimeDelta::hours(2);
        let (context, mut connection) = scripted(vec![ElysiaOutcome::UsageLimited {
            resets_at,
            session_id: None,
        }])
        .await;
        let turn = queue_message(&mut connection, "Triage my inbox.").await;

        dispatch(&context, &CancellationToken::new())
            .await
            .expect("the queue runs");

        let turns = elysia::list_turns(&mut connection, 10)
            .await
            .expect("turns load");
        let failed = turns
            .iter()
            .find(|candidate| candidate.id == turn.id)
            .expect("the turn is listed");
        assert_eq!(failed.status, TurnStatus::Failed);
        let again: Vec<&Turn> = turns
            .iter()
            .filter(|candidate| candidate.status == TurnStatus::Queued)
            .collect();
        assert_eq!(again.len(), 1, "the turn is queued again");
        assert_eq!(again[0].trigger_kind, trigger::MESSAGE);

        let state = elysia::state(&mut connection).await.expect("state loads");
        assert!(
            state
                .usage_paused_until
                .is_some_and(|until| until > Utc::now())
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn nothing_starts_while_she_is_paused() {
        let (context, mut connection) = scripted(Vec::new()).await;
        let turn = queue_message(&mut connection, "Are you there?").await;
        elysia::update_state(
            &mut connection,
            elysia::ElysiaStateChanges {
                paused: Some(true),
                availability: None,
            },
            crate::test_support::TEST_PERSON_ID,
        )
        .await
        .expect("she is paused");

        dispatch(&context, &CancellationToken::new())
            .await
            .expect("the queue runs");

        let active = elysia::active_turns(&mut connection)
            .await
            .expect("turns load");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, turn.id);
        assert_eq!(active[0].status, TurnStatus::Queued);
    }

    #[test]
    fn a_recent_revisit_restarts_the_cooldown() {
        let now = Utc::now();
        let mut gave_up = work(WorkStatus::GaveUp, 5, None);
        gave_up.updated_at = now - TimeDelta::minutes(10);
        let mut overdue = work(WorkStatus::Working, 0, Some(-60));
        overdue.revisited_at = Some(now - TimeDelta::minutes(10));
        assert!(revisit_candidates(&[gave_up, overdue], now).is_empty());
    }
}
