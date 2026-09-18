// Copyright © 2026 Jalapeno Labs

//! The one place a Jira call is checked against what its credential may touch.
//!
//! Every read and every write goes through here, in one of two ways:
//!
//! - Where an issue key names the project, [`issue_project`] checks it **before** the call,
//!   and [`ensure_allowed`] checks the project Jira reports **after** it, so an issue moved
//!   to another project since the key was written cannot slip through.
//! - A search is **bounded** rather than filtered: [`bounded_jql`] rewrites the caller's JQL
//!   so Jira itself never looks outside the allowed projects. Filtering the answer would
//!   still have sent the question. Wrapping only holds for JQL that can be wrapped, so the
//!   caller's query is checked for balanced parentheses and terminated quotes first, and
//!   refused when it is neither.
//!
//! Nothing here calls Jira or touches the database, so every rule below is a plain function
//! with a test.

use crate::errors::ApiError;
use crate::jira::project_of_issue_key;
use crate::models::jira_credential::{AllowedProject, Allowlist};

/// How a search is ordered when the caller's JQL says nothing about it.
const DEFAULT_ORDERING: &str = "updated DESC";

/// Refuses a project this credential may not touch, naming the project and the credential.
///
/// # Errors
/// Returns [`ApiError::Forbidden`] for a project outside the allowlist.
pub fn ensure_allowed(
    projects: &Allowlist<AllowedProject>,
    credential_name: &str,
    project_key: &str,
) -> Result<(), ApiError> {
    if projects.allows(project_key) {
        return Ok(());
    }

    Err(ApiError::Forbidden(format!(
        "project {project_key} is not one the Jira credential {credential_name} may touch"
    )))
}

/// The project an issue key names, refused unless this credential may touch it.
///
/// # Errors
/// Returns [`ApiError::BadRequest`] when the key is not an issue key, and
/// [`ApiError::Forbidden`] when its project is outside the allowlist.
pub fn issue_project<'key>(
    projects: &Allowlist<AllowedProject>,
    credential_name: &str,
    issue_key: &'key str,
) -> Result<&'key str, ApiError> {
    let Some(project_key) = project_of_issue_key(issue_key) else {
        return Err(ApiError::BadRequest(format!(
            "{issue_key} is not an issue key; an issue key is a project key, a hyphen, and a \
             number, such as ELY-12"
        )));
    };
    ensure_allowed(projects, credential_name, project_key)?;
    Ok(project_key)
}

/// The caller's JQL bounded to the allowed projects, or `None` when nothing is allowed.
///
/// `None` means the credential holds an empty list of projects, so no query can match and
/// the caller should be answered an empty page rather than sent to Jira with `project IN ()`,
/// which is not valid JQL.
///
/// The caller's ordering is kept, since `(… ORDER BY x) AND …` is not valid JQL and dropping
/// it would quietly reorder their results. With no JQL at all, the answer is every allowed
/// project, newest first.
///
/// # Errors
/// Returns [`ApiError::BadRequest`] for JQL that cannot be wrapped: see [`split_ordering`].
pub fn bounded_jql(
    jql: &str,
    projects: &Allowlist<AllowedProject>,
) -> Result<Option<String>, ApiError> {
    let (conditions, ordering) = split_ordering(jql.trim())?;
    let ordering = if ordering.is_empty() {
        DEFAULT_ORDERING
    } else {
        ordering
    };

    let bounded = match projects {
        Allowlist::All => conditions.to_owned(),
        Allowlist::Only(allowed) => {
            if allowed.is_empty() {
                return Ok(None);
            }
            // Keys are quoted because a project key may be a JQL reserved word, and their
            // shape is checked both in the API and by a constraint on the link table, so a
            // key can hold no quote of its own.
            let keys: Vec<String> = allowed
                .iter()
                .map(|project| format!("\"{}\"", project.key))
                .collect();
            let scope = format!("project IN ({})", keys.join(", "));
            if conditions.is_empty() {
                scope
            } else {
                format!("({conditions}) AND {scope}")
            }
        }
    };

    if bounded.is_empty() {
        return Ok(Some(format!("ORDER BY {ordering}")));
    }
    Ok(Some(format!("{bounded} ORDER BY {ordering}")))
}

