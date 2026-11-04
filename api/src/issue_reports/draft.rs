// Copyright © 2026 Jalapeno Labs

//! What a report posts: its title and Markdown body, rendered from a [`Problem`].
//!
//! Rendering is a pure function of the problem, the environment, the person's note, and the
//! redactor, with no clock and no randomness, so the preview a person confirms is byte for byte
//! what is posted. The body is the same whether it opens an issue or joins one as a comment.
//!
//! Every piece of text that came from outside Elysium's own code (an error, a log line, the
//! note) is redacted, then cut to length, then set in a fenced code block. A fence shows its
//! contents as written: no HTML runs, no `@` notifies anyone, no `#12` links an issue. Cutting
//! after redacting matters: cutting first could leave half a token that no pattern recognizes.

use super::fingerprint::fingerprint;
use super::redact::Redactor;

/// The most characters of an error a report carries.
const ERROR_MAX_CHARACTERS: usize = 4000;

/// The most characters of one related line, such as another check's outcome.
const RELATED_LINE_MAX_CHARACTERS: usize = 300;

/// The most related lines a report carries.
const RELATED_MAX_LINES: usize = 12;

/// The most characters of the person's own note.
pub const NOTE_MAX_CHARACTERS: usize = 2000;

/// The most characters of the error a title quotes. GitHub allows 256 in a title; this leaves
/// the summary in front of it room.
const TITLE_QUOTE_MAX_CHARACTERS: usize = 80;

/// The most characters of a fact's value, which Elysium writes itself.
const FACT_MAX_CHARACTERS: usize = 120;

/// The line that ends every report, before its fingerprint. Search finds a report's issue by
/// the fingerprint, and the match is confirmed by this exact line in its body.
const FINGERPRINT_LINE_PREFIX: &str = "Fingerprint: `";

/// What went wrong, as a subject gathered it, before anything is redacted.
#[derive(Debug, Clone)]
pub struct Problem {
    /// Where it happened, in the fingerprint's terms, such as `connector:jira:check:sign_in`.
    /// Written by Elysium from its own identifiers, never from free text.
    pub category: String,
    /// One sentence saying what failed, in Elysium's words, such as
    /// `The Jira connector's sign_in check failed.`
    pub summary: String,
    /// The error itself, as the provider or Elysium put it.
    pub error: String,
    /// Facts about where it happened, each a label and a value Elysium wrote: a kind, a check,
    /// a state, a reason.
    pub facts: Vec<(&'static str, String)>,
    /// Lines that help read it, such as the other checks' outcomes and recent failed syncs.
    pub related: Vec<String>,
}

/// What was running, and where the report was made from.
#[derive(Debug, Clone)]
pub struct Environment {
    /// The release, or the crate version for a build of `main`.
    pub elysium: String,
    /// The commit and branch the API was built from, and when.
    pub build: String,
    /// The reporter's browser, as [`super::browser::describe`] names it.
    pub browser: String,
}

/// A report, ready to show and to post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub title: String,
    pub body: String,
    pub fingerprint: String,
}

/// The line a report's body carries its fingerprint on, which an issue found by search must
/// hold to count as this problem's.
pub fn fingerprint_line(fingerprint: &str) -> String {
    format!("{FINGERPRINT_LINE_PREFIX}{fingerprint}`")
}

