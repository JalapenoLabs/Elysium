// Copyright © 2026 Jalapeno Labs

//! Pull requests as Elysium merges them: everything one look needs to decide, the rules GitHub
//! enforces on the base branch, and the merge itself. See `docs/github.md`, Merging.
//!
//! What a pull request is (its state, its author, whether it merges cleanly, its review threads,
//! its reviews, and its head commit's checks) is read in one GraphQL call ([`Github::pull_request`])
//! because review threads, and whether each is resolved, exist only in GraphQL. The rulesets that
//! govern its base branch, which GraphQL shows a token without admin rights only in part, are
//! read from REST ([`Github::branch_rules`]). Both go to the fixed `api.github.com`.
//!
//! Nothing here decides anything. Every field is read as GitHub wrote it, with what GitHub did
//! not answer, or answered in a shape this build does not know, kept as unknown, so the decision
//! (`crate::worker_pool::merging`) can refuse it: a merge fails closed.

use chrono::{DateTime, Utc};
use reqwest::{Method, StatusCode};
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::json;

use super::issues::IssueRef;
use super::{Github, GithubError, Repository};
use crate::models::work_batch::pull_request::MergeMethod;

/// The most review threads, reviews, and checks one read takes of each. GraphQL pages at most
/// a hundred; a pull request with more is one a person merges, since what was not read cannot
/// be said to pass.
const PAGE: u32 = 100;

/// One read of a pull request: its repository's merge settings, the pull request, its reviews
/// and threads, its base branch's classic protection as a non-admin sees it, and its head
/// commit's checks. Field names were checked against a live answer.
const PULL_REQUEST_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $name) {
    nameWithOwner
    squashMergeAllowed
    mergeCommitAllowed
    rebaseMergeAllowed
    pullRequest(number: $number) {
      number url title state isDraft mergedAt
      mergedBy { login }
      author { login }
      createdAt isCrossRepository headRefOid baseRefName
      mergeable mergeStateStatus reviewDecision
      baseRef { refUpdateRule { requiredStatusCheckContexts requiresLinearHistory } }
      latestOpinionatedReviews(first: 100) { nodes { state } pageInfo { hasNextPage } }
      reviewThreads(first: 100) { totalCount nodes { isResolved } }
      commits(last: 1) { nodes { commit { oid statusCheckRollup { state
        contexts(first: 100) { pageInfo { hasNextPage } nodes {
          __typename
          ... on CheckRun { name status conclusion }
          ... on StatusContext { context state }
        } }
      } } } }
    }
  }
}";

/// Where a pull request stands on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PullRequestStatus {
    Open,
    Closed,
    Merged,
    #[serde(other)]
    Unknown,
}

/// Whether GitHub can merge a pull request's head into its base without conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mergeability {
    Mergeable,
    Conflicting,
    /// GitHub has not worked it out yet, as it does in the background after a push.
    #[serde(other)]
    Unknown,
}

/// GitHub's own summary of whether a pull request may merge now (`mergeStateStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeState {
    /// Mergeable, and every commit status passes.
    Clean,
    /// Its head is behind its base, which the branch's rules require it not to be.
    Behind,
    /// A rule of the base branch blocks it: a required review, check, or the like.
    Blocked,
    /// It conflicts with its base.
    Dirty,
    Draft,
    /// Mergeable with passing checks and pre-receive hooks, which only GitHub Enterprise Server
    /// has.
    HasHooks,
    /// Mergeable, but a check that is not required is failing.
    Unstable,
    #[serde(other)]
    Unknown,
}

/// What a pull request's reviews add up to, when its base branch asks for reviews.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
    #[serde(other)]
    Unknown,
}

/// How one check on the head commit ended, or that it has not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    /// Succeeded, or ended neutral or skipped, which GitHub counts as passing.
    Passed,
    /// Queued, running, or expected and not reported yet.
    Pending,
    /// Failed, errored, timed out, was cancelled, or needs someone's action.
    Failed,
}

/// One check on the head commit: a check run, or a commit status, by the name rules call it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub outcome: CheckOutcome,
}

