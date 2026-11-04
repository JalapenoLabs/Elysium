// Copyright © 2026 Jalapeno Labs

//! Drafts against Postgres: where a reply goes, a send that fails keeping its text and saying
//! why, one send at a time, and what a sent draft leaves on the timeline. Nothing here reaches a
//! real Slack, Teams, or mail server; a connector without its browser fails the way a browser
//! that cannot post does.

use diesel_async::AsyncPgConnection;
use uuid::uuid;

use super::*;
use crate::models::connector::{Connector, NewConnector};
use crate::models::user;
use crate::test_support::{TEST_PERSON_ID, app_state, migrated_database};

const OTHER_PERSON_ID: Uuid = uuid!("0199a3c4-0000-7000-8000-00000000f00d");

#[test]
fn a_body_must_say_something_and_fit() {
    check_body("Friday works.").expect("fits");
    check_body("  \n ").expect_err("empty");
    check_body(&"a".repeat(BODY_MAX_CHARACTERS)).expect("fits exactly");
    check_body(&"a".repeat(BODY_MAX_CHARACTERS + 1)).expect_err("too long");
}

#[test]
fn a_sent_record_names_the_draft_it_came_from() {
    let id = Uuid::now_v7();
    let metadata = with_draft(json!({ "team": "T1" }), id);
    assert_eq!(metadata, json!({ "team": "T1", "draftId": id }));
    assert_eq!(with_draft(Value::Null, id), json!({ "draftId": id }));
}

struct Fixture {
    state: AppState,
    connection: AsyncPgConnection,
    person: User,
}

impl Fixture {
    async fn start() -> Self {
        let (url, mut connection) = migrated_database().await;
        let state = app_state(&url).await;
        let person = user::find(&mut connection, TEST_PERSON_ID)
            .await
            .expect("the test person");
        Self {
            state,
            connection,
            person,
        }
    }

    /// A connector of `kind` of the test person's, waiting for its browser, as one is between
    /// being made and its person starting it.
    async fn waiting_connector(&mut self, kind: &str) -> Connector {
        connector::create(
            &mut self.connection,
            NewConnector {
                id: Uuid::now_v7(),
                kind: kind.to_owned(),
                browser_session_id: None,
                token_account_id: None,
                jira_credential_id: None,
                github_credential_id: None,
                storage_location_id: None,
                content: json!({ "conversations": "*" }),
                created_by: TEST_PERSON_ID,
            },
        )
        .await
        .expect("the connector is stored")
    }

    async fn record(
        &mut self,
        through: &Connector,
        external_id: &str,
        actor: &str,
        metadata: Value,
    ) -> TimelineRecord {
        timeline_record::insert_new(
            &mut self.connection,
            NewTimelineRecord {
                id: Uuid::now_v7(),
                source_kind: through.kind.clone(),
                connector_id: Some(through.id),
                external_id: Some(external_id.to_owned()),
                occurred_at: Utc::now(),
                actor: actor.to_owned(),
                title: "#launch in Mooreslab".to_owned(),
                body: "Are we launching Friday?".to_owned(),
                url: None,
                media: None,
                metadata,
                created_by: through.created_by,
            },
        )
        .await
        .expect("stored")
        .expect("new")
    }

    async fn slack_message(&mut self) -> (Connector, TimelineRecord) {
        let slack = self.waiting_connector("slack").await;
        let record = self
            .record(
                &slack,
                "T1/C1/1730000000.000200",
                "Ada",
                json!({
                    "team": "T1",
                    "conversation": "C1",
                    "ts": "1730000000.000200",
                    "threadTs": "1730000000.000100",
                }),
            )
            .await;
        (slack, record)
    }