/// The report of `problem`, redacted with `redactor`, with the person's `note` if they wrote
/// one.
pub fn render(
    problem: &Problem,
    environment: &Environment,
    note: Option<&str>,
    redactor: &Redactor,
) -> Draft {
    let error = redactor.redact(problem.error.trim());
    let fingerprint = fingerprint(&problem.category, &error);
    let error = cut(&error, ERROR_MAX_CHARACTERS);

    let first_line = error.lines().next().unwrap_or_default().trim();
    let title = if first_line.is_empty() {
        problem.summary.trim_end_matches('.').to_owned()
    } else {
        format!(
            "{}: {}",
            problem.summary.trim_end_matches('.'),
            cut(first_line, TITLE_QUOTE_MAX_CHARACTERS)
        )
    };

    let mut body = String::new();
    body.push_str("### What happened\n\n");
    body.push_str(&problem.summary);
    body.push_str("\n\n");
    body.push_str(&fenced(&error));

    if !problem.facts.is_empty() {
        body.push_str("\n### Where\n\n");
        let facts: Vec<(&str, &str)> = problem
            .facts
            .iter()
            .map(|(label, value)| (*label, value.as_str()))
            .collect();
        body.push_str(&table(&facts));
    }

    let related: Vec<String> = problem
        .related
        .iter()
        .take(RELATED_MAX_LINES)
        .map(|line| cut(&redactor.redact(line.trim()), RELATED_LINE_MAX_CHARACTERS))
        .filter(|line| !line.is_empty())
        .collect();
    if !related.is_empty() {
        body.push_str("\n### Related\n\n");
        body.push_str(&fenced(&related.join("\n")));
    }

    body.push_str("\n### Environment\n\n");
    body.push_str(&table(&[
        ("Elysium", &environment.elysium),
        ("Build", &environment.build),
        ("Browser", &environment.browser),
    ]));

    let note = note
        .map(|note| cut(&redactor.redact(note.trim()), NOTE_MAX_CHARACTERS))
        .filter(|note| !note.is_empty());
    if let Some(note) = note {
        body.push_str("\n### From the person who reported it\n\n");
        body.push_str(&fenced(&note));
    }

    body.push_str("\n---\n\nReported from Elysium with personal details removed. ");
    body.push_str(&fingerprint_line(&fingerprint));
    body.push('\n');

    Draft {
        title,
        body,
        fingerprint,
    }
}

/// `text` in a fenced code block whose fence is longer than any run of backticks inside it, so
/// nothing in `text` can close it early.
fn fenced(text: &str) -> String {
    let mut longest = 0;
    let mut current = 0;
    for character in text.chars() {
        if character == '`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}text\n{text}\n{fence}\n")
}

/// A two-column Markdown table of labels and values, each value as [`inline_code`].
fn table(rows: &[(&str, &str)]) -> String {
    let rows: Vec<String> = rows
        .iter()
        .map(|(label, value)| format!("| {label} | {} |", inline_code(value)))
        .collect();
    format!("| | |\n|---|---|\n{}\n", rows.join("\n"))
}

/// `value` as inline code in a table cell: one line, no backticks or pipes to break out of it.
fn inline_code(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|character| match character {
            '`' | '|' => '\'',
            '\n' | '\r' | '\t' => ' ',
            other => other,
        })
        .collect();
    format!("`{}`", cut(cleaned.trim(), FACT_MAX_CHARACTERS))
}

