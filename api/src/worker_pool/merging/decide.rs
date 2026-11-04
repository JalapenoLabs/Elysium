// Copyright © 2026 Jalapeno Labs

//! Whether Elysium merges a worker's pull request now: one pure function of what GitHub said
//! ([`decide`]), tested alone.
//!
//! It fails closed. A pull request merges only when its repository lets Elysium merge
//! ([`MergePolicy::Auto`]) and every one of these holds: the project's account opened it, from
//! the repository itself rather than a fork; it is no draft; GitHub says it merges cleanly and
//! that nothing the base branch requires is missing (`mergeStateStatus` is `CLEAN`); its head
//! commit has checks, every one of them passed, and every check the branch's rules require is
//! among them; no reviewer's latest review asks for changes and no required review is owed;
//! every review thread is resolved; the branch does not merge through a merge queue; and the
//! repository and its rules allow a way to merge. Anything GitHub did not answer, answered in a
//! shape this build does not know, or answered only in part (more threads, reviews, or checks
//! than one read takes) holds it. Every reason that holds it is named, so the worker can fix
//! them all at once.

use std::fmt;

use chrono::{DateTime, Utc};

use crate::github::pulls::{
    BranchRules, CheckOutcome, MergeState, Mergeability, PullRequest, PullRequestStatus,
    ReviewDecision,
};
use crate::models::work_batch::pull_request::MergeMethod;
use crate::models::worker_setup::MergePolicy;

/// The order a way to merge is picked in, among those the repository and its rules allow:
/// squashing keeps one commit per pull request on the base branch, a merge commit keeps the
/// branch's commits as they were signed, and rebasing, which rewrites every commit, comes last.
const METHOD_PREFERENCE: [MergeMethod; 3] =
    [MergeMethod::Squash, MergeMethod::Merge, MergeMethod::Rebase];

/// What a look does with a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It merged already, by whoever GitHub names, when it says.
    Merged {
        by: Option<String>,
        at: Option<DateTime<Utc>>,
    },
    /// It was closed without merging.
    Closed,
    /// A person merges pull requests in its repository.
    PersonMerges,
    /// Nothing holds it: merge it this way.
    Merge(MergeMethod),
    /// It stays open, for every one of these reasons.
    Hold(Vec<Hold>),
}

/// One reason a pull request stays open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hold {
    /// GitHub reported a state this build does not know.
    StatusUnknown,
    /// Someone other than the project's account opened it.
    NotTheProjectsAccount {
        author: Option<String>,
        account: String,
    },
    /// Its head is on a fork.
    FromAFork,
    Draft,
    Conflicts,
    /// GitHub has not worked out whether it merges cleanly yet.
    MergeabilityUnknown,
    /// Its head is behind the base branch, which the branch's rules require it not to be.
    Behind,
    /// A rule of the base branch blocks it.
    Blocked,
    /// GitHub did not say whether it may merge.
    MergeStateUnknown,
    /// A check GitHub counts as failing holds it, though none read here failed.
    Unstable,
    /// No check ever reported on its head commit.
    NoChecks,
    /// It has more checks than one read takes.
    TooManyChecks,
    /// These checks have not finished.
    ChecksPending(Vec<String>),
    /// These checks did not pass.
    ChecksFailed(Vec<String>),
    /// These checks the base branch requires never reported.
    RequiredChecksMissing(Vec<String>),
    /// A reviewer asked for changes.
    ChangesRequested,
    /// The base branch requires an approving review it does not have.
    ReviewRequired,
    /// GitHub reported a review decision this build does not know.
    ReviewDecisionUnknown,
    /// It has more reviews than one read takes.
    TooManyReviews,
    /// This many review threads are not resolved.
    OpenThreads(usize),
    /// It has more review threads than one read takes.
    TooManyThreads,
    /// The base branch's rules could not be read.
    RulesUnread,
    /// The base branch merges only through a merge queue, which Elysium does not drive.
    MergeQueue,
    /// The repository and its rules leave no way to merge.
    NoMergeMethod,
}

