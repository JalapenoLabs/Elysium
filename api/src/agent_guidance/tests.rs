// Copyright © 2026 Jalapeno Labs

//! What every agent reads (#98): its server's introduction, the guidance, and the rules no edit
//! removes, all within what Claude Code reads of a server's instructions; the `CLAUDE.md` block;
//! the version kept in memory never going back; and the first version, seeded by its migration,
//! saying what the MCP server said before the guidance could be edited.
//!
//! The test that reads Postgres is `#[ignore]`d so `cargo test` stays hermetic;
//! `api/scripts/verify-migrations.sh` runs it.

use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::pooled_connection::deadpool::Pool as DeadPool;

use super::*;
use crate::models::user::SYSTEM_USER_ID;
use crate::test_support::migrated_database;

/// Version 1, as its migration writes it: the MCP server's instructions before they could be
/// edited, less the rules [`rules`] adds to every version.
const SEED: &str = "Before asking the person something the company may already know, or \
working out again what someone may have settled, search the knowledge base with \
knowledge_search: what was said in mail, chat, meetings, and pages, and what other agents \
learned. Open what a search cites with knowledge_get, and list past meetings with \
meetings_list. Cite what you use by its title, who said it, and when.\n\n\
The company doc says who the company is and how its people, clients, and terms are spelled: \
read it with company_doc_read before writing about them, and propose what you learn for certain \
with company_doc_propose, which waits for a person's approval.\n\n\
When you learn something the next agent would otherwise have to rediscover, save it with \
learning_save; everyone in the company reads what is saved there, and nothing in it is for \
repeating outside the company.";

fn deployment() -> Url {
    Url::parse("https://elysium.example.com").expect("a static URL parses")
}

fn version(number: i32, content: &str) -> GuidanceVersion {
    GuidanceVersion {
        version: number,
        content: content.to_owned(),
        restored_from: None,
        created_by: SYSTEM_USER_ID,
        created_at: Utc::now(),
    }
}

#[test]
fn every_agent_reads_its_server_then_the_guidance_then_the_rules() {
    for audience in Audience::ALL {
        let text = instructions(audience, "Search before asking.", &deployment());
        let paragraphs: Vec<&str> = text.split(PARAGRAPH_BREAK).collect();
        assert_eq!(
            paragraphs,
            [
                audience.introduction(),
                "Search before asking.",
                rules(&deployment()).as_str(),
            ],
            "{audience:?}"
        );
    }
}

#[test]
fn the_rules_keep_results_in_the_company_and_say_where_a_citation_opens() {
    let text = instructions(Audience::McpClients, "", &deployment());
    assert!(
        text.ends_with(
            "A person opens any citation in Elysium at \
             https://elysium.example.com/knowledge/documents/<documentId>."
        ),
        "{text}"
    );
    assert!(text.contains("coworker's mail"), "{text}");
    assert!(
        text.contains("never for repeating outside the company"),
        "{text}"
    );
    assert!(
        !text.contains("\n\n\n"),
        "empty guidance leaves no gap: {text}"
    );
}

#[test]
fn the_longest_guidance_still_fits_what_claude_code_reads() {
    let public_url = deployment();
    let max = content_max_characters(&public_url);
    let longest = "x".repeat(max);

    let mut fullest = 0;
    for audience in Audience::ALL {
        let length = instructions(audience, &longest, &public_url)
            .chars()
            .count();
        assert!(
            length <= INSTRUCTIONS_MAX_CHARACTERS,
            "{audience:?}: {length}"
        );
        fullest = fullest.max(length);
    }
    assert_eq!(
        fullest, INSTRUCTIONS_MAX_CHARACTERS,
        "the limit leaves nothing unused for the longest introduction"
    );
    assert!(
        max >= 3_000,
        "the guidance has room to grow well past its seed: {max}"
    );
}

#[test]
fn the_seed_reads_as_the_mcp_server_read_before_it_could_be_edited() {
    let text = instructions(Audience::McpClients, SEED, &deployment());
    let before = [
        "Elysium is a shared workspace of projects, action items, initiatives, and coding \
         sessions, and the company's knowledge base.",
        "start one with sessions_create, steer it with sessions_send_prompt, and follow its work \
         with sessions_events.",
        "search the knowledge base with knowledge_search",
        "Cite what you use by its title, who said it, and when.",
        "read it with company_doc_read before writing about them",
        "save it with learning_save",
        "they are the company's to use for its own work, and never for repeating outside the \
         company or in work for another client.",
        "https://elysium.example.com/knowledge/documents/<documentId>.",
    ];
    let mut from = 0;
    for sentence in before {
        let found = text[from..]
            .find(sentence)
            .unwrap_or_else(|| panic!("{sentence:?} follows what came before it in: {text}"));
        from += found + sentence.len();
    }
    for audience in Audience::ALL {
        assert!(
            instructions(audience, SEED, &deployment()).chars().count()
                < INSTRUCTIONS_MAX_CHARACTERS
        );
    }
}

#[test]
fn a_turn_is_told_to_report_a_learning_it_cannot_save() {
    let text = instructions(Audience::Turns, SEED, &deployment());
    assert!(text.contains("This server has no learning_save"), "{text}");
}

#[test]
fn the_claude_md_block_names_its_version_says_how_to_connect_and_keeps_the_rules() {
    let block = claude_md(&version(4, "Search before asking."), &deployment());
    assert!(
        block.starts_with("<!-- Elysium's agent guidance, version 4."),
        "{block}"
    );
    assert!(
        block.contains(
            "claude mcp add --transport http elysium https://elysium.example.com/api/mcp"
        ),
        "{block}"
    );
    assert!(block.contains("\n\nSearch before asking.\n\n"), "{block}");
    assert!(
        block.ends_with(&format!("{}\n", rules(&deployment()))),
        "{block}"
    );

    let empty = claude_md(&version(5, ""), &deployment());
    assert!(!empty.contains("\n\n\n"), "{empty}");
    assert!(
        empty.ends_with(&format!("{}\n", rules(&deployment()))),
        "{empty}"
    );
}

/// A pool that never connects: building one opens nothing, and these tests never ask it for a
/// connection.
fn unconnected() -> AgentGuidance {
    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(
        "postgres://nobody@127.0.0.1:9/nothing",
    );
    let pool = DeadPool::builder(manager).build().expect("a pool builds");
    AgentGuidance::new(pool, deployment())
}

#[test]
fn the_version_kept_never_goes_back() {
    let guidance = unconnected();
    assert!(guidance.kept().is_none());

    guidance.remember(version(2, "Second."));
    // A read that began before version 2 was saved finishes after it.
    let kept = guidance.remember(version(1, "First."));
    assert_eq!(kept.version, 2);
    assert_eq!(guidance.kept().map(|kept| kept.version), Some(2));

    guidance.remember(version(3, "Third."));
    assert_eq!(
        guidance.kept().map(|kept| kept.content.clone()),
        Some("Third.".to_owned())
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn the_first_version_is_the_seed_by_the_system_user() {
    let (_url, mut connection) = migrated_database().await;

    let current = agent_guidance::current(&mut connection)
        .await
        .expect("the seed exists");

    assert_eq!(current.version, 1);
    assert_eq!(current.content, SEED);
    assert_eq!(current.created_by, SYSTEM_USER_ID);
    assert_eq!(current.restored_from, None);
}