/// The checks on a pull request's head commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checks {
    pub checks: Vec<Check>,
    /// Whether there were more than one read takes, so some were not read.
    pub truncated: bool,
}

/// A pull request as one read found it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each is one yes-or-no fact GitHub answers, read independently"
)]
pub struct PullRequest {
    /// The account the token acts as.
    pub viewer: String,
    /// `owner/name`, as GitHub spells it.
    pub repository: String,
    pub number: u64,
    pub url: String,
    pub title: String,
    pub status: PullRequestStatus,
    pub is_draft: bool,
    pub merged_at: Option<DateTime<Utc>>,
    /// Who merged it; `None` while open, or for an account GitHub no longer has.
    pub merged_by: Option<String>,
    /// Who opened it; `None` for an account GitHub no longer has.
    pub author: Option<String>,
    pub created_at: DateTime<Utc>,
    /// Whether its head is on a fork rather than the repository itself.
    pub is_cross_repository: bool,
    /// The commit at its head, which a merge names so a later push cannot slip in unread.
    pub head_oid: String,
    pub base_branch: String,
    pub mergeable: Mergeability,
    pub merge_state: MergeState,
    /// `None` when its base branch asks for no review.
    pub review_decision: Option<ReviewDecision>,
    /// How many of its reviewers' latest reviews ask for changes.
    pub changes_requested: usize,
    /// Whether some reviews were not read.
    pub reviews_truncated: bool,
    /// How many review threads it has, and how many of those read are not resolved.
    pub threads_total: usize,
    pub threads_read: usize,
    pub threads_open: usize,
    /// `None` when no check of any kind ever reported on its head commit.
    pub checks: Option<Checks>,
    /// The checks the base branch's classic protection requires, as a non-admin reads them.
    pub classic_required_checks: Vec<String>,
    /// Whether the base branch's classic protection requires a linear history.
    pub classic_linear_history: bool,
    /// The repository's own merge settings.
    pub squash_allowed: bool,
    pub merge_commit_allowed: bool,
    pub rebase_allowed: bool,
}

/// The rules GitHub enforces on a branch through its rulesets, as anyone who can read the
/// repository sees them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchRules {
    /// Checks that must pass, by name.
    pub required_checks: Vec<String>,
    /// How a pull request may merge, when a rule narrows it; `None` when none does.
    pub allowed_methods: Option<Vec<MergeMethod>>,
    pub linear_history: bool,
    /// Whether pull requests merge only through a merge queue.
    pub merge_queue: bool,
}

