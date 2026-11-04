// Copyright © 2026 Jalapeno Labs

//! Slack's reading, against answers shaped as Slack's web API gives them.

use serde_json::json;

use super::*;

const ME: &str = "U0ME";

fn team(id: &str, name: &str) -> Team {
    Team {
        id: id.to_owned(),
        name: name.to_owned(),
        url: format!("https://{}.slack.com/", name.to_lowercase()),
        user_id: ME.to_owned(),
        counts: None,
    }
}

fn conversation(id: &str, kind: ConversationKind, latest: &str) -> Conversation {
    Conversation {
        id: id.to_owned(),
        kind,
        latest: Some(latest.to_owned()),
    }
}

fn counted(conversations: Vec<Conversation>, threads_unread: bool) -> Vec<(Team, Counts)> {
    vec![(
        team("T1", "Mooreslab"),
        Counts {
            conversations,
            threads_unread,
        },
    )]
}

fn state_with(conversations: &[(&str, &str)]) -> State {
    State {
        conversations: conversations
            .iter()
            .map(|(key, ts)| ((*key).to_owned(), (*ts).to_owned()))
            .collect(),
        names: BTreeMap::new(),
    }
}

fn message(
    conversation: &str,
    kind: ConversationKind,
    ts: &str,
    user: &str,
    text: &str,
) -> Message {
    Message {
        team: "T1".to_owned(),
        conversation: conversation.to_owned(),
        kind,
        ts: ts.to_owned(),
        user: Some(user.to_owned()),
        bot: None,
        text: text.to_owned(),
        thread_ts: None,
    }
}

#[test]
fn counts_list_every_conversation_with_its_kind_and_newest_message() {
    let counts = parse_counts(&json!({
        "ok": true,
        "channels": [{ "id": "C1", "latest": "1730000002.000200", "has_unreads": true, "mention_count": 0 }],
        "mpims": [{ "id": "G1", "latest": "1730000001.000100" }],
        "ims": [{ "id": "D1" }],
        "threads": { "has_unreads": true, "mention_count": 1 },
    }))
    .expect("counts");

    assert_eq!(
        counts.conversations,
        vec![
            conversation("C1", ConversationKind::Channel, "1730000002.000200"),
            conversation("G1", ConversationKind::Group, "1730000001.000100"),
            Conversation {
                id: "D1".to_owned(),
                kind: ConversationKind::Direct,
                latest: None,
            },
        ]
    );
    assert!(counts.threads_unread);
    assert_eq!(
        parse_counts(&json!({ "ok": false, "error": "invalid_auth" })),
        Err("invalid_auth".to_owned())
    );
}

#[test]
fn a_workspace_that_signed_out_is_left_out_and_any_other_failure_of_all_of_them_is_an_error() {
    let mut signed_out = team("T1", "Mooreslab");
    signed_out.counts = Some(json!({ "ok": false, "error": "invalid_auth" }));
    let mut working = team("T2", "Jalapeno");
    working.counts = Some(json!({ "ok": true, "channels": [] }));
    let mut broken = team("T3", "Broken");
    broken.counts = Some(json!({ "ok": false, "error": "ratelimited" }));

    let counted = count_teams(&[signed_out.clone(), working]).expect("one workspace answered");
    assert_eq!(counted.len(), 1);
    assert_eq!(counted[0].0.id, "T2");

    assert!(
        count_teams(&[signed_out.clone()])
            .expect("signed out")
            .is_empty()
    );
    assert!(matches!(
        count_teams(&[signed_out, broken]),
        Err(ConnectorError::Unexpected(message)) if message.contains("ratelimited")
    ));
}

#[test]
fn a_conversation_never_read_is_baselined_at_its_newest_message_not_fetched() {
    let counted = counted(
        vec![conversation(
            "C1",
            ConversationKind::Channel,
            "1730000005.000000",
        )],
        false,
    );
    let plan = plan(&counted, &Content::EVERYTHING, &State::default());

    assert!(plan.calls.is_empty());
    assert_eq!(
        plan.baselined,
        vec![("T1/C1".to_owned(), "1730000005.000000".to_owned())]
    );
}