/// Splits JQL into its conditions and its ordering, at the last top level `ORDER BY`.
///
/// One pass does the splitting and the checking, because both need the same state.
///
/// Quotes are tracked so an `order by` inside a quoted value, such as
/// `summary ~ "sort order by date"`, is left where it is, and so is a parenthesis inside
/// one. Parentheses are counted for two reasons: an `ORDER BY` inside a pair is part of a
/// sub-expression rather than the query's ordering, and the conditions are about to be
/// wrapped in a pair of Elysium's own, which only holds while they balance on their own.
///
/// # Errors
/// Returns [`ApiError::BadRequest`] for a quote that is never closed, or a parenthesis that
/// is closed before it is opened or opened and never closed. `status = Open) OR (project =
/// SECRET` balances by count, yet its first `)` closes the parenthesis Elysium opened, which
/// would leave the bound clause governing only the last disjunct.
fn split_ordering(jql: &str) -> Result<(&str, &str), ApiError> {
    let bytes = jql.as_bytes();
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut depth: usize = 0;
    let mut ordering_at: Option<(usize, usize)> = None;

    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
        } else if let Some(open) = quote {
            if byte == b'\\' {
                escaped = true;
            } else if byte == open {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            let Some(outside) = depth.checked_sub(1) else {
                return Err(ApiError::BadRequest(
                    "the JQL closes a parenthesis it never opened".to_owned(),
                ));
            };
            depth = outside;
        } else if depth == 0
            && let Some(end) = order_by(bytes, index)
        {
            ordering_at = Some((index, end));
            index = end;
            continue;
        }
        index += 1;
    }

    if let Some(open) = quote {
        return Err(ApiError::BadRequest(format!(
            "the JQL opens a {} quote it never closes",
            char::from(open)
        )));
    }
    if depth != 0 {
        return Err(ApiError::BadRequest(
            "the JQL opens a parenthesis it never closes".to_owned(),
        ));
    }

    match ordering_at {
        Some((start, end)) => Ok((jql[..start].trim(), jql[end..].trim())),
        None => Ok((jql.trim(), "")),
    }
}

/// Where an `ORDER BY` starting at `index` ends, in any case and with any spacing.
fn order_by(bytes: &[u8], index: usize) -> Option<usize> {
    // A keyword starts where a word starts: at the beginning, or after something that is
    // not part of one, so `reorder by` is a field named `reorder`.
    let starts_word = index
        .checked_sub(1)
        .map(|before| bytes[before])
        .is_none_or(|previous| !previous.is_ascii_alphanumeric() && previous != b'_');
    if !starts_word {
        return None;
    }

    let after_order = expect_keyword(bytes, index, b"order")?;
    let after_spaces = skip_spaces(bytes, after_order)?;
    let after_by = expect_keyword(bytes, after_spaces, b"by")?;
    // `ORDER BYTES` is a field name, not an ordering.
    match bytes.get(after_by) {
        Some(next) if next.is_ascii_alphanumeric() || *next == b'_' => None,
        _ => Some(after_by),
    }
}

/// Where `keyword` ends if it sits at `index`, ignoring case.
fn expect_keyword(bytes: &[u8], index: usize, keyword: &[u8]) -> Option<usize> {
    let end = index + keyword.len();
    let found = bytes.get(index..end)?;
    if found.eq_ignore_ascii_case(keyword) {
        return Some(end);
    }
    None
}

