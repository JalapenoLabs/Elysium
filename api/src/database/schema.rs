// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "action_item_owner_kind"))]
    pub struct ActionItemOwnerKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "action_item_priority"))]
    pub struct ActionItemPriority;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "action_item_state"))]
    pub struct ActionItemState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "agent_kind"))]
    pub struct AgentKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "browser_state"))]
    pub struct BrowserState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "bunny_storage_region"))]
    pub struct BunnyStorageRegion;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "changeset_decision"))]
    pub struct ChangesetDecision;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "changeset_outcome"))]
    pub struct ChangesetOutcome;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "changeset_state"))]
    pub struct ChangesetState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "container_kind"))]
    pub struct ContainerKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "database_backup_status"))]
    pub struct DatabaseBackupStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "database_backup_trigger"))]
    pub struct DatabaseBackupTrigger;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "deployment_update_status"))]
    pub struct DeploymentUpdateStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "draft_state"))]
    pub struct DraftState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "elysia_question_status"))]
    pub struct ElysiaQuestionStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "elysia_turn_status"))]
    pub struct ElysiaTurnStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "elysia_work_status"))]
    pub struct ElysiaWorkStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "github_access"))]
    pub struct GithubAccess;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "github_token_kind"))]
    pub struct GithubTokenKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "initiative_state"))]
    pub struct InitiativeState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "issue_report_filing"))]
    pub struct IssueReportFiling;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "knowledge_ask_state"))]
    pub struct KnowledgeAskState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "link_kind"))]
    pub struct LinkKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "link_provider"))]
    pub struct LinkProvider;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "link_state"))]
    pub struct LinkState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "link_write_kind"))]
    pub struct LinkWriteKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "llm_type"))]
    pub struct LlmType;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "mail_account_kind"))]
    pub struct MailAccountKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "merge_policy"))]
    pub struct MergePolicy;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "password_server"))]
    pub struct PasswordServer;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "project_cover_fit"))]
    pub struct ProjectCoverFit;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "project_manager_turn_status"))]
    pub struct ProjectManagerTurnStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "project_manager_wake_kind"))]
    pub struct ProjectManagerWakeKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "promise_state"))]
    pub struct PromiseState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "pull_request_merge_method"))]
    pub struct PullRequestMergeMethod;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "pull_request_state"))]
    pub struct PullRequestState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "reply_place"))]
    pub struct ReplyPlace;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "s3_service"))]
    pub struct S3Service;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "session_turn_status"))]
    pub struct SessionTurnStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "step_job_state"))]
    pub struct StepJobState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "storage_location_kind"))]
    pub struct StorageLocationKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "studio_asset_kind"))]
    pub struct StudioAssetKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "tsvector", schema = "pg_catalog"))]
    pub struct Tsvector;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "user_role"))]
    pub struct UserRole;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "user_status"))]
    pub struct UserStatus;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "vector"))]
    pub struct Vector;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "work_batch_origin"))]
    pub struct WorkBatchOrigin;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "work_batch_priority"))]
    pub struct WorkBatchPriority;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "work_batch_state"))]
    pub struct WorkBatchState;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "work_batch_wait"))]
    pub struct WorkBatchWait;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "workflow_run_state"))]
    pub struct WorkflowRunState;
}

