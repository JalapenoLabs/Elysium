// @generated automatically by Diesel CLI.

pub mod sql_types {
    #[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
    #[diesel(postgres_type(name = "llm_type"))]
    pub struct LlmType;
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
