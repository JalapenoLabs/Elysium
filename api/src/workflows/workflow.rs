// Copyright © 2026 Jalapeno Labs

//! Workflows: steps composed in code to solve one problem, in whatever order and subset it needs.
//!
//! A workflow is declared from step types, first to last:
//!
//! ```ignore
//! let meeting = workflows::declare("meeting")
//!     .first::<FetchTranscript>()
//!     .then::<CorrectTranscript>()
//!     .then::<SegmentTranscript>()
//!     .build();
//! ```
//!
//! Each [`then`](WorkflowBuilder::then) takes only a step whose input is the output of the step
//! before it, so a declaration that compiles hands every step what it takes. The declaration
//! holds step types, never instances: a workflow is only names in order, and the instances that
//! run are registered once in [`Steps`](super::Steps), which one workflow or many may share.
//!
//! A run starts from a workflow with its source and input ([`Workflow::new_run`]), or enters it
//! at a later step with that step's own input ([`Workflow::new_run_at`]): a Slack request has a
//! conversation to segment and nothing to fetch. Either way the run's steps are the workflow's
//! from that step on, each a row of the queue.

use std::any::TypeId;
use std::marker::PhantomData;

use serde::Serialize;
use uuid::Uuid;

use super::Step;
use crate::agent_models::AgentKind;
use crate::models::workflow_run::NewRun;

/// Begins declaring the workflow named `name`, which runs list it by and the Runs page shows in
/// words: `snake_case`, and never changed once runs name it.
///
/// # Panics
/// Panics when `name` is not `snake_case`, which only a programming error can cause.
pub fn declare(name: &'static str) -> Declaration {
    assert!(
        super::is_name(name),
        "workflow {name} is not named in snake_case"
    );
    Declaration { name }
}

/// A workflow named, before its first step.
#[derive(Debug, Clone, Copy)]
pub struct Declaration {
    name: &'static str,
}

impl Declaration {
    /// Starts the workflow with `First`, whose input is what a run from the beginning takes.
    pub fn first<First: Step>(self) -> WorkflowBuilder<First::Input, First::Output> {
        WorkflowBuilder {
            name: self.name,
            steps: vec![DeclaredStep::of::<First>()],
            types: PhantomData,
        }
    }
}

/// A workflow being declared: its steps so far, a run's input, and what the last step gives.
#[derive(Debug)]
pub struct WorkflowBuilder<Input, Last> {
    name: &'static str,
    steps: Vec<DeclaredStep>,
    types: PhantomData<fn() -> (Input, Last)>,
}

impl<Input, Last> WorkflowBuilder<Input, Last> {
    /// Follows the last step with `Next`, which takes what that step gives.
    pub fn then<Next: Step<Input = Last>>(mut self) -> WorkflowBuilder<Input, Next::Output> {
        self.steps.push(DeclaredStep::of::<Next>());
        WorkflowBuilder {
            name: self.name,
            steps: self.steps,
            types: PhantomData,
        }
    }

    /// The workflow as declared.
    pub fn build(self) -> Workflow<Input> {
        Workflow {
            name: self.name,
            steps: self.steps,
            input: PhantomData,
        }
    }
}

/// A declared workflow, whose runs from the beginning take `Input`.
#[derive(Debug, Clone)]
pub struct Workflow<Input> {
    name: &'static str,
    steps: Vec<DeclaredStep>,
    input: PhantomData<fn() -> Input>,
}

/// One step of a declared workflow.
#[derive(Debug, Clone, Copy)]
pub struct DeclaredStep {
    name: &'static str,
    /// What the step declares as [`Step::AGENT_KIND`].
    agent_kind: Option<AgentKind>,
    /// The step's type, which the steps registered by its name must be.
    pub(super) type_id: TypeId,
}

impl DeclaredStep {
    fn of<Declared: Step>() -> Self {
        Self {
            name: Declared::NAME,
            agent_kind: Declared::AGENT_KIND,
            type_id: TypeId::of::<Declared>(),
        }
    }

    /// The step's name in code.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The kind of agent its work is, or `None` when it runs no model.
    pub fn agent_kind(&self) -> Option<AgentKind> {
        self.agent_kind
    }
}

/// A declared workflow without its input type: its name and its steps in order, which is all
/// the engine's checks and the model settings page need, so workflows of every input type sit
/// in one list (`crate::workflows::declared`).
#[derive(Debug, Clone)]
pub struct Outline {
    name: &'static str,
    steps: Vec<DeclaredStep>,
}

impl Outline {
    /// The workflow's name in code.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Its steps, first to last.
    pub fn steps(&self) -> &[DeclaredStep] {
        &self.steps
    }
}

/// What a run is about, what its first step takes, and who starts it.
#[derive(Debug, Clone)]
pub struct Start<Input> {
    /// A kind, such as `meeting`, and that thing's id. A workflow has one live run per source.
    pub source_kind: String,
    pub source_id: String,
    pub input: Input,
    /// A person, or a machine user for what Elysium starts itself.
    pub created_by: Uuid,
}

impl<Input> Workflow<Input> {
    /// The workflow's name in code.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The workflow without its input type, for lists of every workflow.
    pub fn outline(&self) -> Outline {
        Outline {
            name: self.name,
            steps: self.steps.clone(),
        }
    }
}

impl<Input: Serialize> Workflow<Input> {
    /// A run of every step, from the first, for `workflows::start`.
    ///
    /// # Panics
    /// Panics when the input cannot be JSON, which only its type can cause.
    pub fn new_run(&self, start: Start<Input>) -> NewRun {
        self.run_from(0, start)
    }

    /// A run that enters the workflow at step `Entry`, with what `Entry` takes, and runs it and
    /// every step after it. A step the workflow declares twice is entered where it is first.
    ///
    /// # Panics
    /// Panics when the workflow has no step `Entry`, or the input cannot be JSON, which only a
    /// programming error can cause.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "intake (#81) enters a workflow at segmenting")
    )]
    pub fn new_run_at<Entry: Step>(&self, start: Start<Entry::Input>) -> NewRun {
        let entry_type = TypeId::of::<Entry>();
        let position = self
            .steps
            .iter()
            .position(|step| step.type_id == entry_type)
            .unwrap_or_else(|| panic!("workflow {} has no step {}", self.name, Entry::NAME));
        self.run_from(position, start)
    }

    /// A run of the steps from `position` on, the first taking `start`'s input.
    fn run_from(&self, position: usize, start: Start<impl Serialize>) -> NewRun {
        let steps = self.steps[position..]
            .iter()
            .map(|step| step.name.to_owned())
            .collect();
        NewRun {
            workflow: self.name.to_owned(),
            source_kind: start.source_kind,
            source_id: start.source_id,
            // A step's input type is a plain record of what it needs; only a type such as a
            // map keyed by numbers cannot be JSON, and the first run of it would say so.
            input: serde_json::to_value(start.input)
                .expect("a step's input type serializes to JSON"),
            steps,
            created_by: start.created_by,
        }
    }
}