#[test]
fn a_conversation_with_something_newer_is_fetched_from_where_it_was_left() {
    let counted = counted(
        vec![
            conversation("C1", ConversationKind::Channel, "1730000005.000000"),
            conversation("C2", ConversationKind::Channel, "1730000003.000000"),
        ],
        false,
    );
    let state = state_with(&[
        ("T1/C1", "1730000001.000000"),
        ("T1/C2", "1730000003.000000"),
    ]);
    let plan = plan(&counted, &Content::EVERYTHING, &state);

    assert_eq!(plan.calls.len(), 1, "C2 is read up to its newest message");
    assert_eq!(plan.calls[0].method, "conversations.history");
    assert_eq!(plan.calls[0].params["channel"], "C1");
    assert_eq!(plan.calls[0].params["oldest"], "1730000001.000000");
    assert_eq!(plan.calls[0].params["inclusive"], false);
}

#[test]
fn one_read_fetches_the_most_recently_active_conversations_first_and_at_most_a_cap() {
    let conversations: Vec<Conversation> = (0..30)
        .map(|index| {
            conversation(
                &format!("C{index:02}"),
                ConversationKind::Channel,
                &format!("17300001{index:02}.000000"),
            )
        })
        .collect();
    let read: Vec<(String, &str)> = (0..30)
        .map(|index| (format!("T1/C{index:02}"), "1730000000.000000"))
        .collect();
    let state = State {
        conversations: read
            .into_iter()
            .map(|(key, ts)| (key, ts.to_owned()))
            .collect(),
        names: BTreeMap::new(),
    };
    let plan = plan(&counted(conversations, false), &Content::EVERYTHING, &state);

    assert_eq!(plan.calls.len(), CONVERSATIONS_PER_READ);
    assert_eq!(plan.calls[0].params["channel"], "C29");
    assert_eq!(
        plan.calls[CONVERSATIONS_PER_READ - 1].params["channel"],
        "C05"
    );
}

#[test]
fn content_bounds_what_is_read_and_a_workspaces_dms_cover_group_messages_too() {
    let counted = counted(
        vec![
            conversation("C1", ConversationKind::Channel, "1730000005.000000"),
            conversation("C2", ConversationKind::Channel, "1730000005.000000"),
            conversation("G1", ConversationKind::Group, "1730000005.000000"),
            conversation("D1", ConversationKind::Direct, "1730000005.000000"),
        ],
        false,
    );
    let only = |picked: &[&str]| Content {
        conversations: Selection::Only(picked.iter().map(|entry| (*entry).to_owned()).collect()),
    };
    let baselined = |content: &Content| -> Vec<String> {
        plan(&counted, content, &State::default())
            .baselined
            .into_iter()
            .map(|(key, _ts)| key)
            .collect()
    };

    assert_eq!(baselined(&only(&["T1/C2"])), vec!["T1/C2"]);
    assert_eq!(baselined(&only(&["T1/dms"])), vec!["T1/G1", "T1/D1"]);
    assert_eq!(baselined(&only(&["T1"])).len(), 4);
    assert!(baselined(&only(&["T9"])).is_empty());
}

#[test]
fn threads_are_viewed_only_where_they_have_unread_replies_and_the_workspace_is_read() {
    let unread = counted(vec![], true);
    let read = plan(&unread, &Content::EVERYTHING, &State::default());
    assert_eq!(read.calls.len(), 1);
    assert_eq!(read.calls[0].method, "subscriptions.thread.getView");

    let only_elsewhere = Content {
        conversations: Selection::Only(vec!["T9".to_owned()]),
    };
    assert!(
        plan(&unread, &only_elsewhere, &State::default())
            .calls
            .is_empty()
    );
    assert!(
        plan(
            &counted(vec![], false),
            &Content::EVERYTHING,
            &State::default()
        )
        .calls
        .is_empty()
    );
}