/// The first `max` characters of `text`, ending in `…` when anything was cut.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut kept: String = text.chars().take(max.saturating_sub(1)).collect();
    kept.push('…');
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(error: &str) -> Problem {
        Problem {
            category: "connector:jira:check:sign_in".to_owned(),
            summary: "The Jira connector's sign_in check failed.".to_owned(),
            error: error.to_owned(),
            facts: vec![
                ("Connector", "jira".to_owned()),
                ("Check", "sign_in, required".to_owned()),
            ],
            related: vec!["list_projects: skipped, blocked by sign_in".to_owned()],
        }
    }

    fn environment() -> Environment {
        Environment {
            elysium: "1.4.2".to_owned(),
            build: "abc1234 on main, built 2026-11-02T10:00:00Z".to_owned(),
            browser: "Firefox 145 on Linux".to_owned(),
        }
    }

    #[test]
    fn a_report_names_the_problem_where_it_happened_and_what_ran() {
        let draft = render(
            &problem("Jira refused ada@lovelace.dev: 401 Unauthorized"),
            &environment(),
            None,
            &Redactor::new(["Ada Lovelace"]),
        );
        assert_eq!(
            draft.title,
            "The Jira connector's sign_in check failed: Jira refused [email]: 401 Unauthorized"
        );
        for expected in [
            "### What happened",
            "```text\nJira refused [email]: 401 Unauthorized\n```",
            "| Check | `sign_in, required` |",
            "list_projects: skipped, blocked by sign_in",
            "| Elysium | `1.4.2` |",
            "| Browser | `Firefox 145 on Linux` |",
        ] {
            assert!(draft.body.contains(expected), "{expected}\n{}", draft.body);
        }
        assert!(
            draft
                .body
                .ends_with(&format!("{}\n", fingerprint_line(&draft.fingerprint)))
        );
        assert!(!draft.body.contains("lovelace"));
    }

    #[test]
    fn rendering_twice_gives_the_same_report() {
        let redactor = Redactor::new(["Ada Lovelace"]);
        let first = render(
            &problem("timeout"),
            &environment(),
            Some("Ada here"),
            &redactor,
        );
        let second = render(
            &problem("timeout"),
            &environment(),
            Some("Ada here"),
            &redactor,
        );
        assert_eq!(first, second);
    }

    #[test]
    fn two_people_with_the_same_failure_share_a_fingerprint_and_a_title() {
        let first = render(
            &problem("Jira refused ada@lovelace.dev at https://lovelace.atlassian.net/rest"),
            &environment(),
            None,
            &Redactor::new(["Ada Lovelace", "ada@lovelace.dev"]),
        );
        let mut later = environment();
        later.elysium = "1.5.0".to_owned();
        later.browser = "Chrome 141 on Windows".to_owned();
        let second = render(
            &problem("Jira refused grace@navy.mil at https://hopper.atlassian.net/rest"),
            &later,
            Some("Started after the upgrade"),
            &Redactor::new(["Grace Hopper", "grace@navy.mil"]),
        );
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.title, second.title);
    }

    #[test]
    fn the_note_is_redacted_and_fenced_so_it_cannot_mention_or_inject() {
        let draft = render(
            &problem("boom"),
            &environment(),
            Some("cc @octocat <img src=x onerror=alert(1)> ```\n# heading"),
            &Redactor::new(Vec::<String>::new()),
        );
        assert!(!draft.body.contains("@octocat"));
        // The note holds a run of three backticks, so its fence is four long and still holds.
        assert!(
            draft
                .body
                .contains("````text\ncc @[user] <img src=x onerror=alert(1)> ```\n# heading\n````"),
            "{}",
            draft.body
        );
    }

    #[test]
    fn a_fact_cannot_break_out_of_its_table_cell() {
        let mut broken = problem("boom");
        broken.facts = vec![("Reason", "a|b`c\nd".to_owned())];
        let draft = render(
            &broken,
            &environment(),
            None,
            &Redactor::new(Vec::<String>::new()),
        );
        assert!(
            draft.body.contains("| Reason | `a'b'c d` |"),
            "{}",
            draft.body
        );
    }

    #[test]
    fn long_text_is_cut_after_it_is_redacted() {
        // Cut first, the limit would fall inside the token and leave a piece no pattern knows.
        let token = "ghp_16C7e42F292c6912E7710c838347Ae178B4a";
        let error = format!("{}{token}", "word ".repeat((ERROR_MAX_CHARACTERS - 20) / 5));
        let draft = render(
            &problem(&error),
            &environment(),
            None,
            &Redactor::new(Vec::<String>::new()),
        );
        assert!(!draft.body.contains("ghp_16C7"), "part of a token survived");
        assert!(draft.body.contains("word [secret]"), "{}", draft.body);
    }

    #[test]
    fn an_empty_error_titles_the_report_with_its_summary() {
        let draft = render(
            &problem("   "),
            &environment(),
            None,
            &Redactor::new(Vec::<String>::new()),
        );
        assert_eq!(draft.title, "The Jira connector's sign_in check failed");
    }
}
