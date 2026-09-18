//! The manifest: TOML on disk, [`ExtensionManifest`] in memory.
//!
//! This is *the* contract. Nothing outside it is available to an extension at
//! runtime, and nothing outside it is promised to one. Translation #10 in the
//! plan is the reason the deserialise target and the runtime handle are two
//! different types: what TOML can express and what the host holds are not the
//! same shape, and collapsing them would mean either a `Deserialize` impl on a
//! type holding an `Arc<dyn ExtensionHost>` or a manifest that cannot describe
//! itself.

use std::collections::BTreeMap;
use std::path::Path;

use orrery_proto::{Aspect, Capability, Contribution, ContributionKind, ExtId, LoadStage};
use serde::{Deserialize, Serialize};

/// The major of the extension API this build implements.
///
/// A manifest naming any other major is refused at [`LoadStage::Manifest`]:
/// loading it half-way and discovering the mismatch at the first call would put
/// the failure somewhere a person cannot act on it.
pub const SUPPORTED_API_MAJOR: u64 = 1;

/// `orrery-ext/<major>`.
///
/// Only the major is on the wire. A minor would imply that an extension can
/// declare which *additions* it needs, and additions are exactly what the
/// non-exhaustive types in `orrery-proto` make safe without a version.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiVersion {
    /// The major.
    pub major: u64,
}

impl ApiVersion {
    /// The version this build implements.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            major: SUPPORTED_API_MAJOR,
        }
    }

    /// Whether this build can load an extension written against this version.
    #[must_use]
    pub const fn is_supported(self) -> bool {
        self.major == SUPPORTED_API_MAJOR
    }
}

impl std::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "orrery-ext/{}", self.major)
    }
}

impl std::str::FromStr for ApiVersion {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        let major = s.strip_prefix("orrery-ext/").ok_or(())?;
        major.parse().map(|major| Self { major }).map_err(|_| ())
    }
}

impl Serialize for ApiVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ApiVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = String::deserialize(d)?;
        raw.parse().map_err(|()| {
            D::Error::custom(format!(
                "`{raw}` is not an api version: expected `orrery-ext/<major>`"
            ))
        })
    }
}

/// How an extension's code is reached.
///
/// `native` is not a cheat (translation #14): a first-party crate compiled in
/// behind a cargo feature takes the same manifest, the same dispatch and the
/// same ledger as anything else. The only thing it is missing is the process
/// boundary, and that is stated rather than hidden.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    /// Compiled into the harness behind a cargo feature.
    #[default]
    Native,
    /// A wasm component. Plan 14.
    Wasm,
    /// `node`, JSON-RPC over stdio, the `@orrery/ext` SDK on the far side.
    Node,
    /// `python`, same RPC path.
    Python,
    /// Any other child process that speaks the protocol.
    Process,
}

impl RuntimeKind {
    /// Whether this runtime is served by a child process, and therefore needs a
    /// `[process]` table when the manifest does not imply the argv itself.
    #[must_use]
    pub const fn is_child_process(self) -> bool {
        matches!(
            self,
            RuntimeKind::Node | RuntimeKind::Python | RuntimeKind::Process
        )
    }
}

impl std::fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RuntimeKind::Native => "native",
            RuntimeKind::Wasm => "wasm",
            RuntimeKind::Node => "node",
            RuntimeKind::Python => "python",
            RuntimeKind::Process => "process",
        };
        f.write_str(s)
    }
}

/// The `[process]` table: how to start a child-process extension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessSpec {
    /// The program, relative to the extension's directory or on `PATH`.
    pub command: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// The protocol it speaks. Only `orrery-ext/<major>` is understood.
    #[serde(default = "ApiVersion::current")]
    pub protocol: ApiVersion,
    /// Extra environment, on top of whatever the broker allows through.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// What a `[requires]` entry may say.
