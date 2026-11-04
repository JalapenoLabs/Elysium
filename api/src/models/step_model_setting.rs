// Copyright © 2026 Jalapeno Labs

//! The model and effort each workflow step's agent turn runs on: one global setting per step.
//!
//! A step reads its setting as its turn starts (`crate::workflows::StepContext::model`), so a
//! change applies from the next turn of that step, a retry's included. A step with no row runs
//! on [`ModelChoice::default`]: Opus 5.5 at high effort. Either way the turn runs on the
//! runner's credential failover stack and usage-aware skipping, like every other turn.
//!
//! This is the one place a step's model is resolved, so admins' settings page and overrides per
//! agent kind (both on the roadmap, `docs/engine.md`, Models) change what [`for_step`] answers
//! rather than what any step does.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::schema::step_model_settings;

/// The model a step runs on until an admin chooses another: the strongest, since a step's
/// work (correcting a transcript, planning a project) is read by people and acted on.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";

/// The effort a step runs at until an admin chooses another.
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
        [Self::Low, Self::Medium, Self::High, Self::Xhigh, Self::Max]
            .into_iter()
            .find(|effort| effort.as_str() == name)
    }
}

/// The model and effort one step's turns run on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

#[derive(Insertable)]
#[diesel(table_name = step_model_settings)]
struct SettingRow<'a> {
    step: &'a str,
    model: &'a str,
    effort: &'static str,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

/// What step `step`'s turns run on: its setting, or the default when no admin chose one.
///
/// # Errors
/// Propagates any database error.
pub async fn for_step(connection: &mut AsyncPgConnection, step: &str) -> QueryResult<ModelChoice> {
    let stored: Option<(String, String)> = step_model_settings::table
        .find(step)
        .select((step_model_settings::model, step_model_settings::effort))
        .first(connection)
        .await
        .optional()?;
    let Some((model, effort)) = stored else {
        return Ok(ModelChoice::default());
    };
    let Some(effort) = Effort::from_name(&effort) else {
        // Only a later version of Elysium could have stored it, since the column's check holds
        // the levels this one knows.
        event!(
            name: "workflows.step_model.unknown_effort",
            Level::WARN,
            workflow.step.name = step,
            effort = %effort,
            "a step's stored effort is not one this build knows; its turns use the default effort",
        );
        return Ok(ModelChoice {
            model,
            effort: DEFAULT_EFFORT,
        });
    };
    Ok(ModelChoice { model, effort })
}

/// Sets what step `step`'s turns run on, as `admin` chose at `at`.
///
/// # Errors
/// Propagates any database error, including a check violation for a step name that is not
/// `snake_case` or a model that could read as a flag.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the admin settings page for models (#90) sets them; tests do until then"
    )
)]
pub async fn set(
    connection: &mut AsyncPgConnection,
    step: &str,
    choice: &ModelChoice,
    admin: Uuid,
    at: DateTime<Utc>,
) -> QueryResult<()> {
    diesel::insert_into(step_model_settings::table)
        .values(SettingRow {
            step,
            model: &choice.model,
            effort: choice.effort.as_str(),
            created_by: admin,
            created_at: at,
        })
        .on_conflict(step_model_settings::step)
        .do_update()
        .set((
            step_model_settings::model.eq(&choice.model),
            step_model_settings::effort.eq(choice.effort.as_str()),
        ))
        .execute(connection)
        .await
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_effort_reads_back_from_the_name_it_is_stored_as() {
        for effort in [
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::Xhigh,
            Effort::Max,
        ] {
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
    fn a_step_nobody_configured_runs_on_opus_at_high_effort() {
        let choice = ModelChoice::default();
        assert_eq!(choice.model, "claude-opus-5-5");
        assert_eq!(choice.effort, Effort::High);
    }
}
