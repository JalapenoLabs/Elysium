// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "bunny_storage_region"))]
    pub struct BunnyStorageRegion;

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
    #[diesel(postgres_type(name = "storage_location_kind"))]
    pub struct StorageLocationKind;
}

diesel::table! {
    use diesel::sql_types::*;

    coding_sessions (id) {
        id -> Uuid,
        satellite_id -> Uuid,
        thread_id -> Text,
        title -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        project_id -> Uuid,
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

    projects (id) {
        id -> Uuid,
        name -> Text,
        description -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
        cover_image -> Nullable<Bytea>,
        cover_image_updated_at -> Nullable<Timestamptz>,
        cover_fit -> ProjectCoverFit,
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
    }
}

diesel::joinable!(coding_sessions -> projects (project_id));
diesel::joinable!(coding_sessions -> satellites (satellite_id));
diesel::joinable!(mail_accounts -> mail_domains (mail_domain_id));
diesel::joinable!(storage_location_projects -> projects (project_id));
diesel::joinable!(storage_location_projects -> storage_locations (storage_location_id));

diesel::allow_tables_to_appear_in_same_query!(
    coding_sessions,
    llms,
    mail_accounts,
    mail_domains,
    mail_servers,
    projects,
    satellites,
    storage_location_projects,
    storage_locations,
);
