// Copyright © 2026 Jalapeno Labs

//! A pull request's chain: what it says in a private repository and a public one, that nothing
//! it quotes becomes Markdown, a mention, or a closing keyword, that it stays within its bound,
//! that it replaces its earlier self and leaves the worker's words alone, and that writing it
//! changes a description on GitHub once.

use chrono::Utc;
use uuid::Uuid;

use super::*;
use crate::github::fake::{self, FakeGithub};
use crate::models::action_item::ActionItemState;
use crate::models::action_item_link::LinkProvider;
use crate::models::work_batch::pull_request::PullRequestState;
use crate::models::work_batch::why::{Batch, Finding, Issue, Item, Message, Record};

const PUBLIC_URL: &str = "https://elysium.example.com";

fn public_url() -> Url {
    Url::parse(PUBLIC_URL).expect("a static URL parses")
}

fn batch() -> Batch {
    Batch {
        id: Uuid::now_v7(),
        project_id: Uuid::now_v7(),
        title: "Ship annual pricing".to_owned(),
    }
}

fn segment(document_id: Option<Uuid>) -> Record {
    Record {
        id: Uuid::now_v7(),
        source_kind: "meeting".to_owned(),
        title: "Annual pricing".to_owned(),
        occurred_at: Utc::now(),
        excerpt: "[00:12] Ada: We need annual pricing\n[00:15] Sam: Before the launch".to_owned(),
        meeting_title: Some("Weekly sync".to_owned()),
        document_id,
    }
}

fn item() -> Item {
    Item {
        id: Uuid::now_v7(),
        title: "Annual pricing".to_owned(),
        state: ActionItemState::Open,
    }
}

fn issue(item: &Item) -> Issue {
    Issue {
        action_item_id: item.id,
        provider: LinkProvider::Jira,
        key: "ELY-12".to_owned(),
        url: "https://acme.atlassian.net/browse/ELY-12".to_owned(),
        title: "Annual pricing".to_owned(),
    }
}

/// A meeting's quote, its item's issue, the item, the batch, and a pull request.
fn chain() -> (Why, Uuid) {
    let document = Uuid::now_v7();
    let item = item();
    let why = Why {
        chain: vec![
            Link::Record(segment(Some(document))),
            Link::Issue(issue(&item)),
            Link::Item(item),
            Link::Batch(batch()),
            Link::PullRequest(crate::models::work_batch::why::PullRequest {
                id: Uuid::now_v7(),
                repository: "acme/web".to_owned(),
                number: 7,
                url: "https://github.com/acme/web/pull/7".to_owned(),
                title: "Annual pricing".to_owned(),
                state: PullRequestState::Open,
            }),
        ],
        parents: Vec::new(),
    };
    (why, document)
}

#[test]
fn a_private_repository_reads_the_whole_chain_each_a_link() {
    let (why, document) = chain();
    let section = section(&why, &public_url(), Audience::Company);
    let batch = why.batch();

    assert!(section.starts_with(&format!("{START}\n")), "{section}");
    assert!(section.ends_with(END), "{section}");
    let document_url = format!("{PUBLIC_URL}/knowledge/documents/{document}");
    assert!(
        section.contains(&format!(
            "- Meeting: [Weekly sync\\: Annual pricing]({document_url})"
        )),
        "the segment opens the meeting at that topic: {section}"
    );
    assert!(
        section.contains(
            "> [“\\[00\\:12\\] Ada\\: We need annual pricing \\[00\\:15\\] Sam\\: Before"
        ),
        "its quote opens it too, on one line: {section}"
    );
    assert!(
        section.contains(
            "- Jira issue: [ELY\\-12 Annual pricing](https://acme.atlassian.net/browse/ELY-12)"
        ),
        "{section}"
    );
    assert!(section.contains(&format!(
        "- Work batch: [Ship annual pricing]({PUBLIC_URL}/work-queue/{})",
        batch.id
    )));
    assert!(
        !section.contains("acme/web"),
        "the description is the pull request, so it does not list it: {section}"
    );
    let meeting = section.find("- Meeting").expect("the meeting");
    let issue = section.find("- Jira issue").expect("the issue");
    let item = section.find("- Action item").expect("the item");
    let batch = section.find("- Work batch").expect("the batch");
    assert!(meeting < issue && issue < item && item < batch, "in order");
}