#[test]
fn a_fetch_yields_its_words_moves_its_place_and_a_failed_one_keeps_it() {
    let fetches = vec![
        Fetch::History {
            team: "T1".to_owned(),
            conversation: "C1".to_owned(),
            kind: ConversationKind::Channel,
            latest: "1730000009.000000".to_owned(),
        },
        Fetch::History {
            team: "T1".to_owned(),
            conversation: "C2".to_owned(),
            kind: ConversationKind::Channel,
            latest: "1730000009.000000".to_owned(),
        },
    ];
    let results = vec![
        json!({ "ok": true, "messages": [
            { "ts": "1730000009.000000", "user": "U1", "text": "Launch is Friday" },
            { "ts": "1730000008.000000", "user": "U2", "subtype": "channel_join", "text": "joined" },
            { "ts": "1730000007.000000", "subtype": "bot_message", "username": "Deploy bot", "text": "Shipped" },
        ]}),
        json!({ "ok": false, "error": "ratelimited" }),
    ];
    let mut state = state_with(&[
        ("T1/C1", "1730000001.000000"),
        ("T1/C2", "1730000001.000000"),
    ]);
    let messages = collect(&fetches, &results, &mut state);

    assert_eq!(messages.len(), 2, "a join is not someone's words");
    assert_eq!(messages[1].bot.as_deref(), Some("Deploy bot"));
    assert_eq!(state.conversations["T1/C1"], "1730000009.000000");
    assert_eq!(state.conversations["T1/C2"], "1730000001.000000");
}

#[test]
fn the_threads_view_yields_its_unread_replies_in_their_conversations() {
    let fetches = vec![Fetch::Threads {
        team: "T1".to_owned(),
    }];
    let results = vec![json!({ "ok": true, "threads": [{
        "root_msg": { "channel": "C1", "ts": "1730000001.000000", "user": ME, "text": "Plan?" },
        "unread_replies": [
            { "ts": "1730000004.000000", "thread_ts": "1730000001.000000", "user": "U1", "text": "Yes" },
        ],
        "latest_replies": [
            { "ts": "1730000002.000000", "thread_ts": "1730000001.000000", "user": "U2", "text": "Read already" },
        ],
    }]})];
    let messages = collect(&fetches, &results, &mut State::default());

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].conversation, "C1");
    assert_eq!(messages[0].thread_ts.as_deref(), Some("1730000001.000000"));
}

#[test]
fn unknown_senders_and_channels_are_looked_up_once_and_remembered() {
    let messages = vec![
        message("C1", ConversationKind::Channel, "1.0", "U1", "hi"),
        message("C1", ConversationKind::Channel, "2.0", "U1", "again"),
        message("D1", ConversationKind::Direct, "3.0", "U2", "dm"),
    ];
    let mut state = State::default();
    let calls = lookups(&messages, &state);
    let asked: Vec<(&str, &Value)> = calls
        .iter()
        .map(|call| (call.method, &call.params))
        .collect();
    assert_eq!(
        asked,
        vec![
            ("conversations.info", &json!({ "channel": "C1" })),
            ("users.info", &json!({ "user": "U1" })),
            ("users.info", &json!({ "user": "U2" })),
        ]
    );

    remember_names(
        &calls,
        &[
            json!({ "ok": true, "channel": { "name": "launch" } }),
            json!({ "ok": true, "user": { "name": "ada", "profile": { "display_name": "", "real_name": "Ada Lovelace" } } }),
            json!({ "ok": false, "error": "user_not_found" }),
        ],
        &mut state,
    );
    assert_eq!(state.names["T1/C1"], "launch");
    assert_eq!(state.names["T1/U1"], "Ada Lovelace");
    assert!(!state.names.contains_key("T1/U2"));
    assert_eq!(
        lookups(&messages, &state).len(),
        1,
        "only U2 is still unknown"
    );
}

