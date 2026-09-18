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
    #[diesel(postgres_type(name = "bunny_storage_region"))]
    pub struct BunnyStorageRegion;

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
    #[diesel(postgres_type(name = "llm_type"))]
    pub struct LlmType;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "mail_account_kind"))]
    pub struct MailAccountKind;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "project_cover_fit"))]
    pub struct ProjectCoverFit;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "s3_service"))]
    pub struct S3Service;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "storage_location_kind"))]
    pub struct StorageLocationKind;
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
    }
}

diesel::table! {
    use diesel::sql_types::*;

    coding_sessions (id) {
        id -> Int8,
        satellite_id -> Uuid,
        thread_id -> Text,
        title -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        project_id -> Uuid,
        github_credential_id -> Nullable<Uuid>,
        action_item_id -> Nullable<Uuid>,
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
    }
}

diesel::table! {
    use diesel::sql_types::*;

    satellites (id) {
        id -> Uuid,
        name -> Text,
        description -> Text,
        url -> Text,
        secret_encrypted -> Bytea,
        is_active -> Bool,
        created_at -> Timestamptz,
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
    }
}

diesel::joinable!(action_item_comments -> action_items (action_item_id));
diesel::joinable!(action_item_events -> action_items (action_item_id));
diesel::joinable!(action_item_events -> initiatives (initiative_id));
diesel::joinable!(action_item_projects -> action_items (action_item_id));
diesel::joinable!(action_item_projects -> projects (project_id));
diesel::joinable!(coding_sessions -> action_items (action_item_id));
diesel::joinable!(coding_sessions -> github_credentials (github_credential_id));
diesel::joinable!(coding_sessions -> projects (project_id));
diesel::joinable!(coding_sessions -> satellites (satellite_id));
diesel::joinable!(initiative_items -> action_items (action_item_id));
diesel::joinable!(initiative_items -> initiatives (initiative_id));
diesel::joinable!(initiative_projects -> initiatives (initiative_id));
diesel::joinable!(initiative_projects -> projects (project_id));
diesel::joinable!(mail_accounts -> mail_domains (mail_domain_id));
diesel::joinable!(projects -> github_credentials (github_credential_id));
diesel::joinable!(storage_location_projects -> projects (project_id));
diesel::joinable!(storage_location_projects -> storage_locations (storage_location_id));

diesel::allow_tables_to_appear_in_same_query!(
    action_item_comments,
    action_item_events,
    action_item_projects,
    action_items,
    coding_sessions,
    environment_variables,
    github_credentials,
    initiative_items,
    initiative_projects,
    initiatives,
    llms,
    mail_accounts,
    mail_domains,
    mail_servers,
    projects,
    satellites,
    storage_location_projects,
    storage_locations,
);