/// Where the run of whitespace at `index` ends, or `None` if there is none.
fn skip_spaces(bytes: &[u8], index: usize) -> Option<usize> {
    let mut end = index;
    while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
        end += 1;
    }
    if end == index {
        return None;
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(key: &str) -> AllowedProject {
        AllowedProject {
            id: "10001".to_owned(),
            key: key.to_owned(),
            name: format!("{key} project"),
        }
    }

    fn allowed(keys: &[&str]) -> Allowlist<AllowedProject> {
        Allowlist::Only(keys.iter().map(|key| project(key)).collect())
    }

    #[test]
    fn a_project_outside_the_list_is_refused_by_name() {
        let projects = allowed(&["ELY", "OPS"]);

        ensure_allowed(&projects, "Work", "ELY").expect("an allowed project");
        ensure_allowed(&projects, "Work", "ops").expect("keys are compared in any case");
        ensure_allowed(&Allowlist::All, "Work", "ANY").expect("every project");

        let refused = ensure_allowed(&projects, "Work", "SECRET").unwrap_err();
        let ApiError::Forbidden(message) = refused else {
            panic!("a project outside the list is forbidden, not something else");
        };
        assert!(message.contains("SECRET"), "{message}");
        assert!(message.contains("Work"), "{message}");
    }

    #[test]
    fn an_issue_keys_project_is_checked_before_the_call() {
        let projects = allowed(&["ELY"]);

        assert_eq!(
            issue_project(&projects, "Work", "ELY-12").expect("allowed"),
            "ELY"
        );
        assert_eq!(
            issue_project(&projects, "Work", "ely-12").expect("allowed in any case"),
            "ely"
        );

        assert!(matches!(
            issue_project(&projects, "Work", "OPS-1").unwrap_err(),
            ApiError::Forbidden(_)
        ));
        for not_a_key in ["ELY", "ELY-", "-12", "ELY-12-3", "ELY-abc", "../../secret"] {
            assert!(
                matches!(
                    issue_project(&projects, "Work", not_a_key).unwrap_err(),
                    ApiError::BadRequest(_)
                ),
                "{not_a_key}"
            );
        }
    }

    /// The JQL Elysium would send for `jql`, for the assertions that expect one.
    fn bounded(jql: &str, projects: &Allowlist<AllowedProject>) -> String {
        bounded_jql(jql, projects)
            .expect("JQL that wraps")
            .expect("an allowlist that can match")
    }

    #[test]
    fn a_search_is_bounded_to_the_allowed_projects() {
        let projects = allowed(&["ELY", "OPS"]);

        assert_eq!(
            bounded("status = Open", &projects),
            r#"(status = Open) AND project IN ("ELY", "OPS") ORDER BY updated DESC"#
        );
        assert_eq!(
            bounded("", &projects),
            r#"project IN ("ELY", "OPS") ORDER BY updated DESC"#,
            "no JQL means every allowed project, newest first"
        );
    }

    #[test]
    fn the_callers_ordering_is_hoisted_out_of_the_bound_clause() {
        let projects = allowed(&["ELY"]);

        assert_eq!(
            bounded("status = Open ORDER BY created ASC", &projects),
            r#"(status = Open) AND project IN ("ELY") ORDER BY created ASC"#
        );
        assert_eq!(
            bounded("order by priority DESC", &projects),
            r#"project IN ("ELY") ORDER BY priority DESC"#,
            "an ordering on its own still bounds"
        );
        assert_eq!(
            bounded("status = Open ORDER   BY created", &projects),
            r#"(status = Open) AND project IN ("ELY") ORDER BY created"#,
            "any spacing between the two words"
        );
    }

    #[test]
    fn an_order_by_inside_a_quoted_value_is_left_alone() {
        let projects = allowed(&["ELY"]);

        assert_eq!(
            bounded(r#"summary ~ "sort order by date""#, &projects),
            r#"(summary ~ "sort order by date") AND project IN ("ELY") ORDER BY updated DESC"#
        );
        assert_eq!(
            bounded(r#"summary ~ "a \" order by b" ORDER BY created"#, &projects),
            r#"(summary ~ "a \" order by b") AND project IN ("ELY") ORDER BY created"#,
            "an escaped quote does not end the value"
        );
        assert_eq!(
            bounded("reorder = 1 AND orderby = 2", &projects),
            r#"(reorder = 1 AND orderby = 2) AND project IN ("ELY") ORDER BY updated DESC"#,
            "only the whole keyword, on its own, is an ordering"
        );
        assert_eq!(
            bounded("status = Open ORDER BY a ORDER BY b", &projects),
            r#"(status = Open ORDER BY a) AND project IN ("ELY") ORDER BY b"#,
            "two orderings are not valid JQL to begin with; the split is at the last one and \
             Jira is left to refuse it"
        );
    }

    #[test]
    fn jql_that_would_break_out_of_the_bound_clause_is_refused() {
        let projects = allowed(&["ELY"]);

        // The parentheses balance by count, so only their order gives this away: the first
        // `)` closes the one Elysium is about to open. Wrapped, Jira would read it as
        // `status = Open OR (project = SECRET AND project IN ("ELY"))`, whose first
        // disjunct carries no bound at all.
        let refused = bounded_jql("status = Open) OR (project = SECRET", &projects).unwrap_err();
        let ApiError::BadRequest(message) = refused else {
            panic!("JQL Elysium cannot bound is the caller's to fix, so a bad request");
        };
        assert!(message.contains("parenthesis"), "{message}");

        for unwrappable in [
            "(status = Open",
            "status = Open)",
            "((status = Open)",
            r#"summary ~ "never closed"#,
            "summary ~ 'never closed",
            r#"summary ~ "escaped \""#,
        ] {
            assert!(
                matches!(
                    bounded_jql(unwrappable, &projects),
                    Err(ApiError::BadRequest(_))
                ),
                "{unwrappable}"
            );
        }
    }

    #[test]
    fn a_balanced_query_is_bounded_exactly_as_it_was_written() {
        let projects = allowed(&["ELY"]);

        assert_eq!(
            bounded(
                "(status = Open OR status = Reopened) AND assignee = currentUser()",
                &projects
            ),
            concat!(
                r#"((status = Open OR status = Reopened) AND assignee = currentUser()) "#,
                r#"AND project IN ("ELY") ORDER BY updated DESC"#
            ),
            "parentheses of the caller's own are kept, and the whole is wrapped in one pair"
        );
        assert_eq!(
            bounded(r#"summary ~ "a (b" AND text ~ 'it\'s )'"#, &projects),
            concat!(
                r#"(summary ~ "a (b" AND text ~ 'it\'s )') "#,
                r#"AND project IN ("ELY") ORDER BY updated DESC"#
            ),
            "a parenthesis or a quote inside a value is text, not structure"
        );
        assert_eq!(
            bounded("(status = Open) ORDER BY created", &projects),
            r#"((status = Open)) AND project IN ("ELY") ORDER BY created"#,
            "an ordering after a closed pair is still the query's own"
        );
    }

    #[test]
    fn every_project_adds_no_clause_and_no_projects_matches_nothing() {
        assert_eq!(
            bounded("status = Open", &Allowlist::All),
            "status = Open ORDER BY updated DESC"
        );
        assert_eq!(bounded("", &Allowlist::All), "ORDER BY updated DESC");

        assert_eq!(
            bounded_jql("status = Open", &Allowlist::Only(Vec::new())).expect("JQL that wraps"),
            None,
            "a credential with no projects can match nothing, so nothing is asked of Jira"
        );
    }
}