#[test]
fn direct_messages_mentions_and_thread_replies_want_attention_unless_the_person_wrote_them() {
    let mut reply = message(
        "C1",
        ConversationKind::Channel,
        "1730000004.000000",
        "U1",
        "Done",
    );
    reply.thread_ts = Some("1730000001.000000".to_owned());
    let messages = vec![
        message(
            "C1",
            ConversationKind::Channel,
            "1730000001.000000",
            "U1",
            "Morning all",
        ),
        message(
            "C1",
            ConversationKind::Channel,
            "1730000002.000000",
            "U1",
            "<@U0ME> can you look?",
        ),
        message(
            "D1",
            ConversationKind::Direct,
            "1730000003.000000",
            "U1",
            "Quick question",
        ),
        reply,
        message(
            "D1",
            ConversationKind::Direct,
            "1730000005.000000",
            ME,
            "Sure",
        ),
    ];
    let reads = reads(&messages, &counted(vec![], false), &State::default());
    let attention: Vec<(&str, bool)> = reads
        .iter()
        .map(|read| {
            (
                read.external_id.rsplit('/').next().unwrap_or(""),
                read.wants_attention,
            )
        })
        .collect();

    assert_eq!(
        attention,
        vec![
            ("1730000005.000000", false),
            ("1730000004.000000", true),
            ("1730000003.000000", true),
            ("1730000002.000000", true),
            ("1730000001.000000", false),
        ],
        "newest first; the person's own message and plain channel chatter raise nothing"
    );
}

#[test]
fn a_direct_message_is_one_conversation_and_a_channel_one_per_thread() {
    let mut state = State::default();
    state.names.insert("T1/U1".to_owned(), "Ada".to_owned());
    state.names.insert("T1/C1".to_owned(), "launch".to_owned());
    let mut reply = message(
        "C1",
        ConversationKind::Channel,
        "1730000004.000000",
        "U1",
        "Done",
    );
    reply.thread_ts = Some("1730000001.000000".to_owned());
    let mut direct_reply = message(
        "D1",
        ConversationKind::Direct,
        "1730000006.000000",
        "U1",
        "In a thread",
    );
    direct_reply.thread_ts = Some("1730000003.000000".to_owned());
    let messages = vec![
        message(
            "C1",
            ConversationKind::Channel,
            "1730000001.000000",
            "U1",
            "<@U0ME> can you look?",
        ),
        reply,
        message(
            "C1",
            ConversationKind::Channel,
            "1730000002.000000",
            "U1",
            "<@U0ME> another thing",
        ),
        message(
            "D1",
            ConversationKind::Direct,
            "1730000003.000000",
            "U1",
            "Quick question",
        ),
        direct_reply,
    ];
    let reads = reads(&messages, &counted(vec![], false), &state);
    let said_in = |ts: &str| {
        let read = reads
            .iter()
            .find(|read| read.external_id.ends_with(ts))
            .expect("read");
        let conversation = read.conversation.clone().expect("a message names one");
        (conversation.key, conversation.title)
    };

    let thread = (
        "T1/C1/1730000001.000000".to_owned(),
        "#launch in Mooreslab".to_owned(),
    );
    assert_eq!(
        said_in("1730000001.000000"),
        thread,
        "a top-level message starts its thread"
    );
    assert_eq!(said_in("1730000004.000000"), thread, "its reply joins it");
    assert_eq!(
        said_in("1730000002.000000").0,
        "T1/C1/1730000002.000000",
        "another top-level message is another thread"
    );
    let direct = (
        "T1/D1".to_owned(),
        "Ada in a direct message in Mooreslab".to_owned(),
    );
    assert_eq!(said_in("1730000003.000000"), direct);
    assert_eq!(
        said_in("1730000006.000000"),
        direct,
        "a direct message is one, threads and all"
    );
}

#[test]
fn a_record_is_named_linked_and_written_in_plain_text() {
    let mut state = State::default();
    state.names.insert("T1/U1".to_owned(), "Ada".to_owned());
    state.names.insert("T1/C1".to_owned(), "launch".to_owned());
    state.names.insert("T1/U0ME".to_owned(), "Alex".to_owned());

    let mut reply = message(
        "C1",
        ConversationKind::Channel,
        "1730000004.000500",
        "U1",
        "<@U0ME> see <https://example.com/spec|the spec> &amp; <#C2|design>",
    );
    reply.thread_ts = Some("1730000001.000000".to_owned());
    let read = &reads(&[reply], &counted(vec![], false), &state)[0];

    assert_eq!(read.external_id, "T1/C1/1730000004.000500");
    assert_eq!(read.actor, "Ada");
    assert_eq!(read.title, "Reply in #launch in Mooreslab");
    assert_eq!(read.body, "@Alex see the spec & #design");
    assert_eq!(
        read.url.as_deref(),
        Some(
            "https://mooreslab.slack.com/archives/C1/p1730000004000500?thread_ts=1730000001.000000&cid=C1"
        )
    );
    assert_eq!(
        read.occurred_at.map(|at| at.to_rfc3339()),
        Some("2024-10-27T03:33:24.000500+00:00".to_owned())
    );
    // Where it was said, which a reply to it needs.
    assert_eq!(
        read.metadata,
        json!({
            "team": "T1",
            "conversation": "C1",
            "ts": "1730000004.000500",
            "threadTs": "1730000001.000000",
        })
    );

    let direct = message(
        "D1",
        ConversationKind::Direct,
        "1730000006.000000",
        "U1",
        "hey",
    );
    let read = &reads(&[direct], &counted(vec![], false), &state)[0];
    assert_eq!(read.title, "Ada in a direct message in Mooreslab");
}

