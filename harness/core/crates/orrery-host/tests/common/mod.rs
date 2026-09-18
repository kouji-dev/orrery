//! A compiled-in extension a test can steer.
//!
//! Each test binary uses a different slice of it.

#![allow(dead_code)]

use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::{CallCtx, HostError, NativeExtension, ToolDef};
use orrery_proto::{Aspect, Outcome};
use parking_lot::Mutex;
use serde_json::Value;

/// How the extension's tools behave once they are called.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Answer immediately with the input echoed back.
    Echo,
    /// Wait for the cancel token, then settle `Cancelled`. A well-behaved
    /// extension.
    AwaitCancel,
    /// Ignore the cancel token entirely. The one the grace window exists for.
    IgnoreCancel,
}

/// A native extension whose manifest, tools and behaviour a test picks.
pub struct TestExt {
    manifest: String,
    tools: Vec<ToolDef>,
    mode: Mode,
    /// Every tool name this extension was actually asked to run, in order.
    pub calls: Arc<Mutex<Vec<String>>>,
}

impl TestExt {
    /// An extension with this manifest and these tools, answering immediately.
    pub fn new(manifest: impl Into<String>, tools: Vec<ToolDef>) -> Self {
        Self {
            manifest: manifest.into(),
            tools,
            mode: Mode::Echo,
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Change how its tools behave.
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// A handle on what it was asked to do.
    pub fn calls(&self) -> Arc<Mutex<Vec<String>>> {
        self.calls.clone()
    }
}

#[async_trait]
impl NativeExtension for TestExt {
    fn manifest(&self) -> &str {
        &self.manifest
    }

    fn tools(&self) -> Vec<ToolDef> {
        self.tools.clone()
    }

    async fn call(&self, tool: &str, input: Value, ctx: &CallCtx) -> Result<Outcome, HostError> {
        self.calls.lock().push(tool.to_owned());
        match self.mode {
            Mode::Echo => Ok(Outcome::Ok {
                surface: None,
                value: Some(input),
            }),
            Mode::AwaitCancel => {
                ctx.cancel.cancelled().await;
                Ok(Outcome::Cancelled {
                    reason: orrery_proto::CancelReason::Shutdown,
                })
            }
            Mode::IgnoreCancel => {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                Ok(Outcome::ok())
            }
        }
    }
}

/// A manifest for an extension called `name` providing `tools`.
pub fn manifest(name: &str, tools: &[&str], requires: &str) -> String {
    let tools = tools
        .iter()
        .map(|t| format!("\"{t}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "[extension]\n\
         api = \"orrery-ext/1\"\n\
         name = \"{name}\"\n\
         version = \"0.1.0\"\n\
         runtime = \"native\"\n\
         \n\
         [provides]\n\
         tools = [{tools}]\n\
         \n\
         [requires]\n\
         {requires}\n"
    )
}

/// A tool that needs nothing.
pub fn tool(name: &str) -> ToolDef {
    ToolDef::new(name).described("a tool")
}

/// A tool that cannot work without one aspect.
pub fn tool_requiring(name: &str, aspect: Aspect) -> ToolDef {
    tool(name).requiring([aspect])
}
