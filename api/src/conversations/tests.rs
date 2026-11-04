// Copyright © 2026 Jalapeno Labs

//! Gathering, the quiet window, and starting and handing on request runs, against Postgres
//! where they need it. Regression tests for #81.

use super::*;

fn at(seconds: i64) -> DateTime<Utc> {
    let base: DateTime<Utc> = "2026-11-04T12:00:00Z".parse().expect("a time");
    base + TimeDelta::seconds(seconds)
}

fn waiting(conversation_id: Uuid, seconds: i64, wakes: bool) -> ConversationMessage {
    ConversationMessage {
        timeline_record_id: Uuid::now_v7(),
        conversation_id,
        wakes,
        received_at: at(seconds),
        run_id: None,
        delivered_at: None,
    }
}

#[test]
fn a_conversation_is_due_ten_seconds_after_its_newest_message() {
    let burst = Uuid::now_v7();
    let messages = [waiting(burst, 0, true), waiting(burst, 6, true)];

    let early = quiet(&messages, at(15));
    assert!(
        early.due.is_empty(),
        "the second message restarted the window"
    );
    assert_eq!(early.next, Some(at(16)));

    let late = quiet(&messages, at(16));
    assert_eq!(late.due, vec![burst]);
    assert_eq!(late.next, None);
}

#[test]
fn context_alone_starts_nothing_and_what_an_agent_was_given_still_owes_a_run() {
    let context_only = Uuid::now_v7();
    let given = Uuid::now_v7();
    let mut delivered = waiting(given, 0, false);
    delivered.delivered_at = Some(at(1));
    let messages = [waiting(context_only, 0, false), delivered];

    let found = quiet(&messages, at(60));
    assert_eq!(
        found.due,
        vec![given],
        "a reply the person wrote waits for a message that asks something"
    );
    assert_eq!(found.next, None);
}

#[test]
fn the_next_look_is_when_the_soonest_conversation_goes_quiet() {
    let (first, second) = (Uuid::now_v7(), Uuid::now_v7());
    let messages = [waiting(first, 3, true), waiting(second, 1, true)];
    let found = quiet(&messages, at(5));
    assert!(found.due.is_empty());
    assert_eq!(found.next, Some(at(11)));
}

/// What the tests below drive: a Slack connector of the test person's, and the bus ingest
/// publishes on.
struct Fixture {
    url: secrecy::SecretString,
    connection: AsyncPgConnection,
    events: EventBus,
    slack: Connector,
}

impl Fixture {
    async fn start() -> Self {
        let (url, mut connection) = crate::test_support::migrated_database().await;
        let slack = crate::models::connector::create(
            &mut connection,
            crate::models::connector::NewConnector {
                id: Uuid::now_v7(),
                kind: "slack".to_owned(),
                browser_session_id: None,
                token_account_id: None,
                jira_credential_id: None,
                github_credential_id: None,
                storage_location_id: None,
                content: serde_json::json!({ "conversations": "*" }),
                created_by: crate::test_support::TEST_PERSON_ID,
            },
        )
        .await
        .expect("the connector is stored");
        Self {
            url,
            connection,
            events: EventBus::in_process(),
            slack,
        }
    }

    /// A pool on the fixture's database, for a step, which takes one.
    async fn pool(&self) -> Pool {
        crate::connections::connect_postgres(&self.url, 2)
            .await
            .expect("Postgres accepts connections")
    }

    /// Ingests a Slack message said in conversation `key` as a baselined read would, and answers
    /// its record and the conversation it gathered in.
    async fn ingest(&mut self, key: &str, ts: &str, wants_attention: bool) -> (Uuid, Option<Uuid>) {
        let read = connectors::Read {
            external_id: format!("{key}/{ts}"),
            occurred_at: Some(Utc::now()),
            actor: "Ada".to_owned(),
            title: "Ada in a direct message in Mooreslab".to_owned(),
            body: format!("Message {ts}"),
            url: None,
            wants_attention,
            metadata: serde_json::json!({}),
            conversation: Some(connectors::Conversation {
                key: key.to_owned(),
                title: "Ada in a direct message in Mooreslab".to_owned(),
            }),
        };
        let stored = connectors::ingest::record(
            &mut self.connection,
            &self.events,
            &self.slack,
            connectors::ConnectorKind::Slack,
            &read,
            None,
            true,
        )
        .await
        .expect("the read is recorded");
        assert!(stored, "a new message");
        let record = crate::models::timeline_record::find_by_external_id(
            &mut self.connection,
            self.slack.id,
            &read.external_id,
        )
        .await
        .expect("reads")
        .expect("the record");
        let gathered = conversation::of_records(&mut self.connection, &[record.id])
            .await
            .expect("reads")
            .map(|found| found.id);
        (record.id, gathered)
    }

    /// Lets run `run_id` succeed, as its steps would.
    async fn succeed(&mut self, run_id: Uuid) {
        use diesel::prelude::*;
        use diesel_async::RunQueryDsl;

        use crate::database::schema::workflow_runs;
        diesel::update(workflow_runs::table.find(run_id))
            .set((
                workflow_runs::state.eq(WorkflowRunState::Succeeded),
                workflow_runs::started_at.eq(Utc::now()),
                workflow_runs::finished_at.eq(Utc::now()),
            ))
            .execute(&mut self.connection)
            .await
            .expect("the run succeeds");
    }
}

fn started(claimed: Claimed) -> Box<Settled> {
    match claimed {
        Claimed::Started(settled) => settled,
        other => panic!("expected a run to start, got {other:?}"),
    }
}

