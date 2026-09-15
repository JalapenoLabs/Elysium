// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "llm_type"))]
    pub struct LlmType;

    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "mail_account_kind"))]
    pub struct MailAccountKind;
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
    }
}

diesel::table! {
    use diesel::sql_types::*;

    mail_servers (id) {
        id -> Uuid,
        domain -> Text,
        admin_username -> Text,
        admin_secret_encrypted -> Bytea,
        created_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    projects (id) {
        id -> Uuid,
        name -> Text,
        description -> Text,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
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

diesel::joinable!(coding_sessions -> projects (project_id));
diesel::joinable!(coding_sessions -> satellites (satellite_id));

diesel::allow_tables_to_appear_in_same_query!(
    coding_sessions,
    llms,
    mail_accounts,
    mail_servers,
    projects,
    satellites,
);
