//! The Provider trait and its request, event and auth types. No concrete providers.
//!
//! A community provider is a manifest plus three functions. Everything that
//! would otherwise be copied into each of them lives here instead: the failure
//! classification, the tool-call accumulator, the heuristic token counter.
//!
//! # The rules this crate exists to enforce
//!
//! - **Provider code never sleeps and never retries.** It classifies with
//!   [`ProviderError`]; the kernel decides whether to spend more budget.
//! - **Credentials stay in the broker.** A provider asks for a named grant
//!   through [`ProviderAuth`]; it never reads a key off disk or out of config.
//! - **Dropping the stream aborts the request.** Cancellation has to stop
//!   costing money, not just stop rendering.
//! - **Object-safe without `async_trait`.** [`Provider::stream`] is a plain fn
//!   returning a `'static` boxed stream.
//!
//! Implementation plan: `harness/docs/plans/03-provider-layer.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod accumulate;
pub mod auth;
pub mod counter;
pub mod error;
pub mod event;
pub mod provider;
pub mod request;

pub use accumulate::{AccumulateError, CompletedToolCall, ToolCallAccumulator};
pub use auth::{AuthCtx, AuthMethod, AuthState, ProviderAuth};
pub use counter::{HeuristicCounter, TokenCounter};
pub use error::ProviderError;
pub use event::{ModelEvent, StopReason};
pub use provider::{Capabilities, Provider};
pub use orrery_proto::ToolDescriptor;
pub use request::ModelRequest;