#[test]
fn slack_markup_reads_as_plain_text() {
    let state = State::default();
    assert_eq!(
        plain_text(
            "<!here> ship <https://x.dev> &lt;now&gt; <@U9>",
            "T1",
            &state
        ),
        "@here ship https://x.dev <now> @U9"
    );
    assert_eq!(
        plain_text("an <unclosed tag", "T1", &state),
        "an <unclosed tag"
    );
}

#[test]
fn a_ts_is_seconds_and_microseconds_and_orders_as_time() {
    assert_eq!(
        ts_time("1730000000.000100").map(|at| at.to_rfc3339()),
        Some("2024-10-27T03:33:20.000100+00:00".to_owned())
    );
    assert!(ts_after("1730000000.000101", "1730000000.000100"));
    assert!(ts_after("1730000001", "999999999.999999"));
    assert!(!ts_after("1730000000.000100", "1730000000.000100"));
}

#[test]
fn content_with_no_selection_reads_everything_and_any_other_shape_is_refused() {
    assert!(matches!(
        parse_content(&Value::Null),
        Ok(Content {
            conversations: Selection::All
        })
    ));
    assert!(matches!(
        parse_content(&json!({})),
        Ok(Content {
            conversations: Selection::All
        })
    ));
    parse_content(&json!({ "conversations": ["T1/C1"] })).expect("accepted");
    parse_content(&json!({ "conversations": 7 })).expect_err("refused");
}

#[test]
fn options_list_each_workspace_its_direct_messages_and_its_channels() {
    let teams = vec![team("T1", "Mooreslab")];
    let options = options_of(
        &teams,
        &[json!({
            "ok": true,
            "channels": [{ "id": "C1", "name": "launch" }, { "id": "C2" }],
            "response_metadata": { "next_cursor": "dXNlcjpVMEc5V0ZYTlo=" },
        })],
    );

    assert_eq!(
        options,
        json!({
            "options": [
                { "id": "T1", "label": "Mooreslab", "detailKey": "slack_workspace" },
                { "id": "T1/dms", "labelKey": "slack_direct_messages", "detail": "Mooreslab" },
                { "id": "T1/C1", "label": "#launch", "detail": "Mooreslab" },
            ],
            "truncated": true,
        })
    );
}

#[test]
fn the_web_client_is_read_and_any_other_slack_page_means_signed_out() {
    assert!(is_app("https://app.slack.com/client/T1/C1"));
    assert!(!is_app("https://slack.com/signin"));
    assert!(is_sign_in("https://slack.com/signin#/signin"));
    assert!(is_sign_in("https://mooreslab.slack.com/"));
    assert!(!is_sign_in("https://slack.com.example.org/"));
    assert!(!is_sign_in("http://slack.com/signin"));
}

#[test]
fn the_page_script_takes_its_request_in_one_place() {
    assert_eq!(SCRIPT.matches(REQUEST_PLACEHOLDER).count(), 1);
    let request = serde_json::to_string(&Request {
        counts: true,
        calls: &[sample_call("T1", "C1")],
    })
    .expect("a request");
    assert_eq!(
        serde_json::from_str::<Value>(&request).expect("JSON"),
        json!({
            "counts": true,
            "calls": [{ "team": "T1", "method": "conversations.history", "params": { "channel": "C1", "limit": 5 } }],
        })
    );
}