impl Github {
    /// Pull request `pull_request` as it stands now, or `None` when the token cannot see it.
    ///
    /// # Errors
    /// Returns [`GithubError::Unauthorized`] when GitHub rejects the token, and
    /// [`GithubError::Refused`] when it cannot be reached, answers with errors, or answers in a
    /// shape that cannot be read.
    pub async fn pull_request(
        &self,
        token: &SecretString,
        pull_request: &IssueRef,
    ) -> Result<Option<PullRequest>, GithubError> {
        let number = i64::try_from(pull_request.number).map_err(|_too_large| {
            GithubError::Refused("a pull request number GitHub cannot hold".to_owned())
        })?;
        let body = json!({
            "query": PULL_REQUEST_QUERY,
            "variables": {
                "owner": pull_request.repository.owner,
                "name": pull_request.repository.name,
                "number": number,
            },
        });
        let reply = self
            .send(Method::POST, &self.url("/graphql"), token, Some(&body))
            .await?;
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }
        let answer: GraphqlAnswer = reply.read("a pull request")?;
        if let Some(error) = answer.errors.as_ref().and_then(|errors| errors.first()) {
            // A repository or pull request the token cannot see is an error of type NOT_FOUND
            // beside a null in `data`, which is "cannot see", not a failure to read.
            if error.kind.as_deref() == Some("NOT_FOUND") {
                return Ok(None);
            }
            return Err(GithubError::Refused(format!("GitHub: {}", error.message)));
        }
        let Some(data) = answer.data else {
            return Err(GithubError::Refused(
                "GitHub answered a pull request with no data".to_owned(),
            ));
        };
        let Some(repository) = data.repository else {
            return Ok(None);
        };
        let Some(found) = repository.pull_request else {
            return Ok(None);
        };
        Ok(Some(read_pull_request(
            data.viewer.login,
            repository.name_with_owner,
            RepositorySettings {
                squash: repository.squash_merge_allowed,
                merge_commit: repository.merge_commit_allowed,
                rebase: repository.rebase_merge_allowed,
            },
            found,
        )))
    }

    /// The rules `repository`'s rulesets enforce on `branch`.
    ///
    /// # Errors
    /// As [`Github::pull_request`], and [`GithubError::Refused`] for a branch with more rules
    /// than one page, since a rule not read could be one that forbids the merge.
    pub async fn branch_rules(
        &self,
        token: &SecretString,
        repository: &Repository,
        branch: &str,
    ) -> Result<BranchRules, GithubError> {
        let url = format!(
            "{}?per_page={PAGE}",
            self.repository_url(repository, &["rules", "branches", branch])
        );
        let reply = self.get(&url, token).await?;
        if !reply.status.is_success() {
            return Err(reply.refusal());
        }
        if reply.next_page.is_some() {
            return Err(GithubError::Refused(format!(
                "the branch {branch} has more than {PAGE} rules, more than Elysium reads"
            )));
        }
        let rules: Vec<RuleBody> = reply.read("branch rules")?;
        Ok(read_rules(rules))
    }

    /// Merges pull request `pull_request` with `method`, provided its head is still `head_oid`,
    /// and answers the commit it merged as. GitHub enforces the base branch's rules on the call
    /// itself; nothing here asks it to bypass them.
    ///
    /// # Errors
    /// Returns [`GithubError::Refused`] with GitHub's message when it refuses: `405` for a pull
    /// request its rules keep from merging, `409` when its head moved, `422` for a method the
    /// repository does not allow; [`GithubError::NoAccess`] for a token that may not merge.
    pub async fn merge_pull_request(
        &self,
        token: &SecretString,
        pull_request: &IssueRef,
        method: MergeMethod,
        head_oid: &str,
    ) -> Result<String, GithubError> {
        let url = self.repository_url(
            &pull_request.repository,
            &["pulls", &pull_request.number.to_string(), "merge"],
        );
        let body = json!({ "merge_method": method.as_str(), "sha": head_oid });
        let reply = self.send(Method::PUT, &url, token, Some(&body)).await?;
        if reply.status != StatusCode::OK {
            return Err(reply.write_refusal());
        }
        let merged: MergedBody = reply.read("a merge")?;
        if !merged.merged {
            return Err(GithubError::Refused(format!("GitHub: {}", merged.message)));
        }
        Ok(merged.sha)
    }
}

/// A repository's own merge settings.
#[derive(Clone, Copy)]
struct RepositorySettings {
    squash: bool,
    merge_commit: bool,
    rebase: bool,
}

/// A pull request's GraphQL answer as Elysium reads it.
fn read_pull_request(
    viewer: String,
    repository: String,
    settings: RepositorySettings,
    body: PullRequestBody,
) -> PullRequest {
    let threads_open = body
        .review_threads
        .nodes
        .iter()
        .filter(|thread| !thread.is_resolved)
        .count();
    let changes_requested = body
        .latest_opinionated_reviews
        .nodes
        .iter()
        .filter(|review| review.state == "CHANGES_REQUESTED")
        .count();
    let rule = body.base_ref.and_then(|base| base.ref_update_rule);
    let head = body
        .commits
        .nodes
        .into_iter()
        .next()
        .map(|node| node.commit);
    let checks = head
        .as_ref()
        .and_then(|commit| commit.status_check_rollup.as_ref())
        .map(|rollup| Checks {
            checks: rollup.contexts.nodes.iter().map(read_check).collect(),
            truncated: rollup.contexts.page_info.has_next_page,
        });
    PullRequest {
        viewer,
        repository,
        number: body.number,
        url: body.url,
        title: body.title,
        status: body.state,
        is_draft: body.is_draft,
        merged_at: body.merged_at,
        merged_by: body.merged_by.map(|account| account.login),
        author: body.author.map(|account| account.login),
        created_at: body.created_at,
        is_cross_repository: body.is_cross_repository,
        head_oid: body.head_ref_oid,
        base_branch: body.base_ref_name,
        mergeable: body.mergeable,
        merge_state: body.merge_state_status,
        review_decision: body.review_decision,
        changes_requested,
        reviews_truncated: body.latest_opinionated_reviews.page_info.has_next_page,
        threads_total: body.review_threads.total_count,
        threads_read: body.review_threads.nodes.len(),
        threads_open,
        checks,
        classic_required_checks: rule
            .as_ref()
            .map(|rule| rule.required_status_check_contexts.clone())
            .unwrap_or_default(),
        classic_linear_history: rule.is_some_and(|rule| rule.requires_linear_history),
        squash_allowed: settings.squash,
        merge_commit_allowed: settings.merge_commit,
        rebase_allowed: settings.rebase,
    }
}

