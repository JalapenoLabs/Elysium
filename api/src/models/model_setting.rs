// Copyright © 2026 Jalapeno Labs

//! The model and effort an agent runs on, where an admin chose one: one row per agent.
//!
//! An agent is named as it is in code: a workflow step's name (`correct_transcript`), or the
//! name of an agent outside the step framework (`elysia`, `turn_summary`). A row exists only for
//! an agent an admin changed; every other one runs on its default, which `crate::agent_models`
//! holds beside the list of agents. That module is the one place an agent's model is resolved,
//! and the one caller of [`set`] and [`clear`], which it reaches after checking the agent exists
//! and runs a model.
//!
//! Every change is recorded in the workspace's history (`user_events`) with what the agent ran
//! on before and after, in the same transaction, as every other workspace setting is.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::schema::model_settings;
use crate::models::user::{self, UserEventKind};

/// The model an agent runs on until an admin chooses another, unless its own default says
/// otherwise: the strongest, since an agent's work (correcting a transcript, planning a
/// project, a coding session) is read by people and acted on.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

/// The effort an agent runs at until an admin chooses another, unless its own default says
/// otherwise.
pub const DEFAULT_EFFORT: Effort = Effort::High;

/// How hard the model thinks before it answers: the levels the official CLI's `--effort` takes.
/// Stored as text, so a level Anthropic adds needs no enum migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl Effort {
    /// Every level, lowest first: the order the settings page offers them in.
    pub const ALL: [Self; 5] = [Self::Low, Self::Medium, Self::High, Self::Xhigh, Self::Max];

    /// The level as the CLI and the database name it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }

    /// The level stored as `name`, if this build knows it.
    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|effort| effort.as_str() == name)
    }
}

/// The model and effort one agent's turns run on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChoice {
    /// What the CLI's `--model` takes: an id such as [`DEFAULT_MODEL`], or an alias.
    pub model: String,
    pub effort: Effort,
}

impl Default for ModelChoice {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_owned(),
            effort: DEFAULT_EFFORT,
        }
    }
}

/// An agent's stored setting, as the settings page lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSetting {
    pub agent: String,
    pub choice: ModelChoice,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = model_settings)]
struct SettingRow<'a> {
    agent: &'a str,
    model: &'a str,
    effort: &'static str,
    created_by: Uuid,
}

/// Agent `agent`'s stored setting, if an admin chose one.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    agent: &str,
) -> QueryResult<Option<StoredSetting>> {
    let stored: Option<(String, String, DateTime<Utc>)> = model_settings::table
        .find(agent)
        .select((
            model_settings::model,
            model_settings::effort,
            model_settings::updated_at,
        ))
        .first(connection)
        .await
        .optional()?;
    Ok(stored.map(|(model, effort, updated_at)| StoredSetting {
        agent: agent.to_owned(),
        choice: choice_from_row(agent, model, &effort),
        updated_at,
    }))
}

/// Every stored setting, by agent name.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<StoredSetting>> {
    let rows: Vec<(String, String, String, DateTime<Utc>)> = model_settings::table
        .select((
            model_settings::agent,
            model_settings::model,
            model_settings::effort,
            model_settings::updated_at,
        ))
        .order(model_settings::agent)
        .load(connection)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(agent, model, effort, updated_at)| {
            let choice = choice_from_row(&agent, model, &effort);
            StoredSetting {
                agent,
                choice,
                updated_at,
            }
        })
        .collect())
}

/// Sets what agent `agent`'s turns run on, as `admin` chose, recording the change. Answers
/// whether anything changed: choosing what is already stored records nothing.
///
/// # Errors
/// Propagates any database error, including a check violation for an agent name that is not
/// `snake_case` or a model that could read as a flag.
pub async fn set(
    connection: &mut AsyncPgConnection,
    agent: &str,
    choice: &ModelChoice,
    admin: Uuid,
) -> QueryResult<bool> {
    connection
        .transaction(async move |connection| {
            let before = locked(connection, agent).await?;
            if before.as_ref() == Some(choice) {
                return Ok(false);
            }
            diesel::insert_into(model_settings::table)
                .values(SettingRow {
                    agent,
                    model: &choice.model,
                    effort: choice.effort.as_str(),
                    created_by: admin,
                })
                .on_conflict(model_settings::agent)
                .do_update()
                .set((
                    model_settings::model.eq(&choice.model),
                    model_settings::effort.eq(choice.effort.as_str()),
                ))
                .execute(connection)
                .await?;
            record_change(connection, agent, before.as_ref(), Some(choice), admin).await?;
            Ok(true)
        })
        .await
}

