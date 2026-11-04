// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/drafts`: write a draft, sent nowhere until `POST /drafts/{id}/send`.
//!
//! A reply names one of the person's records (`kind: "reply"`), and for a chat where in its
//! conversation it goes, or for mail which of their mailboxes it goes from when they have more
//! than one. A new mail (`kind: "mail"`) names its mailbox, recipients, and subject.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::DraftResponse;
use crate::auth::CurrentUser;
use crate::drafts::{self, NewMail, Reply};
use crate::errors::ApiError;
use crate::models::draft::ReplyPlace;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum RequestBody {
    #[serde(rename_all = "camelCase")]
    Reply {
        record_id: Uuid,
        place: Option<ReplyPlace>,
        mail_account_id: Option<Uuid>,
        body: String,
    },
    #[serde(rename_all = "camelCase")]
    Mail {
        mail_account_id: Uuid,
        to: Vec<String>,
        subject: String,
        body: String,
    },
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    let created = match body {
        RequestBody::Reply {
            record_id,
            place,
            mail_account_id,
            body,
        } => {
            let reply = Reply {
                record_id,
                place,
                mail_account_id,
                body,
            };
            drafts::draft_reply(&state, &current.0, reply).await?
        }
        RequestBody::Mail {
            mail_account_id,
            to,
            subject,
            body,
        } => {
            let mail = NewMail {
                mail_account_id,
                to,
                subject,
                body,
            };
            drafts::draft_mail(&state, &current.0, mail).await?
        }
    };
    Ok((
        StatusCode::CREATED,
        Json(json!({ "draft": DraftResponse::from(created) })),
    ))
}