impl fmt::Display for Hold {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StatusUnknown => {
                formatter.write_str("GitHub reports a state Elysium does not know")
            }
            Self::NotTheProjectsAccount { author, account } => write!(
                formatter,
                "{} opened it, not the project's account {account}",
                author
                    .as_deref()
                    .unwrap_or("an account GitHub no longer has")
            ),
            Self::FromAFork => formatter.write_str("its branch is on a fork"),
            Self::Draft => formatter.write_str("it is a draft"),
            Self::Conflicts => formatter.write_str("it conflicts with its base branch"),
            Self::MergeabilityUnknown => {
                formatter.write_str("GitHub is still working out whether it merges cleanly")
            }
            Self::Behind => formatter.write_str(
                "its branch is behind the base branch, which must be brought up to date",
            ),
            Self::Blocked => formatter
                .write_str("the base branch's rules block it, such as a required review or check"),
            Self::MergeStateUnknown => {
                formatter.write_str("GitHub does not say whether it may merge")
            }
            Self::Unstable => formatter.write_str("GitHub counts a check on it as failing"),
            Self::NoChecks => formatter.write_str("no check ran on its latest commit"),
            Self::TooManyChecks => {
                formatter.write_str("it has more checks than Elysium reads, so a person merges it")
            }
            Self::ChecksPending(names) => {
                write!(
                    formatter,
                    "these checks are still running: {}",
                    names.join(", ")
                )
            }
            Self::ChecksFailed(names) => {
                write!(formatter, "these checks did not pass: {}", names.join(", "))
            }
            Self::RequiredChecksMissing(names) => write!(
                formatter,
                "these required checks never reported: {}",
                names.join(", ")
            ),
            Self::ChangesRequested => formatter.write_str("a reviewer asked for changes"),
            Self::ReviewRequired => formatter.write_str("it needs an approving review"),
            Self::ReviewDecisionUnknown => {
                formatter.write_str("GitHub reports a review decision Elysium does not know")
            }
            Self::TooManyReviews => {
                formatter.write_str("it has more reviews than Elysium reads, so a person merges it")
            }
            Self::OpenThreads(1) => formatter.write_str("1 review thread is not resolved"),
            Self::OpenThreads(count) => {
                write!(formatter, "{count} review threads are not resolved")
            }
            Self::TooManyThreads => formatter
                .write_str("it has more review threads than Elysium reads, so a person merges it"),
            Self::RulesUnread => formatter.write_str("the base branch's rules could not be read"),
            Self::MergeQueue => formatter.write_str(
                "the base branch merges through a merge queue, which Elysium does not drive",
            ),
            Self::NoMergeMethod => {
                formatter.write_str("the repository and its rules leave no way to merge it")
            }
        }
    }
}

/// What a look does with `pull_request`, under `policy`, given the base branch's `rules` as
/// read, or `None` when they could not be.
pub fn decide(
    policy: MergePolicy,
    pull_request: &PullRequest,
    rules: Option<&BranchRules>,
) -> Verdict {
    match pull_request.status {
        PullRequestStatus::Merged => {
            return Verdict::Merged {
                by: pull_request.merged_by.clone(),
                at: pull_request.merged_at,
            };
        }
        PullRequestStatus::Closed => return Verdict::Closed,
        PullRequestStatus::Unknown => return Verdict::Hold(vec![Hold::StatusUnknown]),
        PullRequestStatus::Open => {}
    }
    if policy == MergePolicy::Person {
        return Verdict::PersonMerges;
    }

    let mut holds = Holds::default();
    hold_for_whose_and_how_it_merges(pull_request, &mut holds);
    hold_for_checks(pull_request, rules, &mut holds);
    hold_for_reviews(pull_request, &mut holds);
    let method = match rules {
        None => {
            holds.add(Hold::RulesUnread);
            None
        }
        Some(rules) => {
            if rules.merge_queue {
                holds.add(Hold::MergeQueue);
            }
            let method = merge_method(pull_request, rules);
            if method.is_none() {
                holds.add(Hold::NoMergeMethod);
            }
            method
        }
    };

    match method {
        Some(method) if holds.0.is_empty() => Verdict::Merge(method),
        _held => Verdict::Hold(holds.0),
    }
}