/// One check as its rule names it, and how it ended. A shape this build does not know reads as
/// failed, so it can never count as passing.
fn read_check(node: &CheckBody) -> Check {
    match node {
        CheckBody::CheckRun {
            name,
            status,
            conclusion,
        } => {
            let outcome = if status != "COMPLETED" {
                CheckOutcome::Pending
            } else if matches!(
                conclusion.as_deref(),
                Some("SUCCESS" | "NEUTRAL" | "SKIPPED")
            ) {
                CheckOutcome::Passed
            } else {
                CheckOutcome::Failed
            };
            Check {
                name: name.clone(),
                outcome,
            }
        }
        CheckBody::StatusContext { context, state } => Check {
            name: context.clone(),
            outcome: match state.as_str() {
                "SUCCESS" => CheckOutcome::Passed,
                "PENDING" | "EXPECTED" => CheckOutcome::Pending,
                _failed => CheckOutcome::Failed,
            },
        },
        CheckBody::Unknown => Check {
            name: "a check of a kind Elysium does not read".to_owned(),
            outcome: CheckOutcome::Failed,
        },
    }
}

/// What a branch's rules require, from every ruleset that reaches it.
fn read_rules(rules: Vec<RuleBody>) -> BranchRules {
    let mut read = BranchRules::default();
    for rule in rules {
        match rule.kind.as_str() {
            "required_status_checks" => {
                let checks = rule
                    .parameters
                    .required_status_checks
                    .into_iter()
                    .map(|check| check.context);
                read.required_checks.extend(checks);
            }
            "pull_request" => {
                if let Some(methods) = rule.parameters.allowed_merge_methods {
                    // Every ruleset applies, so a method must be allowed by each that names any.
                    read.allowed_methods = Some(match read.allowed_methods {
                        Some(earlier) => earlier
                            .into_iter()
                            .filter(|method| methods.contains(method))
                            .collect(),
                        None => methods,
                    });
                }
            }
            "required_linear_history" => read.linear_history = true,
            "merge_queue" => read.merge_queue = true,
            _other => {}
        }
    }
    read
}

#[derive(Deserialize)]
struct GraphqlAnswer {
    data: Option<DataBody>,
    errors: Option<Vec<GraphqlError>>,
}