///
/// `net = false` asks for nothing; `net = true` asks for the unqualified
/// aspect; a list asks for exactly those names or patterns. The bool form is
/// what stops a manifest having to write `net = ["*"]` and meaning something
/// subtly different from an empty scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Requirement {
    /// The whole aspect, or none of it.
    Unqualified(bool),
    /// These names or patterns.
    Scoped(Vec<String>),
}

impl Requirement {
    /// The capability this asks for, or `None` when it asks for nothing.
    #[must_use]
    pub fn capability(&self, aspect: Aspect) -> Option<Capability> {
        match self {
            Requirement::Unqualified(false) => None,
            Requirement::Unqualified(true) => Some(Capability::all(aspect)),
            Requirement::Scoped(scope) if scope.is_empty() => None,
            Requirement::Scoped(scope) => Some(Capability::scoped(aspect, scope.clone())),
        }
    }
}

/// Generate [`Provides`] and its contribution mapping from one list.
///
/// The TypeScript original used `keyof ExtensionDefinition` so the two could
/// not drift. Rust has no `keyof`, and a `derive` would need a second crate on
/// the dependency graph of a *published* API crate for what is, in the end, one
/// list written once. So: one macro invocation, one list, both halves generated
/// from it. Adding a field without saying what it contributes does not compile.
macro_rules! provides {
    (
        collections { $( $cname:ident : $ckind:expr ),* $(,)? }
        singletons  { $( $sname:ident : $skind:expr ),* $(,)? }
    ) => {
        /// What an extension says it brings.
        ///
        /// Mirrors the extension definition field list exactly. Collections
        /// merge across extensions; singletons resolve by layer precedence and
        /// the loser is named in the ledger.
        #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct Provides {
            $(
                #[doc = concat!("The `", stringify!($cname), "` this extension contributes.")]
                #[serde(default, skip_serializing_if = "Vec::is_empty")]
                pub $cname: Vec<String>,
            )*
            $(
                #[doc = concat!("The `", stringify!($sname), "` this extension claims, if any.")]
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $sname: Option<String>,
            )*
        }

        impl Provides {
            /// Everything this manifest promises, as ledger entries.
            ///
            /// A field whose kind is `None` has no [`ContributionKind`] in
            /// `orrery-proto` yet — see the crate-level note. It still parses
            /// and is still held; it is simply not reportable until the wire
            /// type grows a variant for it.
            #[must_use]
            pub fn contributions(&self) -> Vec<Contribution> {
                let mut out = Vec::new();
                $(
                    if let Some(kind) = $ckind {
                        for name in &self.$cname {
                            out.push(Contribution { kind, name: name.clone() });
                        }
                    }
                )*
                $(
                    if let (Some(kind), Some(name)) = ($skind, self.$sname.as_ref()) {
                        out.push(Contribution { kind, name: name.clone() });
                    }
                )*
                out
            }

            /// Whether this manifest promises anything at all.
            #[must_use]
            pub fn is_empty(&self) -> bool {
                $( self.$cname.is_empty() && )* $( self.$sname.is_none() && )* true
            }

            /// The singleton slots this manifest claims.
            #[must_use]
            pub fn singletons(&self) -> Vec<SingletonSlot> {
                let mut out = Vec::new();
                $(
                    if self.$sname.is_some() {
                        out.push(SingletonSlot::$sname);
                    }
                )*
                out
            }
        }

        /// The slots exactly one extension may hold.
        #[non_exhaustive]
        #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[allow(non_camel_case_types)]
        pub enum SingletonSlot {
            $(
                #[doc = concat!("The `", stringify!($sname), "` slot.")]
                $sname,
            )*
        }

        impl std::fmt::Display for SingletonSlot {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(match self { $( SingletonSlot::$sname => stringify!($sname), )* })
            }
        }
    };
}

