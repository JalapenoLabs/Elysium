// Copyright © 2026 Jalapeno Labs

//! Finding action items, projects, coding sessions, and initiatives by what was typed.
//!
//! The global search (`GET /api/v1/search`, `crate::routes::v1::search`) asks each kind's
//! model for its closest titles, using the pieces here, so every kind matches and ranks alike:
//!
//! - **Matching.** Every typed word must begin a word of the title, case ignored, as the web
//!   app's registry of pages matches (`frontend/src/search/searchMatch.ts`). A word is a run of
//!   letters and digits; anything else separates words, so `way 123` finds `WAY-123`. Each
//!   word becomes `title ~* '\mword'`, which `pg_trgm`'s GIN index on the title answers (see the
//!   `index_titles_for_search` migration for why trigrams rather than full text).
//! - **Ranking.** A title that begins with the typed words in order comes first, then the
//!   closest by `pg_trgm`'s `similarity`, which favors a title that is mostly the typed words,
//!   then alphabetically. The web app ranks the merged results again with its own score; this
//!   order decides which of a kind's matches reach it.
//! - **Budget.** One person may search [`PER_PERSON`]'s rate, on top of the API's limit per
//!   client address.

use diesel::dsl::AsExprOf;
use diesel::expression::{AsExpression, Expression};
use diesel::pg::Pg;
use diesel::sql_types::Text;

use crate::middleware::rate_limit::Rate;

/// The longest query searched, in characters. A search box holds a few words; anything
/// longer is a question for Elysia, which the web app offers beside the results.
pub const MAX_QUERY_CHARS: u64 = 200;

/// Results per kind when the caller names no limit: the topbar shows the closest five overall,
/// so five of each kind always holds them.
pub const DEFAULT_LIMIT: i64 = 5;

/// The most results per kind one search answers. The search page asks for this many.
pub const MAX_LIMIT: i64 = 50;

/// One person's searches: the web app waits 300ms after typing stops before it asks, so
/// someone typing steadily sends about three a second. Twenty at once and five a second
/// leaves room for that and a few cancelled requests, while a script looping over the route
/// is held to five a second.
pub const PER_PERSON: Rate = Rate {
    per_second: 5,
    burst: 20,
};

diesel::infix_operator!(MatchesIgnoringCase, " ~* ", backend: Pg);

diesel::define_sql_function! {
    /// `pg_trgm`'s `similarity`: from 0 to 1, how much of both texts' trigrams they share.
    fn similarity(left: Text, right: Text) -> diesel::sql_types::Float4;
}

/// What was typed, as the patterns a title is matched and ranked by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTerms {
    /// The words, joined by spaces, for ranking by similarity.
    text: String,
    /// One `\mword` per word; a title matches when it matches every one.
    word_patterns: Vec<String>,
    /// Matches a title whose first words begin with the typed words, in order.
    title_start_pattern: String,
}

impl SearchTerms {
    /// The words of `typed`: runs of letters and digits, lowercased. Only those characters
    /// reach a pattern, so nothing typed can change what a pattern means.
    pub fn parse(typed: &str) -> Self {
        let words: Vec<String> = typed
            .split(|character: char| !character.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect();

        let word_patterns = words.iter().map(|word| format!(r"\m{word}")).collect();
        // `^`, then each word at the start of the next word: "new p" begins "New project".
        let title_start_pattern = format!(
            r"^[^[:alnum:]]*{}",
            words.join(r"[[:alnum:]]*[^[:alnum:]]+")
        );

        Self {
            text: words.join(" "),
            word_patterns,
            title_start_pattern,
        }
    }

    /// Whether nothing searchable was typed, such as only spaces or punctuation.
    pub fn is_empty(&self) -> bool {
        self.word_patterns.is_empty()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn word_patterns(&self) -> &[String] {
        &self.word_patterns
    }

    pub fn title_start_pattern(&self) -> &str {
        &self.title_start_pattern
    }
}

/// `column ~* pattern`: whether a title matches one of [`SearchTerms`]'s patterns.
pub fn matches<Column>(
    column: Column,
    pattern: &str,
) -> MatchesIgnoringCase<Column, AsExprOf<String, Text>>
where
    Column: Expression<SqlType = Text>,
{
    MatchesIgnoringCase::new(
        column,
        AsExpression::<Text>::as_expression(pattern.to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_runs_of_letters_and_digits_lowercased() {
        let terms = SearchTerms::parse("  Fix WAY-123, on «Réglages»  ");
        assert_eq!(terms.text(), "fix way 123 on réglages");
        assert_eq!(
            terms.word_patterns(),
            [r"\mfix", r"\mway", r"\m123", r"\mon", r"\mréglages"]
        );
    }

    #[test]
    fn nothing_but_spaces_or_punctuation_is_empty() {
        assert!(SearchTerms::parse("   ").is_empty());
        assert!(SearchTerms::parse("-- ' % _ \\ .*").is_empty());
        assert!(!SearchTerms::parse("a").is_empty());
    }

    // The patterns go to Postgres as regular expressions, so nothing typed may reach one but
    // letters and digits.
    #[test]
    fn regular_expression_syntax_never_reaches_a_pattern() {
        let terms = SearchTerms::parse(r"a.* (b|c) [d] \m e+ ^f$ _g");
        for pattern in terms.word_patterns() {
            let word = pattern
                .strip_prefix(r"\m")
                .expect("each pattern starts a word");
            assert!(word.chars().all(char::is_alphanumeric), "{pattern}");
        }
    }

    #[test]
    fn the_title_start_pattern_puts_each_word_at_the_next_word() {
        assert_eq!(
            SearchTerms::parse("New p").title_start_pattern(),
            r"^[^[:alnum:]]*new[[:alnum:]]*[^[:alnum:]]+p"
        );
        assert_eq!(
            SearchTerms::parse("fix").title_start_pattern(),
            r"^[^[:alnum:]]*fix"
        );
    }
}
