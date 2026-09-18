//! Opaque ids, and the one id that is not a uuid.
//!
//! Every id in the protocol is a newtype over a uuid **v7**, not v4 and not a
//! ulid: v7 is already in the ADE's dependency tree and it is time-sortable, so
//! the primary key *is* the insertion order the turn tree wants. Every id
//! serialises as a plain JSON string, which is also what keeps them cheap in
//! CBOR and readable in a fixture.

use std::fmt;
use std::str::FromStr;

use serde::de::{Deserializer, Error as _};
use serde::{Deserialize, Serialize, Serializer};

/// An id or namespace that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    /// The string is not a uuid.
    #[error("`{value}` is not a valid {kind}: expected a uuid")]
    NotAUuid {
        /// The id type that rejected the string.
        kind: &'static str,
        /// The offending string.
        value: String,
    },
    /// The string is not a valid [`ExtId`].
    #[error(
        "`{value}` is not a valid extension id: expected [a-z0-9-]+, \
         with `.` allowed only in the `mcp.<name>` form"
    )]
    NotAnExtId {
        /// The offending string.
        value: String,
    },
}

macro_rules! opaque_id {
    ($(#[$outer:meta])* $name:ident) => {
        $(#[$outer])*
        ///
        /// A uuid v7. Serialises as a plain string.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(uuid::Uuid);

        impl $name {
            /// A fresh, time-sortable id.
            #[must_use]
            pub fn new() -> Self {
                Self(uuid::Uuid::now_v7())
            }

            /// Wrap an existing uuid, for stores that already hold one.
            #[must_use]
            pub const fn from_uuid(uuid: uuid::Uuid) -> Self {
                Self(uuid)
            }

            /// The uuid underneath.
            #[must_use]
            pub const fn as_uuid(&self) -> &uuid::Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                uuid::Uuid::parse_str(s)
                    .map(Self)
                    .map_err(|_| IdError::NotAUuid {
                        kind: stringify!($name),
                        value: s.to_owned(),
                    })
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(d)?;
                raw.parse().map_err(D::Error::custom)
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> String {
                stringify!($name).to_owned()
            }

            fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
                schemars::schema::SchemaObject {
                    instance_type: Some(schemars::schema::InstanceType::String.into()),
                    format: Some("uuid".to_owned()),
                    ..Default::default()
                }
                .into()
            }
        }
    };
}

opaque_id!(
    /// One conversation, with its whole turn tree.
    SessionId
);
opaque_id!(
    /// One turn: an input and everything the kernel did about it.
    TurnId
);
opaque_id!(
    /// One line through the turn tree.
    BranchId
);
opaque_id!(
    /// One tool call.
    CallId
);
opaque_id!(
    /// One policy rule, so a denial can name what denied it.
    RuleId
);
opaque_id!(
    /// One surface, so a patch can address it.
    SurfaceId
);
opaque_id!(
    /// One consent prompt, so an answer can be matched to its question.
    PromptId
);
opaque_id!(
    /// One run of the eval runner.
    RunId
);
opaque_id!(
    /// One request, so a response can be matched to it.
    ReqId
);

/// Per-session monotonic ordering.
///
/// Deliberately **not** a uuid: a client detects a gap in an event stream by
/// arithmetic, which needs a number.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Seq(pub u64);

impl Seq {
    /// The sequence number after this one.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl fmt::Display for Seq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl schemars::JsonSchema for Seq {
    fn schema_name() -> String {
        "Seq".to_owned()
    }

    fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        schemars::schema::SchemaObject {
            instance_type: Some(schemars::schema::InstanceType::Integer.into()),
            format: Some("uint64".to_owned()),
            ..Default::default()
        }
        .into()
    }
}

/// An extension namespace: `buildgraph`, `builtin`, `mcp.jira`.
///
/// Validated on construction, because it is half of every tool name and a
/// namespace with a stray dot in it would make [`ToolRef`](crate::ToolRef)
/// parsing ambiguous. The grammar is `[a-z0-9-]+`, with one exception: an MCP
/// server's namespace is `mcp.<name>` — exactly one dot, at the front.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExtId(String);

impl ExtId {
    /// Validate and wrap a namespace.
    ///
    /// # Errors
    ///
    /// [`IdError::NotAnExtId`] when the string is not of the documented shape.
    pub fn new(raw: impl Into<String>) -> Result<Self, IdError> {
        let raw = raw.into();
        if Self::is_valid(&raw) {
            Ok(Self(raw))
        } else {
            Err(IdError::NotAnExtId { value: raw })
        }
    }

    fn is_valid(raw: &str) -> bool {
        let body = match raw.split_once('.') {
            None => raw,
            // `.` only in the `mcp.<name>` form, and only once.
            Some(("mcp", rest)) => rest,
            Some(_) => return false,
        };
        Self::is_segment(body)
    }

    fn is_segment(s: &str) -> bool {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }

    /// The namespace as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when this namespace names an MCP server rather than a first-party
    /// or community extension.
    #[must_use]
    pub fn is_mcp(&self) -> bool {
        self.0.starts_with("mcp.")
    }
}

impl fmt::Display for ExtId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ExtId {
    type Err = IdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Serialize for ExtId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ExtId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Self::new(raw).map_err(D::Error::custom)
    }
}

impl schemars::JsonSchema for ExtId {
    fn schema_name() -> String {
        "ExtId".to_owned()
    }

    fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        schemars::schema::SchemaObject {
            instance_type: Some(schemars::schema::InstanceType::String.into()),
            string: Some(Box::new(schemars::schema::StringValidation {
                pattern: Some("^(mcp\\.)?[a-z0-9-]+$".to_owned()),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into()
    }
}

/// Where in a session something happened: the session, the branch through its
/// turn tree, and optionally the turn itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionRef {
    /// The conversation.
    pub session: SessionId,
    /// The line through its turn tree.
    pub branch: BranchId,
    /// The turn, when one is in play.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<TurnId>,
}
