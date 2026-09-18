//! What is being asked for, in the shape a rule can match.

use std::collections::BTreeMap;

use orrery_proto::{Aspect, CallId};

use crate::parse::aspect_word;

/// One thing a subject wants to do, before anything has decided about it.
///
/// Deliberately flat and owned: [`crate::PolicyEngine::check`] is a sync, pure
/// `fn`, so it can never fetch anything it was not handed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingCall {
    /// The call this belongs to, so a token can be tied to it and revoked with it.
    pub call: CallId,
    /// What is being asked for.
    pub aspect: Aspect,
    /// The main target: a tool id, a path, a host, a command, a memory scope.
    pub target: String,
    /// The tool's own specifier, when there is one — the command line of a
    /// `shell.exec`, say.
    pub specifier: Option<String>,
    /// Named inputs a `param:` term can match.
    pub params: BTreeMap<String, String>,
}

impl PendingCall {
    /// A call with an aspect and a target.
    #[must_use]
    pub fn new(aspect: Aspect, target: impl Into<String>) -> Self {
        Self {
            call: CallId::new(),
            aspect,
            target: target.into(),
            specifier: None,
            params: BTreeMap::new(),
        }
    }

    /// Tie this to an existing call, so its token is revoked with the call.
    #[must_use]
    pub fn in_call(mut self, call: CallId) -> Self {
        self.call = call;
        self
    }

    /// Add the tool's own specifier.
    #[must_use]
    pub fn with_specifier(mut self, specifier: impl Into<String>) -> Self {
        self.specifier = Some(specifier.into());
        self
    }

    /// Add a named input.
    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// Calling a namespaced tool.
    #[must_use]
    pub fn tool(id: impl Into<String>) -> Self {
        Self::new(Aspect::Tool, id)
    }

    /// Reading a path.
    #[must_use]
    pub fn read(path: impl Into<String>) -> Self {
        Self::new(Aspect::Read, path)
    }

    /// Writing a path.
    #[must_use]
    pub fn write(path: impl Into<String>) -> Self {
        Self::new(Aspect::Write, path)
    }

    /// Creating a process.
    #[must_use]
    pub fn spawn(command: impl Into<String>) -> Self {
        Self::new(Aspect::Spawn, command)
    }

    /// Reaching a host.
    #[must_use]
    pub fn net(domain: impl Into<String>) -> Self {
        Self::new(Aspect::Net, domain)
    }

    /// Using a named credential. The **name**: there is never a value here.
    #[must_use]
    pub fn creds(name: impl Into<String>) -> Self {
        Self::new(Aspect::Creds, name)
    }

    /// The rule-grammar form, which is what an audit record and an explanation
    /// both print: `write(./src/main.rs)`.
    #[must_use]
    pub fn match_text(&self) -> String {
        let word = aspect_word(self.aspect);
        match &self.specifier {
            Some(spec) => format!("{word}({}: {spec})", self.target),
            None => format!("{word}({})", self.target),
        }
    }
}

impl std::fmt::Display for PendingCall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.match_text())
    }
}
