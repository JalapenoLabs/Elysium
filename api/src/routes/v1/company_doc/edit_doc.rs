// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/company-doc`: a person's own change to any number of sections, recorded as one
//! entry in the doc's history, so it can be undone. Sections left out keep their text.

use std::collections::BTreeMap;

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;

use super::author;
use crate::auth::MemberUser;
use crate::company_doc::{self, CompanyDocResponse};
use crate::errors::ApiError;
use crate::models::company_doc::{self as doc, Section};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// Each section to change, by its key, with its new Markdown, which is trimmed.
    sections: BTreeMap<Section, String>,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<CompanyDocResponse>, ApiError> {
    let Json(body) = body?;
    let changes = body
        .sections
        .into_iter()
        .map(|(section, content)| (section, content.trim().to_owned()))
        .collect();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let entry = doc::edit(&mut connection, changes, author(&member)).await?;
    if entry.is_some() {
        state.events.publish(&ServerEvent::CompanyDocChanged);
    }

    Ok(Json(company_doc::read(&mut connection).await?))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_edit_names_sections_by_their_keys_alone() {
        let body: RequestBody = serde_json::from_value(json!({
            "sections": { "glossary": "- ACME", "people": "" },
        }))
        .expect("parses");
        assert_eq!(body.sections.len(), 2);

        serde_json::from_value::<RequestBody>(json!({ "sections": { "history": "x" } }))
            .expect_err("only the doc's sections exist");
        serde_json::from_value::<RequestBody>(json!({ "sections": {}, "title": "x" }))
            .expect_err("a stray field is refused");
    }
}