/// Removes agent `agent`'s setting, as `admin` asked, so it runs on its default again, recording
/// the change. Answers whether there was one to remove.
///
/// # Errors
/// Propagates any database error.
pub async fn clear(
    connection: &mut AsyncPgConnection,
    agent: &str,
    admin: Uuid,
) -> QueryResult<bool> {
    connection
        .transaction(async move |connection| {
            let Some(before) = locked(connection, agent).await? else {
                return Ok(false);
            };
            diesel::delete(model_settings::table.find(agent))
                .execute(connection)
                .await?;
            record_change(connection, agent, Some(&before), None, admin).await?;
            Ok(true)
        })
        .await
}

/// Agent `agent`'s stored setting, its row locked until the transaction ends, so two admins'
/// changes are recorded one after the other with the right values before each.
async fn locked(
    connection: &mut AsyncPgConnection,
    agent: &str,
) -> QueryResult<Option<ModelChoice>> {
    let stored: Option<(String, String)> = model_settings::table
        .find(agent)
        .select((model_settings::model, model_settings::effort))
        .for_update()
        .first(connection)
        .await
        .optional()?;
    Ok(stored.map(|(model, effort)| choice_from_row(agent, model, &effort)))
}

/// Records in the workspace's history that `admin` moved agent `agent` from `before` to
/// `after`, either of which is `None` for the agent's default.
async fn record_change(
    connection: &mut AsyncPgConnection,
    agent: &str,
    before: Option<&ModelChoice>,
    after: Option<&ModelChoice>,
    admin: Uuid,
) -> QueryResult<()> {
    user::record_event(
        connection,
        None,
        admin,
        UserEventKind::ModelSettingChanged,
        json!({
            "agent": agent,
            "before": history_value(before),
            "after": history_value(after),
        }),
    )
    .await
}

/// A setting as the history records it: its model and effort, or `null` for the default.
fn history_value(choice: Option<&ModelChoice>) -> Value {
    choice.map_or(
        Value::Null,
        |choice| json!({ "model": choice.model, "effort": choice.effort.as_str() }),
    )
}

/// A stored row as a choice. Its effort reads as the default when this build does not know it.
fn choice_from_row(agent: &str, model: String, effort: &str) -> ModelChoice {
    let Some(effort) = Effort::from_name(effort) else {
        // Only a later version of Elysium could have stored it, since the column's check holds
        // the levels this one knows.
        event!(
            name: "model_settings.unknown_effort",
            Level::WARN,
            agent.name = agent,
            effort = %effort,
            "an agent's stored effort is not one this build knows; its turns use the default effort",
        );
        return ModelChoice {
            model,
            effort: DEFAULT_EFFORT,
        };
    };
    ModelChoice { model, effort }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effort_reads_back_from_the_name_it_is_stored_as() {
        for effort in Effort::ALL {
            assert_eq!(Effort::from_name(effort.as_str()), Some(effort));
            assert_eq!(
                serde_json::to_value(effort).expect("serializes"),
                effort.as_str(),
                "clients see the name the CLI takes"
            );
        }
        assert_eq!(Effort::from_name("extreme"), None);
    }

    #[test]
    fn an_agent_nobody_configured_runs_on_opus_at_high_effort() {
        let choice = ModelChoice::default();
        assert_eq!(choice.model, "claude-opus-5-5");
        assert_eq!(choice.effort, Effort::High);
    }

    #[test]
    fn the_history_names_a_default_as_null_and_a_choice_by_its_words() {
        let choice = ModelChoice {
            model: "claude-haiku-5-5".to_owned(),
            effort: Effort::Xhigh,
        };
        assert_eq!(history_value(None), Value::Null);
        assert_eq!(
            history_value(Some(&choice)),
            json!({ "model": "claude-haiku-5-5", "effort": "xhigh" })
        );
    }
}