    fn reply(record: &TimelineRecord, place: Option<ReplyPlace>) -> Reply {
        Reply {
            record_id: record.id,
            place,
            mail_account_id: None,
            body: "Friday works.".to_owned(),
        }
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_reply_to_a_slack_message_is_drafted_into_its_thread_and_sends_nothing() {
    let mut fixture = Fixture::start().await;
    let (slack, record) = fixture.slack_message().await;

    let written = draft_reply(
        &fixture.state,
        &fixture.person,
        Fixture::reply(&record, Some(ReplyPlace::Thread)),
    )
    .await
    .expect("drafted");
    assert_eq!(written.state, DraftState::Draft);
    assert_eq!(written.sender(), Sender::Connector(slack.id));
    assert_eq!(written.place, Some(ReplyPlace::Thread));
    assert_eq!(written.reply_to_record_id, Some(record.id));
    assert_eq!(
        written.target,
        json!({ "team": "T1", "channel": "C1", "threadTs": "1730000000.000100" })
    );

    let in_conversation = draft_reply(
        &fixture.state,
        &fixture.person,
        Fixture::reply(&record, Some(ReplyPlace::Conversation)),
    )
    .await
    .expect("drafted");
    assert_eq!(
        in_conversation.target,
        json!({ "team": "T1", "channel": "C1" })
    );

    let unsent = draft::list_unsent_for_record(&mut fixture.connection, record.id, TEST_PERSON_ID)
        .await
        .expect("listed");
    assert_eq!(unsent.len(), 2);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_send_that_fails_keeps_the_draft_says_why_and_can_be_sent_again() {
    let mut fixture = Fixture::start().await;
    let (_slack, record) = fixture.slack_message().await;
    let written = draft_reply(
        &fixture.state,
        &fixture.person,
        Fixture::reply(&record, None),
    )
    .await
    .expect("drafted");

    let failed = send(&fixture.state, &fixture.person, written.id)
        .await
        .expect("a failed send is an answer");
    assert_eq!(failed.state, DraftState::Failed);
    assert_eq!(failed.body, "Friday works.");
    let reason = failed.last_error.clone().expect("a reason");
    assert!(reason.contains("no browser yet"), "{reason}");
    assert_eq!(failed.sent_record_id, None);

    let edited = update(
        &fixture.state,
        &fixture.person,
        failed.id,
        "Friday at noon.",
    )
    .await
    .expect("a failed draft can be edited");
    assert_eq!(edited.body, "Friday at noon.");
    assert_eq!(edited.last_error.as_deref(), Some(reason.as_str()));

    let again = send(&fixture.state, &fixture.person, failed.id)
        .await
        .expect("sent again");
    assert_eq!(again.state, DraftState::Failed);
    assert_eq!(again.body, "Friday at noon.");
    assert!(again.attempted_at > failed.attempted_at);
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_draft_is_sent_once_at_a_time() {
    let mut fixture = Fixture::start().await;
    let (_slack, record) = fixture.slack_message().await;
    let written = draft_reply(
        &fixture.state,
        &fixture.person,
        Fixture::reply(&record, None),
    )
    .await
    .expect("drafted");

    let now = Utc::now();
    let stale_after = TimeDelta::minutes(2);
    draft::claim(
        &mut fixture.connection,
        written.id,
        TEST_PERSON_ID,
        now,
        stale_after,
    )
    .await
    .expect("claimed")
    .expect("it may be sent");
    let second = draft::claim(
        &mut fixture.connection,
        written.id,
        TEST_PERSON_ID,
        now,
        stale_after,
    )
    .await
    .expect("asked");
    assert!(second.is_none(), "a draft being sent is not claimed twice");

    let refused = send(&fixture.state, &fixture.person, written.id)
        .await
        .expect_err("refused");
    assert!(matches!(
        refused,
        ApiError::Conflict("this draft is being sent")
    ));
    let refused = update(&fixture.state, &fixture.person, written.id, "Changed")
        .await
        .expect_err("refused");
    assert!(matches!(refused, ApiError::Conflict(_)));
    let refused = discard(&fixture.state, &fixture.person, written.id)
        .await
        .expect_err("refused");
    assert!(matches!(refused, ApiError::Conflict(_)));

    // A process that stopped mid-send leaves it sending; past the wait it can be sent again.
    let later = now + stale_after + TimeDelta::seconds(1);
    let reclaimed = draft::claim(
        &mut fixture.connection,
        written.id,
        TEST_PERSON_ID,
        later,
        stale_after,
    )
    .await
    .expect("asked");
    assert!(reclaimed.is_some());
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn a_sent_draft_becomes_the_record_its_reader_would_write_once() {
    let mut fixture = Fixture::start().await;
    let (slack, record) = fixture.slack_message().await;
    let first = draft_reply(
        &fixture.state,
        &fixture.person,
        Fixture::reply(&record, None),
    )
    .await
    .expect("drafted");
    let second = draft_reply(
        &fixture.state,
        &fixture.person,
        Fixture::reply(&record, None),
    )
    .await
    .expect("drafted");

    let sent_record = |external_id: &str| NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: "slack".to_owned(),
        connector_id: Some(slack.id),
        external_id: Some(external_id.to_owned()),
        occurred_at: Utc::now(),
        actor: "Tester".to_owned(),
        title: record.title.clone(),
        body: "Friday works.".to_owned(),
        url: None,
        media: None,
        metadata: json!({}),
        created_by: TEST_PERSON_ID,
    };

    let (sent, created) = record_sent(
        &mut fixture.connection,
        &first,
        sent_record("T1/C1/1730000100.000300"),
    )
    .await
    .expect("recorded");
    let created = created.expect("a new record");
    assert_eq!(sent.state, DraftState::Sent);
    assert_eq!(sent.sent_record_id, Some(created.id));
    assert!(sent.sent_at.is_some());

    // The reader got to the message first: the draft points at the record it already wrote.
    let (sent, created_again) = record_sent(
        &mut fixture.connection,
        &second,
        sent_record("T1/C1/1730000100.000300"),
    )
    .await
    .expect("recorded");
    assert!(created_again.is_none());
    assert_eq!(sent.sent_record_id, Some(created.id));

    let unsent = draft::list_unsent_for_record(&mut fixture.connection, record.id, TEST_PERSON_ID)
        .await
        .expect("listed");
    assert!(unsent.is_empty());
    let refused = send(&fixture.state, &fixture.person, first.id)
        .await
        .expect_err("refused");
    assert!(matches!(
        refused,
        ApiError::Conflict("this draft was already sent")
    ));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn mail_is_answered_from_the_persons_own_mailbox() {
    let mut fixture = Fixture::start().await;
    let gmail = fixture.waiting_connector("gmail").await;
    let record = fixture
        .record(
            &gmail,
            "1812345678901234567",
            "Ada <ada@example.org>",
            json!({}),
        )
        .await;

    let refused = draft_reply(
        &fixture.state,
        &fixture.person,
        Fixture::reply(&record, None),
    )
    .await
    .expect_err("no mailbox");
    assert!(matches!(
        refused,
        ApiError::CodedBadRequest {
            code: "no_mailbox",
            ..
        }
    ));
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL and TEST_REDIS_URL; run api/scripts/verify-migrations.sh"]
async fn someone_elses_record_and_draft_are_not_found() {
    let mut fixture = Fixture::start().await;
    diesel_async::RunQueryDsl::execute(
        diesel::sql_query(
            "INSERT INTO users (id, kratos_identity_id, email, name, role, status, approved_at) \
             VALUES ($1, $2, 'grace@navy.mil', 'Grace Hopper', 'member', 'active', now())",
        )
        .bind::<diesel::sql_types::Uuid, _>(OTHER_PERSON_ID)
        .bind::<diesel::sql_types::Uuid, _>(Uuid::now_v7()),
        &mut fixture.connection,
    )
    .await
    .expect("the other person is created");
    let other = user::find(&mut fixture.connection, OTHER_PERSON_ID)
        .await
        .expect("the other person");
    let (_slack, record) = fixture.slack_message().await;

    let refused = draft_reply(&fixture.state, &other, Fixture::reply(&record, None))
        .await
        .expect_err("not theirs");
    assert!(matches!(refused, ApiError::NotFound));

    let written = draft_reply(
        &fixture.state,
        &fixture.person,
        Fixture::reply(&record, None),
    )
    .await
    .expect("drafted");
    let refused = send(&fixture.state, &other, written.id)
        .await
        .expect_err("not theirs");
    assert!(matches!(refused, ApiError::NotFound));
}