#[test]
fn a_public_repository_reads_what_each_link_is_and_nothing_said_inside_the_company() {
    let (mut why, _document) = chain();
    why.chain.insert(
        0,
        Link::Message(Message {
            id: Uuid::now_v7(),
            author_id: Uuid::now_v7(),
            author: "Alex".to_owned(),
            body: "Ship annual pricing this week.".to_owned(),
            sent_at: Utc::now(),
        }),
    );
    let section = section(&why, &public_url(), Audience::Anyone);

    for said in [
        "Weekly sync",
        "annual pricing",
        "Ada",
        "Alex",
        "Annual pricing",
    ] {
        assert!(
            !section.contains(said),
            "{said} stays in the company: {section}"
        );
    }
    assert!(
        section.contains("- Meeting: [Open in Elysium]("),
        "{section}"
    );
    assert!(
        section.contains("- Jira issue: [ELY\\-12](https://acme.atlassian.net/browse/ELY-12)"),
        "an issue's key says nothing of it: {section}"
    );
    assert!(section.contains(&format!(
        "- Message to its Project Manager: [Open in Elysium]({PUBLIC_URL}/projects/{}?tab=manager)",
        why.batch().project_id
    )));
}

#[test]
fn nothing_quoted_becomes_markdown_a_mention_or_a_closing_keyword() {
    let mut item = item();
    item.title = "Closes #12 for @ada <script>](https://evil.example) **now**".to_owned();
    let why = Why {
        chain: vec![Link::Item(item), Link::Batch(batch())],
        parents: Vec::new(),
    };
    let section = section(&why, &public_url(), Audience::Company);
    let line = section
        .lines()
        .find(|line| line.starts_with("- Action item"))
        .expect("the item's line");

    assert!(
        line.starts_with(
            "- Action item: [Closes \\#12 for \\@ada \\<script\\>\\]\\(https\\:\\/\\/evil\\.example\\) \\*\\*now\\*\\*]("
        ),
        "every mark is escaped inside the link's text: {line}"
    );
}

#[test]
fn a_record_not_indexed_yet_opens_on_the_batchs_page() {
    let why = Why {
        chain: vec![Link::Record(segment(None)), Link::Batch(batch())],
        parents: Vec::new(),
    };
    let section = section(&why, &public_url(), Audience::Company);
    assert!(section.contains(&format!(
        "Annual pricing]({PUBLIC_URL}/work-queue/{})",
        why.batch().id
    )));
}

#[test]
fn an_accepted_finding_says_who_accepted_it() {
    // Found work leads its chain with the finding a person accepted, and its description says who.
    let why = Why {
        chain: vec![
            Link::Finding(Finding {
                id: Uuid::now_v7(),
                title: "The login page leaks its token".to_owned(),
                accepted_by_id: Uuid::now_v7(),
                accepted_by: "Alex".to_owned(),
                accepted_at: Utc::now(),
            }),
            Link::Batch(batch()),
        ],
        parents: Vec::new(),
    };
    let section = section(&why, &public_url(), Audience::Company);
    assert!(
        section.contains(&format!(
            // Punctuation quoted from the finding is escaped, so it stays plain text.
            "- Finding: [The login page leaks its token\\, accepted by Alex]({PUBLIC_URL}/projects/{}?tab=findings)",
            why.batch().project_id
        )),
        "{section}"
    );
}

#[test]
fn a_long_chain_stays_within_its_bound_and_keeps_its_batch() {
    let mut chain: Vec<Link> = Vec::new();
    for index in 0..200 {
        let mut item = item();
        item.title = format!("Item {index} {}", "x".repeat(150));
        chain.push(Link::Item(item));
    }
    chain.push(Link::Batch(batch()));
    let why = Why {
        chain,
        parents: Vec::new(),
    };
    let section = section(&why, &public_url(), Audience::Company);

    assert!(section.chars().count() <= SECTION_MAX_CHARACTERS);
    assert!(section.contains("more on its work batch's page in Elysium"));
    assert!(section.contains("- Work batch: [Ship annual pricing]"));
    assert!(section.contains("Item 0 "), "the chain keeps its start");
}

#[test]
fn splicing_appends_once_then_replaces_in_place() {
    let first = format!("{START}\n### Why this exists\n- one\n{END}");
    let second = format!("{START}\n### Why this exists\n- two\n{END}");

    assert_eq!(splice("", &first), first);
    let worker = "Adds annual pricing.\r\n\r\nCloses #12\r\n";
    let added = splice(worker, &first);
    assert_eq!(
        added,
        format!("Adds annual pricing.\r\n\r\nCloses #12\n\n{first}")
    );

    let edited = format!("{added}\r\n\r\nA note the worker added later.");
    let replaced = splice(&edited, &second);
    assert_eq!(
        replaced,
        format!(
            "Adds annual pricing.\r\n\r\nCloses #12\n\n{second}\r\n\r\nA note the worker added later."
        ),
        "the chain is replaced where it was, and the worker's words are left as they are"
    );
    assert_eq!(
        splice(&replaced, &second),
        replaced,
        "the same chain changes nothing"
    );
}