fn record_ids_of(settled: &Settled) -> Vec<Uuid> {
    serde_json::from_value(settled.run.input["recordIds"].clone()).expect("record ids")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn bug_81_messages_gather_per_conversation_and_one_quiet_run_takes_them() {
    let mut fixture = Fixture::start().await;
    let (first, direct) = fixture.ingest("T1/D1", "1.1", true).await;
    let (second, direct_again) = fixture.ingest("T1/D1", "1.2", true).await;
    let (_elsewhere, thread) = fixture.ingest("T1/C1/9.0", "9.1", true).await;
    let (_aside, nowhere) = fixture.ingest("T1/C2/8.0", "8.1", false).await;
    let direct = direct.expect("a direct message gathers");
    assert_eq!(direct_again, Some(direct), "one conversation, two messages");
    assert!(thread.is_some_and(|thread| thread != direct));
    assert_eq!(
        nowhere, None,
        "a message asking nothing starts no conversation"
    );

    let now = Utc::now();
    assert!(
        matches!(
            claim(&mut fixture.connection, direct, now).await,
            Ok(Claimed::Nothing)
        ),
        "nothing starts inside the quiet window"
    );
    let quiet_at = now + QUIET_WINDOW + TimeDelta::seconds(1);
    let run = started(
        claim(&mut fixture.connection, direct, quiet_at)
            .await
            .expect("claims"),
    );
    assert_eq!(run.run.workflow, "request");
    assert_eq!(run.run.source_id, direct.to_string());
    assert_eq!(record_ids_of(&run), vec![first, second]);
    assert!(
        matches!(
            claim(&mut fixture.connection, direct, quiet_at).await,
            Ok(Claimed::Nothing)
        ),
        "a taken message is taken once"
    );

    // While the run is live, a new message waits for no new run, and the person's own reply
    // joins as context.
    let (third, joined) = fixture.ingest("T1/D1", "1.3", true).await;
    assert_eq!(joined, Some(direct));
    let (own, own_joined) = fixture.ingest("T1/D1", "1.4", false).await;
    assert_eq!(
        own_joined,
        Some(direct),
        "a reply joins a conversation being worked on"
    );
    let later = Utc::now() + QUIET_WINDOW + TimeDelta::seconds(1);
    assert!(matches!(
        claim(&mut fixture.connection, direct, later).await,
        Ok(Claimed::Busy(WorkflowRunState::Queued))
    ));

    // Once it ends, the next run takes what arrived, reading what came before as context.
    fixture.succeed(run.run.id).await;
    let next = started(
        claim(&mut fixture.connection, direct, later)
            .await
            .expect("claims"),
    );
    assert_eq!(record_ids_of(&next), vec![first, second, third, own]);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn bug_81_an_attempt_taking_over_gives_the_agent_only_what_it_was_not_given() {
    let mut fixture = Fixture::start().await;
    let (first, gathered) = fixture.ingest("T1/D1", "1.1", true).await;
    let (second, _) = fixture.ingest("T1/D1", "1.2", true).await;
    let conversation_id = gathered.expect("gathers");
    let quiet_at = Utc::now() + QUIET_WINDOW + TimeDelta::seconds(1);
    let run = started(
        claim(&mut fixture.connection, conversation_id, quiet_at)
            .await
            .expect("claims"),
    );

    // An earlier attempt's turn landed the first message and asked to wait.
    let until = Utc::now() + TimeDelta::seconds(30);
    conversation::record_turn(
        &mut fixture.connection,
        conversation_id,
        &conversation::TurnRecorded {
            harness_session_id: Some("session-a".to_owned()),
            delivered: &[first],
            wait: Some((until, run.run.id)),
        },
        Utc::now(),
    )
    .await
    .expect("records the turn");
    let held = conversation::find(&mut fixture.connection, conversation_id)
        .await
        .expect("reads");
    assert_eq!(held.harness_session_id.as_deref(), Some("session-a"));
    assert_eq!(held.waited_in_run_id, Some(run.run.id));
    assert!(held.waiting_until.is_some());
    let owed: Vec<Uuid> =
        conversation::undelivered_of(&mut fixture.connection, conversation_id, 50)
            .await
            .expect("reads")
            .into_iter()
            .map(|message| message.timeline_record_id)
            .collect();
    assert_eq!(
        owed,
        vec![second],
        "the next attempt gives only what was not given"
    );

    // The turn after the wait lands the rest, keeps the session, and ends the wait, but the run
    // keeps the wait it spent.
    conversation::record_turn(
        &mut fixture.connection,
        conversation_id,
        &conversation::TurnRecorded {
            harness_session_id: None,
            delivered: &[second],
            wait: None,
        },
        Utc::now(),
    )
    .await
    .expect("records the turn");
    let after = conversation::find(&mut fixture.connection, conversation_id)
        .await
        .expect("reads");
    assert_eq!(after.harness_session_id.as_deref(), Some("session-a"));
    assert_eq!(after.waiting_until, None);
    assert_eq!(after.waited_in_run_id, Some(run.run.id));

    // With every message given, the step has nothing to give and runs no turn.
    let step = handle::HandleRequest::for_test(fixture.pool().await);
    let input = crate::meetings::segment::Segmented {
        segments: Vec::new(),
        record_ids: vec![first, second],
    };
    let done = crate::workflows::Step::run(
        &step,
        input,
        crate::workflows::StepContext::for_test::<handle::HandleRequest>(fixture.pool().await),
    )
    .await
    .expect("runs");
    assert!(matches!(done, crate::workflows::Done::Skipped { .. }));
}