/// The reasons found so far, each once, in the order found.
#[derive(Debug, Default)]
struct Holds(Vec<Hold>);

impl Holds {
    fn add(&mut self, reason: Hold) {
        if !self.0.contains(&reason) {
            self.0.push(reason);
        }
    }
}

/// Holds it unless the project's account opened it from the repository itself, as no draft, and
/// GitHub says it merges cleanly with nothing the base branch requires missing.
fn hold_for_whose_and_how_it_merges(pull_request: &PullRequest, holds: &mut Holds) {
    let opened_by_account = pull_request
        .author
        .as_deref()
        .is_some_and(|author| author.eq_ignore_ascii_case(&pull_request.viewer));
    if !opened_by_account {
        holds.add(Hold::NotTheProjectsAccount {
            author: pull_request.author.clone(),
            account: pull_request.viewer.clone(),
        });
    }
    if pull_request.is_cross_repository {
        holds.add(Hold::FromAFork);
    }
    if pull_request.is_draft {
        holds.add(Hold::Draft);
    }
    match pull_request.mergeable {
        Mergeability::Mergeable => {}
        Mergeability::Conflicting => holds.add(Hold::Conflicts),
        Mergeability::Unknown => holds.add(Hold::MergeabilityUnknown),
    }
    match pull_request.merge_state {
        MergeState::Clean => {}
        MergeState::Behind => holds.add(Hold::Behind),
        MergeState::Blocked => holds.add(Hold::Blocked),
        MergeState::Dirty => holds.add(Hold::Conflicts),
        MergeState::Draft => holds.add(Hold::Draft),
        MergeState::Unstable => holds.add(Hold::Unstable),
        MergeState::HasHooks | MergeState::Unknown => holds.add(Hold::MergeStateUnknown),
    }
}

/// Holds it unless its head commit has checks, all read and all passed, among them every check
/// the base branch's classic protection and `rules` require.
fn hold_for_checks(pull_request: &PullRequest, rules: Option<&BranchRules>, holds: &mut Holds) {
    let checks = pull_request
        .checks
        .as_ref()
        .map_or(&[][..], |checks| checks.checks.as_slice());
    if checks.is_empty() {
        holds.add(Hold::NoChecks);
    }
    if pull_request
        .checks
        .as_ref()
        .is_some_and(|checks| checks.truncated)
    {
        holds.add(Hold::TooManyChecks);
    }
    let mut pending = Vec::new();
    let mut failed = Vec::new();
    for check in checks {
        match check.outcome {
            CheckOutcome::Passed => {}
            CheckOutcome::Pending => pending.push(check.name.clone()),
            CheckOutcome::Failed => failed.push(check.name.clone()),
        }
    }
    if !pending.is_empty() {
        holds.add(Hold::ChecksPending(pending));
    }
    if !failed.is_empty() {
        holds.add(Hold::ChecksFailed(failed));
    }

    // A required check that reported is already held for above unless it passed; one that never
    // reported is held for here.
    let mut missing: Vec<String> = Vec::new();
    let required = pull_request
        .classic_required_checks
        .iter()
        .chain(rules.iter().flat_map(|rules| rules.required_checks.iter()));
    for name in required {
        let reported = checks.iter().any(|check| check.name == *name);
        if !reported && !missing.contains(name) {
            missing.push(name.clone());
        }
    }
    if !missing.is_empty() {
        holds.add(Hold::RequiredChecksMissing(missing));
    }
}

