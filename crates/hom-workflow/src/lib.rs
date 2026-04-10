//! # hom-workflow
//!
//! YAML-driven DAG workflow engine for HOM.
//!
//! Parses workflow definitions, resolves step dependencies into a DAG,
//! and executes steps across harnesses with retry, timeout, and templating.

pub mod checkpoint;
pub mod condition;
pub mod dag;
pub mod executor;
pub mod parser;

pub use executor::{
    CheckpointStore, StepResultRecord, WorkflowExecutor, WorkflowResult, WorkflowRuntime,
};
pub use parser::WorkflowDef;