diesel::table! {
    use diesel::sql_types::*;

    action_item_comments (id) {
        id -> Uuid,
        action_item_id -> Uuid,
        author -> Text,
        body -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    action_item_events (id) {
        id -> Uuid,
        action_item_id -> Nullable<Uuid>,
        initiative_id -> Nullable<Uuid>,
        kind -> Text,
        actor -> Text,
        data -> Jsonb,
        created_at -> Timestamptz,
        changeset_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::LinkProvider;

    action_item_issue_filings (id) {
        id -> Uuid,
        action_item_id -> Uuid,
        provider -> LinkProvider,
        jira_credential_id -> Nullable<Uuid>,
        github_credential_id -> Nullable<Uuid>,
        token_account_id -> Nullable<Uuid>,
        destination -> Text,
        filed_by -> Text,
        changeset_operation_id -> Nullable<Uuid>,
        timeline_record_id -> Nullable<Uuid>,
        attempts -> Int4,
        first_attempt_at -> Nullable<Timestamptz>,
        last_error -> Nullable<Text>,
        last_attempt_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::LinkWriteKind;

    action_item_link_writes (id) {
        id -> Uuid,
        link_id -> Uuid,
        kind -> LinkWriteKind,
        comment_id -> Nullable<Uuid>,
        attempts -> Int4,
        last_error -> Nullable<Text>,
        last_attempt_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::LinkProvider;
    use super::sql_types::LinkKind;
    use super::sql_types::LinkState;
    use super::sql_types::ActionItemOwnerKind;

    action_item_links (id) {
        id -> Uuid,
        action_item_id -> Uuid,
        provider -> LinkProvider,
        kind -> LinkKind,
        jira_credential_id -> Nullable<Uuid>,
        github_credential_id -> Nullable<Uuid>,
        external_id -> Text,
        external_key -> Text,
        url -> Text,
        title -> Text,
        is_primary -> Bool,
        observed_state -> LinkState,
        observed_owner_kind -> ActionItemOwnerKind,
        observed_owner_name -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        token_account_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    action_item_projects (action_item_id, project_id) {
        action_item_id -> Uuid,
        project_id -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ActionItemState;
    use super::sql_types::ActionItemPriority;
    use super::sql_types::ActionItemOwnerKind;

    action_items (id) {
        id -> Uuid,
        title -> Text,
        notes -> Text,
        state -> ActionItemState,
        priority -> ActionItemPriority,
        due_at -> Nullable<Timestamptz>,
        snoozed_until -> Nullable<Timestamptz>,
        waiting_on -> Nullable<Text>,
        owner_kind -> ActionItemOwnerKind,
        owner_name -> Nullable<Text>,
        resolved_at -> Nullable<Timestamptz>,
        dismissed_at -> Nullable<Timestamptz>,
        deleted_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    agent_configuration_status (singleton) {
        singleton -> Bool,
        repository -> Nullable<Text>,
        github_credential_id -> Nullable<Uuid>,
        commit_sha -> Nullable<Text>,
        commit_subject -> Nullable<Text>,
        fetched_at -> Nullable<Timestamptz>,
        skipped -> Jsonb,
        error -> Nullable<Text>,
        failed_at -> Nullable<Timestamptz>,
        inbox_item_id -> Nullable<Uuid>,
        created_by -> Uuid,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    agent_guidance_versions (version) {
        version -> Int4,
        content -> Text,
        restored_from -> Nullable<Int4>,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    agent_tool_grants (token_sha256) {
        token_sha256 -> Bytea,
        scope -> Jsonb,
        servers -> Array<Nullable<Text>>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        expires_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    assigned_issues (id) {
        id -> Uuid,
        connector_id -> Uuid,
        external_id -> Text,
        action_item_id -> Uuid,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    authenticator_nicknames (user_id) {
        user_id -> Uuid,
        nickname -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    authenticator_replacements (user_id) {
        user_id -> Uuid,
        secret_encrypted -> Bytea,
        expires_at -> Timestamptz,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::BrowserState;

    browser_sessions (id) {
        id -> Uuid,
        name -> Text,
        start_url -> Text,
        port -> Int4,
        member_password_encrypted -> Bytea,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        start_page_opened_at -> Nullable<Timestamptz>,
        state -> BrowserState,
        state_error -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ChangesetDecision;
    use super::sql_types::ChangesetOutcome;

    changeset_operations (id) {
        id -> Uuid,
        changeset_id -> Uuid,
        position -> Int4,
        operation -> Jsonb,
        reason -> Text,
        quote -> Nullable<Text>,
        source -> Nullable<Text>,
        decision -> ChangesetDecision,
        outcome -> ChangesetOutcome,
        error -> Nullable<Text>,
        result -> Jsonb,
        undo -> Nullable<Jsonb>,
        cause_record_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ChangesetState;

    changesets (id) {
        id -> Uuid,
        proposer -> Text,
        project_id -> Nullable<Uuid>,
        summary -> Text,
        state -> ChangesetState,
        decided_at -> Nullable<Timestamptz>,
        undone_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Uuid,
        decided_by -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    claude_accounts (llm_id) {
        llm_id -> Uuid,
        account_uuid -> Uuid,
        email -> Text,
        organization_uuid -> Nullable<Uuid>,
        organization_name -> Nullable<Text>,
        subscription_type -> Nullable<Text>,
        refresh_token_encrypted -> Bytea,
        access_token_expires_at -> Timestamptz,
        refresh_token_expires_at -> Nullable<Timestamptz>,
        sign_in_state -> Text,
        last_refreshed_at -> Nullable<Timestamptz>,
        last_refresh_error -> Nullable<Text>,
        five_hour_utilization -> Nullable<Float8>,
        five_hour_resets_at -> Nullable<Timestamptz>,
        seven_day_utilization -> Nullable<Float8>,
        seven_day_resets_at -> Nullable<Timestamptz>,
        usage_read_at -> Nullable<Timestamptz>,
        usage_error -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    coding_sessions (id) {
        id -> Int8,
        title -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        project_id -> Nullable<Uuid>,
        github_credential_id -> Nullable<Uuid>,
        action_item_id -> Nullable<Uuid>,
        created_by -> Uuid,
        studio_item_id -> Nullable<Uuid>,
        harness_session_id -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    company_doc_proposals (id) {
        id -> Uuid,
        section -> Text,
        base_content -> Text,
        content -> Text,
        reason -> Text,
        quote -> Nullable<Text>,
        source -> Nullable<Text>,
        timeline_record_id -> Nullable<Uuid>,
        workflow_run_id -> Nullable<Uuid>,
        state -> Text,
        decided_by -> Nullable<Uuid>,
        decided_at -> Nullable<Timestamptz>,
        created_by -> Uuid,
        agent -> Nullable<Text>,
        agent_name -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    company_doc_review_cursor (singleton) {
        singleton -> Bool,
        reviewed_through_at -> Nullable<Timestamptz>,
        reviewed_through_id -> Nullable<Uuid>,
        created_by -> Uuid,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    connector_check_results (connector_id, check_id) {
        connector_id -> Uuid,
        check_id -> Text,
        state -> Text,
        count -> Nullable<Int8>,
        reason -> Nullable<Text>,
        message -> Nullable<Text>,
        blocked_by -> Nullable<Text>,
        ran_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    connector_runs (id) {
        id -> Uuid,
        connector_id -> Uuid,
        started_at -> Timestamptz,
        finished_at -> Timestamptz,
        outcome -> Text,
        records -> Int4,
        error -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    connectors (id) {
        id -> Uuid,
        kind -> Text,
        browser_session_id -> Nullable<Uuid>,
        sign_in_state -> Text,
        cursor -> Jsonb,
        last_polled_at -> Nullable<Timestamptz>,
        last_error -> Nullable<Text>,
        is_active -> Bool,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        storage_location_id -> Nullable<Uuid>,
        token_account_id -> Nullable<Uuid>,
        jira_credential_id -> Nullable<Uuid>,
        github_credential_id -> Nullable<Uuid>,
        content -> Jsonb,
        syncing_since -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    conversation_messages (timeline_record_id) {
        timeline_record_id -> Uuid,
        conversation_id -> Uuid,
        wakes -> Bool,
        received_at -> Timestamptz,
        run_id -> Nullable<Uuid>,
        delivered_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    conversations (id) {
        id -> Uuid,
        connector_id -> Uuid,
        key -> Text,
        title -> Text,
        harness_session_id -> Nullable<Text>,
        waiting_until -> Nullable<Timestamptz>,
        waited_in_run_id -> Nullable<Uuid>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::DatabaseBackupTrigger;
    use super::sql_types::DatabaseBackupStatus;

    database_backups (id) {
        id -> Uuid,
        trigger -> DatabaseBackupTrigger,
        status -> DatabaseBackupStatus,
        storage_location_id -> Nullable<Uuid>,
        started_at -> Timestamptz,
        finished_at -> Nullable<Timestamptz>,
        size_bytes -> Nullable<Int8>,
        error -> Nullable<Text>,
        created_by -> Uuid,
        claimed_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::DeploymentUpdateStatus;

    deployment_updates (id) {
        id -> Uuid,
        from_version -> Text,
        to_version -> Text,
        status -> DeploymentUpdateStatus,
        reason -> Nullable<Text>,
        started_at -> Timestamptz,
        finished_at -> Nullable<Timestamptz>,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ReplyPlace;
    use super::sql_types::DraftState;

    drafts (id) {
        id -> Uuid,
        reply_to_record_id -> Nullable<Uuid>,
        connector_id -> Nullable<Uuid>,
        mail_account_id -> Nullable<Uuid>,
        target -> Jsonb,
        place -> Nullable<ReplyPlace>,
        body -> Text,
        state -> DraftState,
        last_error -> Nullable<Text>,
        attempted_at -> Nullable<Timestamptz>,
        sent_at -> Nullable<Timestamptz>,
        sent_record_id -> Nullable<Uuid>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        promise_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    elysia_messages (id) {
        id -> Uuid,
        body -> Text,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ElysiaQuestionStatus;

    elysia_questions (id) {
        id -> Uuid,
        prompt -> Text,
        options -> Jsonb,
        status -> ElysiaQuestionStatus,
        answer -> Nullable<Text>,
        action_item_id -> Nullable<Uuid>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        answered_by -> Nullable<Uuid>,
        answered_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    elysia_state (singleton) {
        singleton -> Bool,
        paused -> Bool,
        availability -> Jsonb,
        usage_paused_until -> Nullable<Timestamptz>,
        runner_session_id -> Nullable<Text>,
        updated_by -> Uuid,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ElysiaTurnStatus;

    elysia_turns (id) {
        id -> Uuid,
        trigger_kind -> Text,
        trigger_detail -> Jsonb,
        status -> ElysiaTurnStatus,
        summary -> Nullable<Text>,
        error -> Nullable<Text>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        finished_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ElysiaWorkStatus;

    elysia_work (id) {
        id -> Uuid,
        title -> Text,
        detail -> Text,
        status -> ElysiaWorkStatus,
        ready_at -> Nullable<Timestamptz>,
        action_item_id -> Nullable<Uuid>,
        revisits -> Int4,
        revisited_at -> Nullable<Timestamptz>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    environment_variables (id) {
        id -> Uuid,
        key -> Text,
        value_encrypted -> Bytea,
        is_secret -> Bool,
        description -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::GithubTokenKind;

    github_credentials (id) {
        id -> Uuid,
        name -> Text,
        kind -> GithubTokenKind,
        token_encrypted -> Bytea,
        login -> Text,
        scopes -> Text,
        token_expires_at -> Nullable<Timestamptz>,
        checked_at -> Timestamptz,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        is_default -> Bool,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    initiative_items (id) {
        id -> Uuid,
        initiative_id -> Uuid,
        action_item_id -> Uuid,
        joined_at -> Timestamptz,
        left_at -> Nullable<Timestamptz>,
        via_link_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::LinkProvider;
    use super::sql_types::ContainerKind;

    initiative_links (id) {
        id -> Uuid,
        initiative_id -> Uuid,
        provider -> LinkProvider,
        kind -> ContainerKind,
        jira_credential_id -> Nullable<Uuid>,
        github_credential_id -> Nullable<Uuid>,
        external_id -> Text,
        external_key -> Text,
        url -> Text,
        title -> Text,
        synced_at -> Nullable<Timestamptz>,
        sync_error -> Nullable<Text>,
        truncated -> Bool,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    initiative_projects (initiative_id, project_id) {
        initiative_id -> Uuid,
        project_id -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::InitiativeState;

    initiatives (id) {
        id -> Uuid,
        name -> Text,
        description -> Text,
        target_at -> Nullable<Timestamptz>,
        state -> InitiativeState,
        deleted_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::IssueReportFiling;

    issue_reports (id) {
        id -> Uuid,
        fingerprint -> Text,
        issue_number -> Int8,
        issue_url -> Text,
        filed_as -> IssueReportFiling,
        url -> Text,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    jira_credential_boards (jira_credential_id, board_id) {
        jira_credential_id -> Uuid,
        board_id -> Int8,
        name -> Text,
        project_key -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    jira_credential_projects (jira_credential_id, project_id) {
        jira_credential_id -> Uuid,
        project_id -> Text,
        project_key -> Text,
        name -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    jira_credentials (id) {
        id -> Uuid,
        name -> Text,
        site_url -> Text,
        account_email -> Text,
        token_encrypted -> Bytea,
        account_id -> Text,
        display_name -> Text,
        all_projects -> Bool,
        all_boards -> Bool,
        checked_at -> Timestamptz,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    jira_done_transitions (jira_credential_id, project_key) {
        jira_credential_id -> Uuid,
        project_key -> Text,
        status_id -> Text,
        status_name -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::AgentKind;

    kind_model_settings (id) {
        id -> Uuid,
        project_id -> Nullable<Uuid>,
        kind -> AgentKind,
        model -> Text,
        effort -> Text,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::KnowledgeAskState;

    knowledge_asks (id) {
        id -> Uuid,
        created_by -> Uuid,
        question -> Text,
        filters -> Jsonb,
        passages -> Jsonb,
        searched_by_meaning -> Bool,
        state -> KnowledgeAskState,
        answer -> Nullable<Jsonb>,
        error -> Nullable<Text>,
        created_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        finished_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    knowledge_backfills (source) {
        source -> Text,
        version -> Nullable<Int4>,
        through_id -> Nullable<Uuid>,
        finished_at -> Nullable<Timestamptz>,
        created_by -> Uuid,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    knowledge_documents (id) {
        id -> Uuid,
        source -> Text,
        timeline_record_id -> Nullable<Uuid>,
        title -> Text,
        url -> Nullable<Text>,
        occurred_at -> Timestamptz,
        content_sha256 -> Text,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        page_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::Tsvector;
    use super::sql_types::Vector;

    knowledge_passages (document_id, position) {
        document_id -> Uuid,
        position -> Int4,
        content -> Text,
        search_vector -> Tsvector,
        embedding -> Nullable<Vector>,
        embedding_model -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    learning_categories (key) {
        key -> Text,
        label -> Text,
        description -> Text,
        position -> Int4,
        built_in -> Bool,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    link_watch_cursors (id) {
        id -> Uuid,
        jira_credential_id -> Nullable<Uuid>,
        github_credential_id -> Nullable<Uuid>,
        watched_through -> Timestamptz,
        last_error -> Nullable<Text>,
        updated_at -> Timestamptz,
        token_account_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::LlmType;

    llms (id) {
        id -> Uuid,
        name -> Text,
        description -> Text,
        #[sql_name = "type"]
        type_ -> LlmType,
        secret_token_encrypted -> Bytea,
        priority -> Int4,
        is_active -> Bool,
        expires_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::MailAccountKind;

    mail_accounts (id) {
        id -> Uuid,
        kind -> MailAccountKind,
        address -> Text,
        display_name -> Text,
        credential_encrypted -> Bytea,
        external_id -> Nullable<Text>,
        is_active -> Bool,
        last_checked_at -> Nullable<Timestamptz>,
        last_error -> Nullable<Text>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        mail_domain_id -> Nullable<Uuid>,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    mail_domains (id) {
        id -> Uuid,
        name -> Text,
        stalwart_id -> Text,
        is_default -> Bool,
        created_at -> Timestamptz,
        created_by -> Uuid,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    mail_servers (id) {
        id -> Uuid,
        hostname -> Text,
        admin_username -> Text,
        admin_secret_encrypted -> Bytea,
        created_at -> Timestamptz,
        created_by -> Uuid,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    meeting_corrections (meeting_id) {
        meeting_id -> Uuid,
        transcript -> Jsonb,
        transcript_hash -> Text,
        based_on -> Text,
        source_transcript_hash -> Text,
        whisper_transcript_hash -> Nullable<Text>,
        changed_lines -> Int4,
        fixes -> Jsonb,
        approved_by -> Nullable<Uuid>,
        approved_at -> Nullable<Timestamptz>,
        review_item_id -> Nullable<Uuid>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    meeting_transcriptions (meeting_id) {
        meeting_id -> Uuid,
        transcript -> Jsonb,
        transcript_hash -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    meetings (id) {
        id -> Uuid,
        connector_id -> Nullable<Uuid>,
        source_kind -> Text,
        external_id -> Text,
        title -> Text,
        started_at -> Timestamptz,
        duration_ms -> Nullable<Int8>,
        organizer -> Text,
        participants -> Jsonb,
        url -> Nullable<Text>,
        transcript -> Jsonb,
        storage_location_id -> Nullable<Uuid>,
        recording_path -> Nullable<Text>,
        recording_content_type -> Nullable<Text>,
        segments -> Nullable<Jsonb>,
        state -> Text,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        transcript_hash -> Nullable<Text>,
        hold_for_approval -> Bool,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    model_settings (agent) {
        agent -> Text,
        model -> Text,
        effort -> Text,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    page_attachments (id) {
        id -> Uuid,
        page_id -> Uuid,
        storage_location_id -> Uuid,
        storage_path -> Text,
        file_name -> Text,
        content_type -> Text,
        size_bytes -> Int8,
        created_by -> Uuid,
        agent -> Nullable<Text>,
        agent_name -> Nullable<Text>,
        created_at -> Timestamptz,
        deleted_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    page_events (page_id, id) {
        id -> Uuid,
        page_id -> Uuid,
        kind -> Text,
        created_by -> Uuid,
        agent -> Nullable<Text>,
        agent_name -> Nullable<Text>,
        data -> Jsonb,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    page_labels (page_id, label) {
        page_id -> Uuid,
        label -> Text,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    page_projects (page_id, project_id) {
        page_id -> Uuid,
        project_id -> Uuid,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    page_versions (page_id, version) {
        page_id -> Uuid,
        version -> Int4,
        title -> Text,
        body -> Text,
        category -> Nullable<Text>,
        evidence -> Nullable<Text>,
        restored_from -> Nullable<Int4>,
        created_by -> Uuid,
        agent -> Nullable<Text>,
        agent_name -> Nullable<Text>,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    pages (id) {
        id -> Uuid,
        parent_id -> Nullable<Uuid>,
        position -> Int8,
        title -> Text,
        body -> Text,
        category -> Nullable<Text>,
        evidence -> Nullable<Text>,
        current_version -> Int4,
        proposal_only -> Bool,
        deleted_at -> Nullable<Timestamptz>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::PasswordServer;

    password_vault (singleton) {
        singleton -> Bool,
        server -> PasswordServer,
        server_url -> Nullable<Text>,
        client_id -> Text,
        client_secret_encrypted -> Bytea,
        master_password_encrypted -> Bytea,
        account_email -> Text,
        collection_id -> Nullable<Uuid>,
        collection_name -> Nullable<Text>,
        organization_id -> Nullable<Uuid>,
        checked_at -> Timestamptz,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    process_activities (process_id) {
        process_id -> Uuid,
        activities -> Jsonb,
        expires_at -> Timestamptz,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::LinkProvider;

    project_issue_trackers (project_id) {
        project_id -> Uuid,
        provider -> LinkProvider,
        jira_credential_id -> Nullable<Uuid>,
        token_account_id -> Nullable<Uuid>,
        destination -> Text,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ProjectManagerTurnStatus;

    project_manager_turns (id) {
        id -> Uuid,
        project_id -> Uuid,
        status -> ProjectManagerTurnStatus,
        summary -> Nullable<Text>,
        reply -> Nullable<Text>,
        error -> Nullable<Text>,
        log -> Text,
        timeline_record_id -> Nullable<Uuid>,
        created_by -> Uuid,
        started_at -> Timestamptz,
        finished_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ProjectManagerWakeKind;

    project_manager_wakes (id) {
        id -> Uuid,
        project_id -> Uuid,
        kind -> ProjectManagerWakeKind,
        timeline_record_id -> Nullable<Uuid>,
        work_batch_id -> Nullable<Uuid>,
        body -> Nullable<Text>,
        turn_id -> Nullable<Uuid>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        promise_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    project_managers (project_id) {
        project_id -> Uuid,
        runner_session_id -> Nullable<Text>,
        usage_paused_until -> Nullable<Timestamptz>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::MergePolicy;

    project_repositories (id) {
        id -> Uuid,
        project_id -> Uuid,
        position -> Int4,
        url -> Text,
        base_branch -> Nullable<Text>,
        setup_script -> Nullable<Text>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        merge_policy -> MergePolicy,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::ProjectCoverFit;
    use super::sql_types::GithubAccess;

    projects (id) {
        id -> Uuid,
        name -> Text,
        description -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        cover_image -> Nullable<Bytea>,
        cover_image_updated_at -> Nullable<Timestamptz>,
        cover_fit -> ProjectCoverFit,
        github_access -> GithubAccess,
        github_credential_id -> Nullable<Uuid>,
        created_by -> Uuid,
        cover_removed_at -> Nullable<Timestamptz>,
        manager_writes_directly -> Bool,
        tools_setup_script -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::PromiseState;

    promises (id) {
        id -> Uuid,
        project_id -> Uuid,
        timeline_record_id -> Uuid,
        recipient -> Text,
        summary -> Text,
        state -> PromiseState,
        work_batch_id -> Nullable<Uuid>,
        work_item_id -> Nullable<Uuid>,
        due_at -> Nullable<Timestamptz>,
        follow_up_item_id -> Nullable<Uuid>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    secret_copies (target, row_id) {
        target -> Text,
        row_id -> Uuid,
        sealed_for -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    session_events (session_id, sequence) {
        session_id -> Int8,
        sequence -> Int8,
        occurred_at -> Timestamptz,
        turn_id -> Nullable<Uuid>,
        payload -> Jsonb,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::SessionTurnStatus;

    session_turns (id) {
        id -> Uuid,
        session_id -> Int8,
        prompt -> Text,
        system_prompt_append -> Nullable<Text>,
        status -> SessionTurnStatus,
        stop_requested -> Bool,
        queued_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        finished_at -> Nullable<Timestamptz>,
        created_by -> Uuid,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::StepJobState;

    step_jobs (id) {
        id -> Uuid,
        run_id -> Uuid,
        position -> Int4,
        step -> Text,
        state -> StepJobState,
        input -> Nullable<Jsonb>,
        output -> Nullable<Jsonb>,
        summary -> Nullable<Text>,
        error -> Nullable<Text>,
        log -> Text,
        attempts -> Int4,
        available_at -> Nullable<Timestamptz>,
        claim_token -> Nullable<Uuid>,
        lease_expires_at -> Nullable<Timestamptz>,
        started_at -> Nullable<Timestamptz>,
        finished_at -> Nullable<Timestamptz>,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    storage_location_projects (storage_location_id, project_id) {
        storage_location_id -> Uuid,
        project_id -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::StorageLocationKind;
    use super::sql_types::BunnyStorageRegion;
    use super::sql_types::S3Service;

    storage_locations (id) {
        id -> Uuid,
        name -> Text,
        kind -> StorageLocationKind,
        bunny_zone -> Nullable<Text>,
        bunny_region -> Nullable<BunnyStorageRegion>,
        path_prefix -> Text,
        storage_limit_bytes -> Nullable<Int8>,
        access_key_encrypted -> Bytea,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        all_projects -> Bool,
        s3_service -> Nullable<S3Service>,
        s3_bucket -> Nullable<Text>,
        s3_region -> Nullable<Text>,
        s3_access_key_id -> Nullable<Text>,
        created_by -> Uuid,
        is_studio_default -> Bool,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::StudioAssetKind;

    studio_assets (id) {
        id -> Uuid,
        studio_item_id -> Uuid,
        session_id -> Nullable<Int8>,
        kind -> StudioAssetKind,
        artifact_path -> Text,
        content_type -> Nullable<Text>,
        size_bytes -> Int8,
        sha256 -> Text,
        storage_path -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    studio_feedback (id) {
        id -> Uuid,
        studio_item_id -> Uuid,
        session_id -> Nullable<Int8>,
        turn_id -> Nullable<Text>,
        prompt -> Text,
        source_asset_id -> Nullable<Uuid>,
        camera_orbit -> Nullable<Text>,
        annotated_storage_path -> Text,
        annotated_size_bytes -> Int8,
        capture_storage_path -> Nullable<Text>,
        capture_size_bytes -> Nullable<Int8>,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    studio_items (id) {
        id -> Uuid,
        title -> Text,
        prompt -> Text,
        project_id -> Nullable<Uuid>,
        storage_location_id -> Uuid,
        thumbnail_asset_id -> Nullable<Uuid>,
        pull_error -> Nullable<Text>,
        deleted_at -> Nullable<Timestamptz>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    studio_storage_reservations (id) {
        id -> Uuid,
        storage_location_id -> Uuid,
        size_bytes -> Int8,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    timeline_record_events (id) {
        id -> Uuid,
        timeline_record_id -> Uuid,
        project_id -> Uuid,
        kind -> Text,
        created_by -> Uuid,
        data -> Jsonb,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    timeline_record_projects (timeline_record_id, project_id) {
        timeline_record_id -> Uuid,
        project_id -> Uuid,
        confidence -> Float4,
        attributed_by -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    timeline_records (id) {
        id -> Uuid,
        source_kind -> Text,
        connector_id -> Nullable<Uuid>,
        external_id -> Nullable<Text>,
        occurred_at -> Timestamptz,
        actor -> Text,
        title -> Text,
        body -> Text,
        url -> Nullable<Text>,
        media -> Nullable<Jsonb>,
        metadata -> Jsonb,
        action_item_id -> Nullable<Uuid>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        turn_summary_session_id -> Nullable<Int8>,
        turn_summary_turn_id -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    token_accounts (id) {
        id -> Uuid,
        kind -> Text,
        name -> Text,
        credential_encrypted -> Bytea,
        identity -> Text,
        checked_at -> Nullable<Timestamptz>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        site_url -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    turn_summaries (session_id, turn_id) {
        session_id -> Int8,
        turn_id -> Uuid,
        summary -> Text,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    update_checks (singleton) {
        singleton -> Bool,
        newest_release -> Nullable<Text>,
        last_checked_at -> Nullable<Timestamptz>,
        last_check_error -> Nullable<Text>,
        updater_seen_at -> Nullable<Timestamptz>,
        requested_release -> Nullable<Text>,
        requested_by -> Nullable<Uuid>,
        updated_by -> Uuid,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    user_events (id) {
        id -> Uuid,
        user_id -> Nullable<Uuid>,
        actor_id -> Nullable<Uuid>,
        kind -> Text,
        data -> Jsonb,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::UserRole;
    use super::sql_types::UserStatus;

    users (id) {
        id -> Uuid,
        kratos_identity_id -> Nullable<Uuid>,
        email -> Nullable<Text>,
        name -> Text,
        role -> Nullable<UserRole>,
        status -> Nullable<UserStatus>,
        approved_at -> Nullable<Timestamptz>,
        last_seen_at -> Nullable<Timestamptz>,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    work_batch_approval_items (project_id) {
        project_id -> Uuid,
        action_item_id -> Uuid,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    work_batch_events (id) {
        id -> Uuid,
        work_batch_id -> Uuid,
        kind -> Text,
        data -> Jsonb,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::PullRequestState;
    use super::sql_types::PullRequestMergeMethod;

    work_batch_pull_requests (id) {
        id -> Uuid,
        work_batch_id -> Uuid,
        repository -> Text,
        number -> Int4,
        url -> Text,
        title -> Text,
        state -> PullRequestState,
        merged_at -> Nullable<Timestamptz>,
        merged_by -> Nullable<Text>,
        merged_by_elysium -> Bool,
        merge_method -> Nullable<PullRequestMergeMethod>,
        held_because -> Nullable<Text>,
        looked_at -> Nullable<Timestamptz>,
        timeline_record_id -> Nullable<Uuid>,
        created_by -> Uuid,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::WorkBatchPriority;
    use super::sql_types::WorkBatchState;
    use super::sql_types::WorkBatchWait;
    use super::sql_types::WorkBatchOrigin;

    work_batches (id) {
        id -> Uuid,
        project_id -> Uuid,
        title -> Text,
        goal -> Text,
        repositories -> Jsonb,
        priority -> WorkBatchPriority,
        scheduled_for -> Nullable<Timestamptz>,
        state -> WorkBatchState,
        result -> Nullable<Text>,
        turn_id -> Nullable<Uuid>,
        source_record_ids -> Jsonb,
        action_item_ids -> Jsonb,
        timeline_record_id -> Nullable<Uuid>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        finished_at -> Nullable<Timestamptz>,
        pinned_at -> Nullable<Timestamptz>,
        assigned_at -> Nullable<Timestamptz>,
        claim_token -> Nullable<Uuid>,
        lease_expires_at -> Nullable<Timestamptz>,
        worker_session_id -> Nullable<Text>,
        waiting_on -> Nullable<WorkBatchWait>,
        waiting_detail -> Nullable<Text>,
        waiting_until -> Nullable<Timestamptz>,
        worker_report -> Nullable<Jsonb>,
        worker_log -> Text,
        configuration -> Nullable<Jsonb>,
        origin -> WorkBatchOrigin,
        generation -> Int4,
        parent_batch_ids -> Jsonb,
        source_message_ids -> Jsonb,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    worker_pool (singleton) {
        singleton -> Bool,
        cpu_count -> Int4,
        usage_held_until -> Nullable<Timestamptz>,
        created_by -> Uuid,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    workflow_run_events (id) {
        id -> Uuid,
        run_id -> Uuid,
        step_job_id -> Nullable<Uuid>,
        kind -> Text,
        created_by -> Uuid,
        data -> Jsonb,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use super::sql_types::WorkflowRunState;

    workflow_runs (id) {
        id -> Uuid,
        workflow -> Text,
        source_kind -> Text,
        source_id -> Text,
        state -> WorkflowRunState,
        input -> Jsonb,
        error -> Nullable<Text>,
        created_by -> Uuid,
        created_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        finished_at -> Nullable<Timestamptz>,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    workspace_settings (singleton) {
        singleton -> Bool,
        signup_open -> Bool,
        require_mfa -> Bool,
        updated_by -> Uuid,
        updated_at -> Timestamptz,
        agent_configuration_repository -> Nullable<Text>,
        backup_storage_location_id -> Nullable<Uuid>,
        worker_pool_size -> Nullable<Int4>,
        page_attachment_storage_location_id -> Nullable<Uuid>,
        agent_configuration_github_credential_id -> Nullable<Uuid>,
    }
}

diesel::joinable!(action_item_comments -> action_items (action_item_id));
diesel::joinable!(action_item_events -> action_items (action_item_id));
diesel::joinable!(action_item_events -> changesets (changeset_id));
diesel::joinable!(action_item_events -> initiatives (initiative_id));
diesel::joinable!(action_item_issue_filings -> action_items (action_item_id));
diesel::joinable!(action_item_issue_filings -> changeset_operations (changeset_operation_id));
diesel::joinable!(action_item_issue_filings -> github_credentials (github_credential_id));
diesel::joinable!(action_item_issue_filings -> jira_credentials (jira_credential_id));
diesel::joinable!(action_item_issue_filings -> timeline_records (timeline_record_id));
diesel::joinable!(action_item_issue_filings -> token_accounts (token_account_id));
diesel::joinable!(action_item_link_writes -> action_item_comments (comment_id));
diesel::joinable!(action_item_link_writes -> action_item_links (link_id));
diesel::joinable!(action_item_links -> action_items (action_item_id));
diesel::joinable!(action_item_links -> github_credentials (github_credential_id));
diesel::joinable!(action_item_links -> jira_credentials (jira_credential_id));
diesel::joinable!(action_item_links -> token_accounts (token_account_id));
diesel::joinable!(action_item_projects -> action_items (action_item_id));
diesel::joinable!(action_item_projects -> projects (project_id));
diesel::joinable!(action_items -> users (created_by));
diesel::joinable!(agent_configuration_status -> action_items (inbox_item_id));
diesel::joinable!(agent_configuration_status -> users (created_by));
diesel::joinable!(agent_guidance_versions -> users (created_by));
diesel::joinable!(agent_tool_grants -> users (created_by));
diesel::joinable!(assigned_issues -> action_items (action_item_id));
diesel::joinable!(assigned_issues -> connectors (connector_id));
diesel::joinable!(assigned_issues -> users (created_by));
diesel::joinable!(authenticator_nicknames -> users (user_id));
diesel::joinable!(authenticator_replacements -> users (user_id));
diesel::joinable!(browser_sessions -> users (created_by));
diesel::joinable!(changeset_operations -> changesets (changeset_id));
diesel::joinable!(changeset_operations -> timeline_records (cause_record_id));
diesel::joinable!(changesets -> projects (project_id));
diesel::joinable!(claude_accounts -> llms (llm_id));
diesel::joinable!(coding_sessions -> action_items (action_item_id));
diesel::joinable!(coding_sessions -> github_credentials (github_credential_id));
diesel::joinable!(coding_sessions -> projects (project_id));
diesel::joinable!(coding_sessions -> studio_items (studio_item_id));
diesel::joinable!(coding_sessions -> users (created_by));
diesel::joinable!(company_doc_proposals -> timeline_records (timeline_record_id));
diesel::joinable!(company_doc_proposals -> workflow_runs (workflow_run_id));
diesel::joinable!(company_doc_review_cursor -> users (created_by));
diesel::joinable!(connector_check_results -> connectors (connector_id));
diesel::joinable!(connector_runs -> connectors (connector_id));
diesel::joinable!(connectors -> browser_sessions (browser_session_id));
diesel::joinable!(connectors -> github_credentials (github_credential_id));
diesel::joinable!(connectors -> jira_credentials (jira_credential_id));
diesel::joinable!(connectors -> storage_locations (storage_location_id));
diesel::joinable!(connectors -> token_accounts (token_account_id));
diesel::joinable!(connectors -> users (created_by));
diesel::joinable!(conversation_messages -> conversations (conversation_id));
diesel::joinable!(conversation_messages -> timeline_records (timeline_record_id));
diesel::joinable!(conversation_messages -> workflow_runs (run_id));
diesel::joinable!(conversations -> connectors (connector_id));
diesel::joinable!(conversations -> users (created_by));
diesel::joinable!(conversations -> workflow_runs (waited_in_run_id));
diesel::joinable!(database_backups -> storage_locations (storage_location_id));
diesel::joinable!(database_backups -> users (created_by));
diesel::joinable!(deployment_updates -> users (created_by));
diesel::joinable!(drafts -> connectors (connector_id));
diesel::joinable!(drafts -> mail_accounts (mail_account_id));
diesel::joinable!(drafts -> promises (promise_id));
diesel::joinable!(drafts -> users (created_by));
diesel::joinable!(elysia_messages -> users (created_by));
diesel::joinable!(elysia_questions -> action_items (action_item_id));
diesel::joinable!(elysia_state -> users (updated_by));
diesel::joinable!(elysia_turns -> users (created_by));
diesel::joinable!(elysia_work -> action_items (action_item_id));
diesel::joinable!(elysia_work -> users (created_by));
diesel::joinable!(environment_variables -> users (created_by));
diesel::joinable!(github_credentials -> users (created_by));
diesel::joinable!(initiative_items -> action_items (action_item_id));
diesel::joinable!(initiative_items -> initiative_links (via_link_id));
diesel::joinable!(initiative_items -> initiatives (initiative_id));
diesel::joinable!(initiative_links -> github_credentials (github_credential_id));
diesel::joinable!(initiative_links -> initiatives (initiative_id));
diesel::joinable!(initiative_links -> jira_credentials (jira_credential_id));
diesel::joinable!(initiative_projects -> initiatives (initiative_id));
diesel::joinable!(initiative_projects -> projects (project_id));
diesel::joinable!(initiatives -> users (created_by));
diesel::joinable!(issue_reports -> users (created_by));
diesel::joinable!(jira_credential_boards -> jira_credentials (jira_credential_id));
diesel::joinable!(jira_credential_projects -> jira_credentials (jira_credential_id));
diesel::joinable!(jira_credentials -> users (created_by));
diesel::joinable!(jira_done_transitions -> jira_credentials (jira_credential_id));
diesel::joinable!(kind_model_settings -> projects (project_id));
diesel::joinable!(kind_model_settings -> users (created_by));
diesel::joinable!(knowledge_asks -> users (created_by));
diesel::joinable!(knowledge_backfills -> users (created_by));
diesel::joinable!(knowledge_documents -> pages (page_id));
diesel::joinable!(knowledge_documents -> timeline_records (timeline_record_id));
diesel::joinable!(knowledge_documents -> users (created_by));
diesel::joinable!(knowledge_passages -> knowledge_documents (document_id));
diesel::joinable!(learning_categories -> users (created_by));
diesel::joinable!(link_watch_cursors -> github_credentials (github_credential_id));
diesel::joinable!(link_watch_cursors -> jira_credentials (jira_credential_id));
diesel::joinable!(link_watch_cursors -> token_accounts (token_account_id));
diesel::joinable!(llms -> users (created_by));
diesel::joinable!(mail_accounts -> mail_domains (mail_domain_id));
diesel::joinable!(mail_accounts -> users (created_by));
diesel::joinable!(mail_domains -> users (created_by));
diesel::joinable!(mail_servers -> users (created_by));
diesel::joinable!(meeting_corrections -> action_items (review_item_id));
diesel::joinable!(meeting_corrections -> meetings (meeting_id));
diesel::joinable!(meeting_corrections -> users (approved_by));
diesel::joinable!(meeting_transcriptions -> meetings (meeting_id));
diesel::joinable!(meetings -> connectors (connector_id));
diesel::joinable!(meetings -> storage_locations (storage_location_id));
diesel::joinable!(meetings -> users (created_by));
diesel::joinable!(model_settings -> users (created_by));
diesel::joinable!(page_attachments -> pages (page_id));
diesel::joinable!(page_attachments -> storage_locations (storage_location_id));
diesel::joinable!(page_attachments -> users (created_by));
diesel::joinable!(page_events -> pages (page_id));
diesel::joinable!(page_events -> users (created_by));
diesel::joinable!(page_labels -> pages (page_id));
diesel::joinable!(page_labels -> users (created_by));
diesel::joinable!(page_projects -> pages (page_id));
diesel::joinable!(page_projects -> projects (project_id));
diesel::joinable!(page_projects -> users (created_by));
diesel::joinable!(page_versions -> pages (page_id));
diesel::joinable!(page_versions -> users (created_by));
diesel::joinable!(pages -> learning_categories (category));
diesel::joinable!(pages -> users (created_by));
diesel::joinable!(password_vault -> users (created_by));
diesel::joinable!(process_activities -> users (created_by));
diesel::joinable!(project_issue_trackers -> jira_credentials (jira_credential_id));
diesel::joinable!(project_issue_trackers -> projects (project_id));
diesel::joinable!(project_issue_trackers -> token_accounts (token_account_id));
diesel::joinable!(project_issue_trackers -> users (created_by));
diesel::joinable!(project_manager_turns -> project_managers (project_id));
diesel::joinable!(project_manager_turns -> timeline_records (timeline_record_id));
diesel::joinable!(project_manager_turns -> users (created_by));
diesel::joinable!(project_manager_wakes -> project_manager_turns (turn_id));
diesel::joinable!(project_manager_wakes -> projects (project_id));
diesel::joinable!(project_manager_wakes -> promises (promise_id));
diesel::joinable!(project_manager_wakes -> timeline_records (timeline_record_id));
diesel::joinable!(project_manager_wakes -> users (created_by));
diesel::joinable!(project_manager_wakes -> work_batches (work_batch_id));
diesel::joinable!(project_managers -> projects (project_id));
diesel::joinable!(project_managers -> users (created_by));
diesel::joinable!(project_repositories -> projects (project_id));
diesel::joinable!(project_repositories -> users (created_by));
diesel::joinable!(projects -> github_credentials (github_credential_id));
diesel::joinable!(projects -> users (created_by));
diesel::joinable!(promises -> projects (project_id));
diesel::joinable!(promises -> timeline_records (timeline_record_id));
diesel::joinable!(promises -> users (created_by));
diesel::joinable!(promises -> work_batches (work_batch_id));
diesel::joinable!(session_events -> coding_sessions (session_id));
diesel::joinable!(session_turns -> coding_sessions (session_id));
diesel::joinable!(session_turns -> users (created_by));
diesel::joinable!(step_jobs -> workflow_runs (run_id));
diesel::joinable!(storage_location_projects -> projects (project_id));
diesel::joinable!(storage_location_projects -> storage_locations (storage_location_id));
diesel::joinable!(storage_locations -> users (created_by));
diesel::joinable!(studio_assets -> coding_sessions (session_id));
diesel::joinable!(studio_feedback -> coding_sessions (session_id));
diesel::joinable!(studio_feedback -> studio_assets (source_asset_id));
diesel::joinable!(studio_feedback -> studio_items (studio_item_id));
diesel::joinable!(studio_feedback -> users (created_by));
diesel::joinable!(studio_items -> projects (project_id));
diesel::joinable!(studio_items -> storage_locations (storage_location_id));
diesel::joinable!(studio_items -> users (created_by));
diesel::joinable!(studio_storage_reservations -> storage_locations (storage_location_id));
diesel::joinable!(studio_storage_reservations -> users (created_by));
diesel::joinable!(timeline_record_events -> projects (project_id));
diesel::joinable!(timeline_record_events -> timeline_records (timeline_record_id));
diesel::joinable!(timeline_record_events -> users (created_by));
diesel::joinable!(timeline_record_projects -> projects (project_id));
diesel::joinable!(timeline_record_projects -> timeline_records (timeline_record_id));
diesel::joinable!(timeline_records -> action_items (action_item_id));
diesel::joinable!(timeline_records -> connectors (connector_id));
diesel::joinable!(timeline_records -> users (created_by));
diesel::joinable!(token_accounts -> users (created_by));
diesel::joinable!(turn_summaries -> coding_sessions (session_id));
diesel::joinable!(work_batch_approval_items -> action_items (action_item_id));
diesel::joinable!(work_batch_approval_items -> projects (project_id));
diesel::joinable!(work_batch_approval_items -> users (created_by));
diesel::joinable!(work_batch_events -> users (created_by));
diesel::joinable!(work_batch_events -> work_batches (work_batch_id));
diesel::joinable!(work_batch_pull_requests -> timeline_records (timeline_record_id));
diesel::joinable!(work_batch_pull_requests -> users (created_by));
diesel::joinable!(work_batch_pull_requests -> work_batches (work_batch_id));
diesel::joinable!(work_batches -> project_manager_turns (turn_id));
diesel::joinable!(work_batches -> projects (project_id));
diesel::joinable!(work_batches -> timeline_records (timeline_record_id));
diesel::joinable!(work_batches -> users (created_by));
diesel::joinable!(worker_pool -> users (created_by));
diesel::joinable!(workflow_run_events -> step_jobs (step_job_id));
diesel::joinable!(workflow_run_events -> users (created_by));
diesel::joinable!(workflow_run_events -> workflow_runs (run_id));
diesel::joinable!(workflow_runs -> users (created_by));
diesel::joinable!(workspace_settings -> github_credentials (agent_configuration_github_credential_id));
diesel::joinable!(workspace_settings -> users (updated_by));

diesel::allow_tables_to_appear_in_same_query!(
    action_item_comments,
    action_item_events,
    action_item_issue_filings,
    action_item_link_writes,
    action_item_links,
    action_item_projects,
    action_items,
    agent_configuration_status,
    agent_guidance_versions,
    agent_tool_grants,
    assigned_issues,
    authenticator_nicknames,
    authenticator_replacements,
    browser_sessions,
    changeset_operations,
    changesets,
    claude_accounts,
    coding_sessions,
    company_doc_proposals,
    company_doc_review_cursor,
    connector_check_results,
    connector_runs,
    connectors,
    conversation_messages,
    conversations,
    database_backups,
    deployment_updates,
    drafts,
    elysia_messages,
    elysia_questions,
    elysia_state,
    elysia_turns,
    elysia_work,
    environment_variables,
    github_credentials,
    initiative_items,
    initiative_links,
    initiative_projects,
    initiatives,
    issue_reports,
    jira_credential_boards,
    jira_credential_projects,
    jira_credentials,
    jira_done_transitions,
    kind_model_settings,
    knowledge_asks,
    knowledge_backfills,
    knowledge_documents,
    knowledge_passages,
    learning_categories,
    link_watch_cursors,
    llms,
    mail_accounts,
    mail_domains,
    mail_servers,
    meeting_corrections,
    meeting_transcriptions,
    meetings,
    model_settings,
    page_attachments,
    page_events,
    page_labels,
    page_projects,
    page_versions,
    pages,
    password_vault,
    process_activities,
    project_issue_trackers,
    project_manager_turns,
    project_manager_wakes,
    project_managers,
    project_repositories,
    projects,
    promises,
    secret_copies,
    session_events,
    session_turns,
    step_jobs,
    storage_location_projects,
    storage_locations,
    studio_assets,
    studio_feedback,
    studio_items,
    studio_storage_reservations,
    timeline_record_events,
    timeline_record_projects,
    timeline_records,
    token_accounts,
    turn_summaries,
    update_checks,
    user_events,
    users,
    work_batch_approval_items,
    work_batch_events,
    work_batch_pull_requests,
    work_batches,
    worker_pool,
    workflow_run_events,
    workflow_runs,
    workspace_settings,
);