/// Holds it while a reviewer asks for changes, a required review is owed, a review thread is
/// open, or some reviews or threads were not read.
fn hold_for_reviews(pull_request: &PullRequest, holds: &mut Holds) {
    match pull_request.review_decision {
        None | Some(ReviewDecision::Approved) => {}
        Some(ReviewDecision::ChangesRequested) => holds.add(Hold::ChangesRequested),
        Some(ReviewDecision::ReviewRequired) => holds.add(Hold::ReviewRequired),
        Some(ReviewDecision::Unknown) => holds.add(Hold::ReviewDecisionUnknown),
    }
    if pull_request.changes_requested > 0 {
        holds.add(Hold::ChangesRequested);
    }
    if pull_request.reviews_truncated {
        holds.add(Hold::TooManyReviews);
    }
    if pull_request.threads_read < pull_request.threads_total {
        holds.add(Hold::TooManyThreads);
    }
    if pull_request.threads_open > 0 {
        holds.add(Hold::OpenThreads(pull_request.threads_open));
    }
}

/// The first way to merge, in [`METHOD_PREFERENCE`], that the repository allows, its rules allow,
/// and a linear history the branch requires does not rule out.
fn merge_method(pull_request: &PullRequest, rules: &BranchRules) -> Option<MergeMethod> {
    let linear = rules.linear_history || pull_request.classic_linear_history;
    METHOD_PREFERENCE.into_iter().find(|method| {
        let repository_allows = match method {
            MergeMethod::Squash => pull_request.squash_allowed,
            MergeMethod::Merge => pull_request.merge_commit_allowed && !linear,
            MergeMethod::Rebase => pull_request.rebase_allowed,
        };
        let rules_allow = rules
            .allowed_methods
            .as_ref()
            .is_none_or(|allowed| allowed.contains(method));
        repository_allows && rules_allow
    })
}