#[test]
fn a_marker_inside_a_line_is_not_one() {
    let quoted = format!("The worker wrote `{START}` and `{END}` in prose.");
    let section = format!("{START}\n- one\n{END}");
    assert_eq!(splice(&quoted, &section), format!("{quoted}\n\n{section}"));
}

#[tokio::test]
async fn writing_puts_the_chain_in_the_description_once() {
    let github = FakeGithub::start().await;
    let token = SecretString::from(fake::TOKEN);
    let mut node = fake::ready_pull_request_node("acme/web", 7, "Annual pricing");
    node["body"] = serde_json::json!("Adds annual pricing.");
    github.put_pull_request_node("acme/web", 7, node);
    let reference = IssueRef::parse("acme/web#7").expect("a reference");
    let (why, _document) = chain();

    let found = github
        .github
        .pull_request(&token, &reference)
        .await
        .expect("reads")
        .expect("found");
    assert_eq!(
        Audience::of(&found),
        Audience::Company,
        "repositories are private by default"
    );
    let written = write(
        &github.github,
        &token,
        &reference,
        &found,
        &why,
        &public_url(),
    )
    .await
    .expect("written");
    assert_eq!(written, Written::Changed);
    let stored = github.pull_request_node("acme/web", 7)["body"]
        .as_str()
        .expect("a description")
        .to_owned();
    assert!(
        stored.starts_with("Adds annual pricing.\n\n<!-- elysium:why -->"),
        "{stored}"
    );
    assert!(stored.contains("Weekly sync"), "{stored}");

    // Read again, it already carries this chain, so nothing is written.
    let found = github
        .github
        .pull_request(&token, &reference)
        .await
        .expect("reads")
        .expect("found");
    let again = write(
        &github.github,
        &token,
        &reference,
        &found,
        &why,
        &public_url(),
    )
    .await
    .expect("written");
    assert_eq!(again, Written::Unchanged);
    let patches = github
        .writes()
        .into_iter()
        .filter(|request| request.path.ends_with("/pulls/7"))
        .count();
    assert_eq!(patches, 1);
}

#[tokio::test]
async fn a_public_repository_is_written_what_anyone_may_read() {
    let github = FakeGithub::start().await;
    github.make_public("acme/web");
    let token = SecretString::from(fake::TOKEN);
    github.put_pull_request_node(
        "acme/web",
        7,
        fake::ready_pull_request_node("acme/web", 7, "Annual pricing"),
    );
    let reference = IssueRef::parse("acme/web#7").expect("a reference");
    let found = github
        .github
        .pull_request(&token, &reference)
        .await
        .expect("reads")
        .expect("found");
    let (why, _document) = chain();

    write(
        &github.github,
        &token,
        &reference,
        &found,
        &why,
        &public_url(),
    )
    .await
    .expect("written");
    let stored = github.pull_request_node("acme/web", 7)["body"].to_string();
    assert!(!stored.contains("Weekly sync"), "{stored}");
    assert!(stored.contains("Open in Elysium"), "{stored}");
}

#[tokio::test]
async fn a_description_past_githubs_limit_is_left_as_it_is() {
    let github = FakeGithub::start().await;
    let token = SecretString::from(fake::TOKEN);
    let mut node = fake::ready_pull_request_node("acme/web", 7, "Annual pricing");
    node["body"] = serde_json::json!("x".repeat(GITHUB_MAX_CHARACTERS - 10));
    github.put_pull_request_node("acme/web", 7, node);
    let reference = IssueRef::parse("acme/web#7").expect("a reference");
    let found = github
        .github
        .pull_request(&token, &reference)
        .await
        .expect("reads")
        .expect("found");
    let (why, _document) = chain();

    let refused = write(
        &github.github,
        &token,
        &reference,
        &found,
        &why,
        &public_url(),
    )
    .await;
    assert!(
        matches!(refused, Err(DescriptionError::TooLong)),
        "{refused:?}"
    );
    // The read is a GraphQL POST, so the description's own write is told apart by its path.
    let patches = github
        .writes()
        .into_iter()
        .filter(|request| request.path.ends_with("/pulls/7"))
        .count();
    assert_eq!(patches, 0, "nothing was written");
}