#[derive(Deserialize)]
struct GraphqlError {
    message: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct DataBody {
    viewer: LoginBody,
    repository: Option<RepositoryBody>,
}

#[derive(Deserialize)]
struct LoginBody {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryBody {
    name_with_owner: String,
    squash_merge_allowed: bool,
    merge_commit_allowed: bool,
    rebase_merge_allowed: bool,
    pull_request: Option<PullRequestBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestBody {
    number: u64,
    url: String,
    title: String,
    state: PullRequestStatus,
    is_draft: bool,
    merged_at: Option<DateTime<Utc>>,
    merged_by: Option<LoginBody>,
    author: Option<LoginBody>,
    created_at: DateTime<Utc>,
    is_cross_repository: bool,
    head_ref_oid: String,
    base_ref_name: String,
    mergeable: Mergeability,
    merge_state_status: MergeState,
    review_decision: Option<ReviewDecision>,
    base_ref: Option<BaseRefBody>,
    latest_opinionated_reviews: ReviewsBody,
    review_threads: ThreadsBody,
    commits: CommitsBody,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BaseRefBody {
    ref_update_rule: Option<RefUpdateRuleBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RefUpdateRuleBody {
    #[serde(default)]
    required_status_check_contexts: Vec<String>,
    requires_linear_history: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewsBody {
    nodes: Vec<ReviewBody>,
    page_info: PageInfoBody,
}

#[derive(Deserialize)]
struct ReviewBody {
    state: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadsBody {
    total_count: usize,
    nodes: Vec<ThreadBody>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadBody {
    is_resolved: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfoBody {
    has_next_page: bool,
}

#[derive(Deserialize)]
struct CommitsBody {
    nodes: Vec<CommitNodeBody>,
}

#[derive(Deserialize)]
struct CommitNodeBody {
    commit: CommitBody,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitBody {
    status_check_rollup: Option<RollupBody>,
}

#[derive(Deserialize)]
struct RollupBody {
    contexts: ContextsBody,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextsBody {
    page_info: PageInfoBody,
    nodes: Vec<CheckBody>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum CheckBody {
    CheckRun {
        name: String,
        status: String,
        conclusion: Option<String>,
    },
    StatusContext {
        context: String,
        state: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct RuleBody {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    parameters: RuleParametersBody,
}

#[derive(Default, Deserialize)]
struct RuleParametersBody {
    #[serde(default)]
    required_status_checks: Vec<RequiredCheckBody>,
    allowed_merge_methods: Option<Vec<MergeMethod>>,
}

#[derive(Deserialize)]
struct RequiredCheckBody {
    context: String,
}

#[derive(Deserialize)]
struct MergedBody {
    merged: bool,
    #[serde(default)]
    sha: String,
    #[serde(default)]
    message: String,
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    /// GitHub's answer for JalapenoLabs/Elysium#63, read live, trimmed to three checks.
    fn answer() -> Value {
        // Built in parts, since one `json!` this deep passes the macro's recursion limit.
        let checks = json!([
            { "__typename": "CheckRun", "name": "API format",
              "status": "COMPLETED", "conclusion": "CANCELLED" },
            { "__typename": "CheckRun", "name": "API test",
              "status": "IN_PROGRESS", "conclusion": null },
            { "__typename": "StatusContext", "context": "deploy", "state": "SUCCESS" },
        ]);
        let commit = json!({
            "oid": "2e8f9b949cdd322b0565288d0161f92364263e1f",
            "statusCheckRollup": { "state": "FAILURE", "contexts": {
                "pageInfo": { "hasNextPage": false },
                "nodes": checks,
            } },
        });
        let pull_request = json!({
            "number": 63,
            "url": "https://github.com/JalapenoLabs/Elysium/pull/63",
            "title": "Give every browser a way out",
            "state": "MERGED",
            "isDraft": false,
            "mergedAt": "2026-11-02T22:26:13Z",
            "mergedBy": { "login": "navarrotech" },
            "author": { "login": "navarrotech" },
            "createdAt": "2026-11-02T22:22:02Z",
            "isCrossRepository": false,
            "headRefOid": "2e8f9b949cdd322b0565288d0161f92364263e1f",
            "baseRefName": "main",
            "mergeable": "UNKNOWN",
            "mergeStateStatus": "UNKNOWN",
            "reviewDecision": "REVIEW_REQUIRED",
            "baseRef": { "refUpdateRule": null },
            "latestOpinionatedReviews": {
                "nodes": [{ "state": "CHANGES_REQUESTED" }, { "state": "APPROVED" }],
                "pageInfo": { "hasNextPage": false },
            },
            "reviewThreads": {
                "totalCount": 2,
                "nodes": [{ "isResolved": true }, { "isResolved": false }],
            },
            "commits": { "nodes": [{ "commit": commit }] },
        });
        json!({ "data": {
            "viewer": { "login": "navarrotech" },
            "repository": {
                "nameWithOwner": "JalapenoLabs/Elysium",
                "squashMergeAllowed": true,
                "mergeCommitAllowed": false,
                "rebaseMergeAllowed": false,
                "pullRequest": pull_request,
            },
        } })
    }

    fn read(answer: &Value) -> PullRequest {
        let answer: GraphqlAnswer = serde_json::from_value(answer.clone()).expect("reads");
        let data = answer.data.expect("data");
        let repository = data.repository.expect("a repository");
        read_pull_request(
            data.viewer.login,
            repository.name_with_owner,
            RepositorySettings {
                squash: repository.squash_merge_allowed,
                merge_commit: repository.merge_commit_allowed,
                rebase: repository.rebase_merge_allowed,
            },
            repository.pull_request.expect("a pull request"),
        )
    }

    #[test]
    fn a_live_answer_reads_into_every_fact_a_merge_is_decided_on() {
        let read = read(&answer());
        assert_eq!(read.viewer, "navarrotech");
        assert_eq!(read.repository, "JalapenoLabs/Elysium");
        assert_eq!(read.status, PullRequestStatus::Merged);
        assert_eq!(read.merged_by.as_deref(), Some("navarrotech"));
        assert_eq!(read.mergeable, Mergeability::Unknown);
        assert_eq!(read.merge_state, MergeState::Unknown);
        assert_eq!(read.review_decision, Some(ReviewDecision::ReviewRequired));
        assert_eq!(read.changes_requested, 1);
        assert_eq!(
            (read.threads_total, read.threads_read, read.threads_open),
            (2, 2, 1)
        );
        assert!(read.squash_allowed && !read.merge_commit_allowed && !read.rebase_allowed);
        let checks = read.checks.expect("checks reported");
        let outcomes: Vec<(&str, CheckOutcome)> = checks
            .checks
            .iter()
            .map(|check| (check.name.as_str(), check.outcome))
            .collect();
        assert_eq!(
            outcomes,
            [
                ("API format", CheckOutcome::Failed),
                ("API test", CheckOutcome::Pending),
                ("deploy", CheckOutcome::Passed),
            ]
        );
    }

    #[test]
    fn what_github_did_not_answer_reads_as_unknown_never_as_passing() {
        let mut answer = answer();
        let pull_request = &mut answer["data"]["repository"]["pullRequest"];
        pull_request["mergeStateStatus"] = json!("SOMETHING_NEW");
        pull_request["reviewDecision"] = Value::Null;
        pull_request["commits"]["nodes"][0]["commit"]["statusCheckRollup"] = Value::Null;
        let read = read(&answer);
        assert_eq!(read.merge_state, MergeState::Unknown);
        assert_eq!(read.review_decision, None);
        assert_eq!(read.checks, None, "no check ever reported");

        let strange = read_check(&CheckBody::Unknown);
        assert_eq!(strange.outcome, CheckOutcome::Failed);
    }

    #[test]
    fn every_ruleset_narrows_the_merge_and_adds_its_checks() {
        let rules: Vec<RuleBody> = serde_json::from_value(json!([
            { "type": "deletion" },
            { "type": "pull_request", "parameters": {
                "required_approving_review_count": 1,
                "allowed_merge_methods": ["squash", "rebase"],
            } },
            { "type": "pull_request", "parameters": { "allowed_merge_methods": ["squash"] } },
            { "type": "required_status_checks", "parameters": {
                "required_status_checks": [{ "context": "API test" }, { "context": "lint" }],
                "strict_required_status_checks_policy": false,
            } },
            { "type": "required_linear_history" },
            { "type": "merge_queue", "parameters": { "merge_method": "SQUASH" } },
        ]))
        .expect("rules read");
        let read = read_rules(rules);
        assert_eq!(read.required_checks, ["API test", "lint"]);
        assert_eq!(read.allowed_methods, Some(vec![MergeMethod::Squash]));
        assert!(read.linear_history);
        assert!(read.merge_queue);

        assert_eq!(read_rules(Vec::new()), BranchRules::default());
    }
}