provides! {
    collections {
        tools:        Some(ContributionKind::Tool),
        providers:    Some(ContributionKind::Provider),
        // TODO(orrery-proto): `ContributionKind` has no `Agent`, `Workflow`,
        // `Interceptor`, `Lifecycle`, `Permissions` or `Mcp` variant yet. The
        // enum is `#[non_exhaustive]` precisely so those can be added without
        // breaking anyone; until they are, these fields parse and are held but
        // are not reported in the ledger.
        agents:       None,
        workflows:    None,
        interceptors: None,
        lifecycle:    None,
        graders:      Some(ContributionKind::Grader),
        commands:     Some(ContributionKind::Command),
        views:        Some(ContributionKind::View),
        renderers:    Some(ContributionKind::Renderer),
        skills:       Some(ContributionKind::Skill),
        mcp:          None,
    }
    singletons {
        memory:      Some(ContributionKind::Memory),
        session:     Some(ContributionKind::SessionStore),
        permissions: None,
        router:      Some(ContributionKind::Router),
    }
}

/// A manifest that is not a manifest.
///
/// Every variant names the file. An extension that does not load is something a
/// person has to be able to go and fix, and "invalid manifest" with no path is
/// not actionable.
#[non_exhaustive]
#[derive(Clone, Debug, thiserror::Error)]
pub enum ManifestError {
    /// The file could not be read.
    #[error("{file}: {message}")]
    Unreadable {
        /// Which file.
        file: String,
        /// What the OS said.
        message: String,
    },
    /// The file is not TOML, or not this shape of TOML.
    #[error("{file}: {message}")]
    Syntax {
        /// Which file.
        file: String,
        /// What the parser said.
        message: String,
    },
    /// A required key is absent.
    #[error("{file}: missing `{field}`")]
    MissingField {
        /// Which file.
        file: String,
        /// Which key.
        field: &'static str,
    },
    /// The manifest is written against an API major this build does not
    /// implement.
    #[error(
        "{file}: this extension is written against orrery-ext/{found}, \
         and this build implements orrery-ext/{supported}"
    )]
    UnknownApi {
        /// Which file.
        file: String,
        /// What it asked for.
        found: u64,
        /// What there is.
        supported: u64,
    },
    /// The extension id is not an [`ExtId`].
    #[error("{file}: {message}")]
    BadName {
        /// Which file.
        file: String,
        /// What is wrong with it.
        message: String,
    },
    /// `runtime = "process"` with no `[process]` table.
    #[error("{file}: `runtime = \"{runtime}\"` needs a [process] table naming the command to run")]
    ProcessSpecMissing {
        /// Which file.
        file: String,
        /// The runtime that needs one.
        runtime: RuntimeKind,
    },
}

impl ManifestError {
    /// How far loading got. Always [`LoadStage::Manifest`] — that is the point
    /// of the type.
    #[must_use]
    pub const fn stage(&self) -> LoadStage {
        LoadStage::Manifest
    }

    /// Which file the problem is in.
    #[must_use]
    pub fn file(&self) -> &str {
        match self {
            ManifestError::Unreadable { file, .. }
            | ManifestError::Syntax { file, .. }
            | ManifestError::MissingField { file, .. }
            | ManifestError::UnknownApi { file, .. }
            | ManifestError::BadName { file, .. }
            | ManifestError::ProcessSpecMissing { file, .. } => file,
        }
    }
}

/// One extension, as declared.
///
/// # Two spellings, one parser
///
/// The plan writes `api`, `name` and `runtime` inside `[extension]`. The
/// eleven first-party bundles were scaffolded with `api` and `runtime` at the
/// top level and the id spelled `id`. Both are accepted, because a parser that
/// rejected the manifests already in the tree would be a parser nobody could
/// test against the real thing. The `[extension]` spelling is canonical and is
/// what `orrery ext new` writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionManifest {
    /// The API major it is written against.
    pub api: ApiVersion,
    /// Its namespace: the first half of every tool name it contributes.
    pub name: ExtId,
    /// Its own version.
    pub version: semver::Version,
    /// How its code is reached.
    pub runtime: RuntimeKind,
    /// What it promises.
    pub provides: Provides,
    /// What it asks for, by aspect.
    pub requires: BTreeMap<Aspect, Requirement>,
    /// How to start it, for a child-process runtime.
    pub process: Option<ProcessSpec>,
}