/// The reasons a look left a pull request open, as one sentence for the worker and the batch's
/// page.
pub fn describe(holds: &[Hold]) -> String {
    let reasons: Vec<String> = holds.iter().map(ToString::to_string).collect();
    let mut sentence = reasons.join("; ");
    if let Some(first) = sentence.get(..1) {
        sentence.replace_range(..1, &first.to_uppercase());
    }
    sentence.push('.');
    sentence
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::github::pulls::{Check, Checks};

    /// An open pull request nothing holds: the project's account opened it, it merges cleanly,
    /// its one check passed, and nobody asked for anything.
    fn ready() -> PullRequest {
        PullRequest {
            viewer: "elysium-bot".to_owned(),
            repository: "JalapenoLabs/web".to_owned(),
            number: 12,
            url: "https://github.com/JalapenoLabs/web/pull/12".to_owned(),
            title: "Fix the login page".to_owned(),
            status: PullRequestStatus::Open,
            is_draft: false,
            merged_at: None,
            merged_by: None,
            author: Some("Elysium-Bot".to_owned()),
            created_at: Utc.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap(),
            is_cross_repository: false,
            head_oid: "abc123".to_owned(),
            base_branch: "main".to_owned(),
            mergeable: Mergeability::Mergeable,
            merge_state: MergeState::Clean,
            review_decision: None,
            changes_requested: 0,
            reviews_truncated: false,
            threads_total: 1,
            threads_read: 1,
            threads_open: 0,
            checks: Some(Checks {
                checks: vec![check("test", CheckOutcome::Passed)],
                truncated: false,
            }),
            classic_required_checks: Vec::new(),
            classic_linear_history: false,
            squash_allowed: true,
            merge_commit_allowed: true,
            rebase_allowed: true,
        }
    }

    fn check(name: &str, outcome: CheckOutcome) -> Check {
        Check {
            name: name.to_owned(),
            outcome,
        }
    }

    fn holds(pull_request: &PullRequest, rules: Option<&BranchRules>) -> Vec<Hold> {
        match decide(MergePolicy::Auto, pull_request, rules) {
            Verdict::Hold(holds) => holds,
            other => panic!("expected a hold, got {other:?}"),
        }
    }

    #[test]
    fn a_ready_pull_request_merges_by_squashing_first() {
        let rules = BranchRules::default();
        assert_eq!(
            decide(MergePolicy::Auto, &ready(), Some(&rules)),
            Verdict::Merge(MergeMethod::Squash)
        );
    }

    #[test]
    fn a_person_merges_unless_the_repository_lets_elysium() {
        let rules = BranchRules::default();
        assert_eq!(
            decide(MergePolicy::Person, &ready(), Some(&rules)),
            Verdict::PersonMerges
        );
    }

    #[test]
    fn a_merge_or_close_is_reported_whoever_made_it() {
        let rules = BranchRules::default();
        let mut merged = ready();
        merged.status = PullRequestStatus::Merged;
        merged.merged_by = Some("alex".to_owned());
        let at = Utc.with_ymd_and_hms(2026, 11, 4, 13, 0, 0).unwrap();
        merged.merged_at = Some(at);
        for policy in [MergePolicy::Person, MergePolicy::Auto] {
            assert_eq!(
                decide(policy, &merged, Some(&rules)),
                Verdict::Merged {
                    by: Some("alex".to_owned()),
                    at: Some(at),
                }
            );
        }

        let mut closed = ready();
        closed.status = PullRequestStatus::Closed;
        assert_eq!(decide(MergePolicy::Auto, &closed, None), Verdict::Closed);

        let mut strange = ready();
        strange.status = PullRequestStatus::Unknown;
        assert_eq!(holds(&strange, Some(&rules)), [Hold::StatusUnknown]);
    }

    #[test]
    fn only_the_projects_own_pull_request_from_its_own_repository_merges() {
        let rules = BranchRules::default();
        let mut someone_elses = ready();
        someone_elses.author = Some("octocat".to_owned());
        someone_elses.is_cross_repository = true;
        assert_eq!(
            holds(&someone_elses, Some(&rules)),
            [
                Hold::NotTheProjectsAccount {
                    author: Some("octocat".to_owned()),
                    account: "elysium-bot".to_owned(),
                },
                Hold::FromAFork,
            ]
        );

        let mut ghost = ready();
        ghost.author = None;
        assert!(matches!(
            holds(&ghost, Some(&rules)).as_slice(),
            [Hold::NotTheProjectsAccount { author: None, .. }]
        ));
    }

    #[test]
    fn unknown_ci_no_ci_and_failing_or_pending_checks_never_merge() {
        let rules = BranchRules::default();
        let mut no_ci = ready();
        no_ci.checks = None;
        assert_eq!(holds(&no_ci, Some(&rules)), [Hold::NoChecks]);

        let mut empty = ready();
        empty.checks = Some(Checks {
            checks: Vec::new(),
            truncated: false,
        });
        assert_eq!(holds(&empty, Some(&rules)), [Hold::NoChecks]);

        let mut mixed = ready();
        mixed.merge_state = MergeState::Unstable;
        mixed.checks = Some(Checks {
            checks: vec![
                check("test", CheckOutcome::Passed),
                check("lint", CheckOutcome::Pending),
                check("build", CheckOutcome::Failed),
            ],
            truncated: true,
        });
        assert_eq!(
            holds(&mixed, Some(&rules)),
            [
                Hold::Unstable,
                Hold::TooManyChecks,
                Hold::ChecksPending(vec!["lint".to_owned()]),
                Hold::ChecksFailed(vec!["build".to_owned()]),
            ]
        );
    }

    #[test]
    fn a_required_check_that_never_reported_holds_it() {
        let rules = BranchRules {
            required_checks: vec!["test".to_owned(), "e2e".to_owned()],
            ..BranchRules::default()
        };
        let mut classic = ready();
        classic.classic_required_checks = vec!["e2e".to_owned(), "security".to_owned()];
        assert_eq!(
            holds(&classic, Some(&rules)),
            [Hold::RequiredChecksMissing(vec![
                "e2e".to_owned(),
                "security".to_owned()
            ])]
        );
    }

    #[test]
    fn open_threads_requested_changes_and_unread_reviews_never_merge() {
        let rules = BranchRules::default();
        let mut reviewed = ready();
        reviewed.review_decision = Some(ReviewDecision::ChangesRequested);
        reviewed.changes_requested = 1;
        reviewed.threads_total = 3;
        reviewed.threads_read = 3;
        reviewed.threads_open = 2;
        assert_eq!(
            holds(&reviewed, Some(&rules)),
            [Hold::ChangesRequested, Hold::OpenThreads(2)]
        );

        // Requested changes count even where no rule asks for reviews.
        let mut unrequired = ready();
        unrequired.changes_requested = 1;
        assert_eq!(holds(&unrequired, Some(&rules)), [Hold::ChangesRequested]);

        let mut unread = ready();
        unread.review_decision = Some(ReviewDecision::Unknown);
        unread.reviews_truncated = true;
        unread.threads_total = 140;
        unread.threads_read = 100;
        assert_eq!(
            holds(&unread, Some(&rules)),
            [
                Hold::ReviewDecisionUnknown,
                Hold::TooManyReviews,
                Hold::TooManyThreads,
            ]
        );

        let mut owed = ready();
        owed.review_decision = Some(ReviewDecision::ReviewRequired);
        owed.merge_state = MergeState::Blocked;
        assert_eq!(
            holds(&owed, Some(&rules)),
            [Hold::Blocked, Hold::ReviewRequired]
        );
    }

    #[test]
    fn whatever_github_has_not_worked_out_holds_it() {
        let rules = BranchRules::default();
        let mut unknown = ready();
        unknown.mergeable = Mergeability::Unknown;
        unknown.merge_state = MergeState::Unknown;
        assert_eq!(
            holds(&unknown, Some(&rules)),
            [Hold::MergeabilityUnknown, Hold::MergeStateUnknown]
        );

        let mut conflicting = ready();
        conflicting.mergeable = Mergeability::Conflicting;
        conflicting.merge_state = MergeState::Dirty;
        conflicting.is_draft = true;
        assert_eq!(
            holds(&conflicting, Some(&rules)),
            [Hold::Draft, Hold::Conflicts]
        );

        let mut behind = ready();
        behind.merge_state = MergeState::Behind;
        assert_eq!(holds(&behind, Some(&rules)), [Hold::Behind]);

        assert_eq!(holds(&ready(), None), [Hold::RulesUnread]);
    }

    #[test]
    fn the_merge_follows_the_repository_and_every_rule_or_does_not_happen() {
        let mut merge_only = ready();
        merge_only.squash_allowed = false;
        let rules = BranchRules::default();
        assert_eq!(
            decide(MergePolicy::Auto, &merge_only, Some(&rules)),
            Verdict::Merge(MergeMethod::Merge)
        );

        // A linear history rules out a merge commit; the rules allow only rebasing then.
        let linear = BranchRules {
            linear_history: true,
            allowed_methods: Some(vec![MergeMethod::Merge, MergeMethod::Rebase]),
            ..BranchRules::default()
        };
        assert_eq!(
            decide(MergePolicy::Auto, &merge_only, Some(&linear)),
            Verdict::Merge(MergeMethod::Rebase)
        );

        let mut nothing = merge_only.clone();
        nothing.rebase_allowed = false;
        assert_eq!(holds(&nothing, Some(&linear)), [Hold::NoMergeMethod]);

        let queued = BranchRules {
            merge_queue: true,
            ..BranchRules::default()
        };
        assert_eq!(holds(&ready(), Some(&queued)), [Hold::MergeQueue]);
    }

    #[test]
    fn every_reason_reads_as_one_sentence() {
        assert_eq!(
            describe(&[
                Hold::OpenThreads(1),
                Hold::ChecksPending(vec!["test".to_owned()])
            ]),
            "1 review thread is not resolved; these checks are still running: test."
        );
    }
}