impl ExtensionManifest {
    /// Parse a manifest that is already in memory.
    ///
    /// `file` is used only for error messages, and is required for exactly that
    /// reason: there is no way to produce an unattributable manifest error.
    ///
    /// # Errors
    ///
    /// [`ManifestError`], always at [`LoadStage::Manifest`].
    pub fn from_toml_str(src: &str, file: impl Into<String>) -> Result<Self, ManifestError> {
        let file = file.into();
        let raw: RawManifest = toml::from_str(src).map_err(|e| ManifestError::Syntax {
            file: file.clone(),
            message: e.message().to_owned(),
        })?;
        raw.resolve(file)
    }

    /// Read and parse a manifest from disk.
    ///
    /// # Errors
    ///
    /// [`ManifestError`], always at [`LoadStage::Manifest`].
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();
        let file = path.display().to_string();
        let src = std::fs::read_to_string(path).map_err(|e| ManifestError::Unreadable {
            file: file.clone(),
            message: e.to_string(),
        })?;
        Self::from_toml_str(&src, file)
    }

    /// What this manifest asks for, as capabilities, in a stable order.
    ///
    /// This is the *ask*, not the grant. What it actually gets is whatever
    /// policy hands the loader, and the difference between the two is what
    /// turns a load into [`LoadOutcome::Degraded`](orrery_proto::LoadOutcome).
    #[must_use]
    pub fn capabilities(&self) -> Vec<Capability> {
        self.requires
            .iter()
            .filter_map(|(aspect, req)| req.capability(*aspect))
            .collect()
    }

    /// Everything this manifest promises, as ledger entries.
    #[must_use]
    pub fn contributions(&self) -> Vec<Contribution> {
        self.provides.contributions()
    }
}

/// The deserialise target. Translation #10: this is not the runtime type.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    /// The scaffolded spelling.
    api: Option<ApiVersion>,
    /// The scaffolded spelling.
    runtime: Option<RuntimeKind>,
    extension: RawExtension,
    #[serde(default)]
    provides: Provides,
    #[serde(default)]
    requires: BTreeMap<Aspect, Requirement>,
    #[serde(default)]
    process: Option<ProcessSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExtension {
    api: Option<ApiVersion>,
    runtime: Option<RuntimeKind>,
    /// The canonical spelling.
    name: Option<String>,
    /// The scaffolded spelling.
    id: Option<String>,
    version: semver::Version,
}

impl RawManifest {
    fn resolve(self, file: String) -> Result<ExtensionManifest, ManifestError> {
        let api = self
            .extension
            .api
            .or(self.api)
            .ok_or_else(|| ManifestError::MissingField {
                file: file.clone(),
                field: "api",
            })?;
        if !api.is_supported() {
            return Err(ManifestError::UnknownApi {
                file,
                found: api.major,
                supported: SUPPORTED_API_MAJOR,
            });
        }

        let name = self.extension.name.or(self.extension.id).ok_or_else(|| {
            ManifestError::MissingField {
                file: file.clone(),
                field: "extension.name",
            }
        })?;
        let name = ExtId::new(name).map_err(|e| ManifestError::BadName {
            file: file.clone(),
            message: e.to_string(),
        })?;

        let runtime = self.extension.runtime.or(self.runtime).unwrap_or_default();
        if runtime == RuntimeKind::Process && self.process.is_none() {
            return Err(ManifestError::ProcessSpecMissing { file, runtime });
        }

        Ok(ExtensionManifest {
            api,
            name,
            version: self.extension.version,
            runtime,
            provides: self.provides,
            requires: self.requires,
            process: self.process,
        })
    }
}
