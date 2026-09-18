//! `orrery install <source>`: one positional argument, seven forms.
//!
//! # The ambiguity rules, pinned
//!
//! - A **bare word** (`buildgraph`, `buildgraph@1.2.0`) is the **registry**, and
//!   nothing else. It never falls through to crates.io: "not in the index" is an
//!   error naming the index, not a guess at another host. A silent fallback is
//!   how a typo installs somebody else's package.
//! - A leading `./` or `../`, or a `file:` prefix, is a **path**.
//! - Everything else needs its prefix: `github:`, `crate:`, `npm:`, or a URL
//!   with a scheme.
//!
//! # Only one of the seven is verified
//!
//! Registry installs are signature-checked and hash-pinned. The other six forms
//! are not, and [`Source::is_registry`] is what [`crate::pin`] asks. The rule,
//! written where it is enforced: **the registry is how you trust an extension;
//! the other sources are how you try one.**

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use crate::error::RegistryError;

/// What a git reference is, and therefore whether the install is reproducible.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitRef {
    /// Whatever the remote's default branch is right now.
    Default,
    /// A branch or a tag: mutable, so the install is not reproducible from the
    /// source string alone.
    Named(String),
    /// A full 40-character commit sha: the only reproducible form.
    Sha(String),
}

impl GitRef {
    /// Whether the source string alone pins the bytes.
    #[must_use]
    pub fn is_reproducible(&self) -> bool {
        matches!(self, GitRef::Sha(_))
    }

    /// What to hand `git` as the thing to check out.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            GitRef::Default => "HEAD",
            GitRef::Named(s) | GitRef::Sha(s) => s,
        }
    }
}

impl fmt::Display for GitRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitRef::Default => f.write_str("HEAD"),
            GitRef::Named(s) | GitRef::Sha(s) => f.write_str(s),
        }
    }
}

/// One of the seven install sources.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// The signed registry index. The default, and the only verified path.
    Registry {
        /// The extension id.
        name: String,
        /// The pinned version, when one was named.
        version: Option<semver::Version>,
    },
    /// A git repository.
    Git {
        /// The URL to clone. `github:o/r` is expanded here, once, so nothing
        /// downstream has to know about shorthands.
        url: String,
        /// What to check out.
        reference: GitRef,
        /// Whether it was written as a `github:` shorthand, which is what the
        /// audit record shows the person typing.
        shorthand: Option<String>,
    },
    /// A crates.io crate.
    Crate {
        /// The crate name.
        name: String,
        /// The version, when one was named.
        version: Option<semver::Version>,
    },
    /// An npm package.
    Npm {
        /// The package name, scope included.
        name: String,
        /// The version, when one was named.
        version: Option<String>,
    },
    /// A directory on this machine.
    Path {
        /// Where.
        path: PathBuf,
    },
}

impl Source {
    /// Whether this is the one verified path.
    #[must_use]
    pub fn is_registry(&self) -> bool {
        matches!(self, Source::Registry { .. })
    }

    /// The name this install will be known by, before a manifest is read.
    ///
    /// For a path or a git URL there is no name until the manifest is parsed,
    /// so this is the last path segment as a first guess; [`crate::install`]
    /// replaces it with the manifest's `name` once there is one.
    #[must_use]
    pub fn provisional_id(&self) -> String {
        match self {
            Source::Registry { name, .. } | Source::Crate { name, .. } => {
                name.trim_start_matches("orrery-ext-").to_owned()
            }
            Source::Npm { name, .. } => name
                .rsplit('/')
                .next()
                .unwrap_or(name)
                .trim_start_matches("orrery-ext-")
                .to_owned(),
            Source::Git { url, .. } => url
                .trim_end_matches('/')
                .trim_end_matches(".git")
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or("extension")
                .to_owned(),
            Source::Path { path } => path
                .file_name()
                .map_or_else(|| "extension".to_owned(), |n| n.to_string_lossy().into_owned()),
        }
    }

    /// A short label for the ledger and the audit stream.
    #[must_use]
    pub fn label(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Registry { name, version } => match version {
                Some(v) => write!(f, "{name}@{v}"),
                None => f.write_str(name),
            },
            Source::Git {
                url,
                reference,
                shorthand,
            } => {
                let base = shorthand.as_deref().unwrap_or(url);
                match reference {
                    GitRef::Default => f.write_str(base),
                    r => write!(f, "{base}#{r}"),
                }
            }
            Source::Crate { name, version } => match version {
                Some(v) => write!(f, "crate:{name}@{v}"),
                None => write!(f, "crate:{name}"),
            },
            Source::Npm { name, version } => match version {
                Some(v) => write!(f, "npm:{name}@{v}"),
                None => write!(f, "npm:{name}"),
            },
            Source::Path { path } => write!(f, "{}", path.display()),
        }
    }
}

impl FromStr for Source {
    type Err = RegistryError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let input = raw.trim();
        let bad = |message: &str| RegistryError::SourceSyntax {
            input: input.to_owned(),
            message: message.to_owned(),
        };
        if input.is_empty() {
            return Err(bad("it is empty"));
        }

        // A path, said two ways.
        if let Some(rest) = input.strip_prefix("file:") {
            let rest = rest.strip_prefix("//").unwrap_or(rest);
            return Ok(Source::Path {
                path: PathBuf::from(rest),
            });
        }
        if input.starts_with("./")
            || input.starts_with("../")
            || input.starts_with(".\\")
            || input.starts_with("..\\")
        {
            return Ok(Source::Path {
                path: PathBuf::from(input),
            });
        }

        if let Some(rest) = input.strip_prefix("github:") {
            let (repo, reference) = split_ref(rest);
            if repo.split('/').filter(|s| !s.is_empty()).count() != 2 {
                return Err(bad("a `github:` source is `github:owner/repo`"));
            }
            return Ok(Source::Git {
                url: format!("https://github.com/{repo}.git"),
                reference,
                shorthand: Some(format!("github:{rest}")),
            });
        }

        if let Some(rest) = input.strip_prefix("crate:") {
            let (name, version) = split_version(rest);
            if name.is_empty() {
                return Err(bad("a `crate:` source needs a crate name"));
            }
            let version = match version {
                None => None,
                Some(v) => Some(
                    semver::Version::parse(v)
                        .map_err(|e| bad(&format!("`{v}` is not a version: {e}")))?,
                ),
            };
            return Ok(Source::Crate {
                name: name.to_owned(),
                version,
            });
        }

        if let Some(rest) = input.strip_prefix("npm:") {
            // `@scope/name@1.2.3` — the leading `@` is part of the scope, so the
            // version separator is the *last* `@`, and only when it is not at 0.
            let (name, version) = match rest.rfind('@') {
                Some(0) | None => (rest, None),
                Some(i) => (&rest[..i], Some(rest[i + 1..].to_owned())),
            };
            if name.is_empty() {
                return Err(bad("an `npm:` source needs a package name"));
            }
            return Ok(Source::Npm {
                name: name.to_owned(),
                version,
            });
        }

        // Any git URL: a scheme we recognise, or scp-style `git@host:path`.
        let is_url = ["https://", "http://", "git://", "ssh://"]
            .iter()
            .any(|p| input.starts_with(p));
        if is_url || input.starts_with("git@") {
            let (url, reference) = split_ref(input);
            return Ok(Source::Git {
                url: url.to_owned(),
                reference,
                shorthand: None,
            });
        }

        // A bare word is the registry, and only the registry.
        let (name, version) = split_version(input);
        if !is_bare_name(name) {
            return Err(bad(
                "it is neither a bare registry name, a `./` path, nor a prefixed source \
                 (`github:`, `crate:`, `npm:`, `file:`, or a URL)",
            ));
        }
        let version = match version {
            None => None,
            Some(v) => Some(
                semver::Version::parse(v)
                    .map_err(|e| bad(&format!("`{v}` is not an exact version: {e}")))?,
            ),
        };
        Ok(Source::Registry {
            name: name.to_owned(),
            version,
        })
    }
}

/// Split a trailing `#ref`, classifying it as a sha or a name.
fn split_ref(input: &str) -> (&str, GitRef) {
    match input.split_once('#') {
        None => (input, GitRef::Default),
        Some((head, "")) => (head, GitRef::Default),
        Some((head, r)) => {
            let reference = if r.len() == 40 && r.bytes().all(|b| b.is_ascii_hexdigit()) {
                GitRef::Sha(r.to_ascii_lowercase())
            } else {
                GitRef::Named(r.to_owned())
            };
            (head, reference)
        }
    }
}

/// Split a trailing `@version`.
fn split_version(input: &str) -> (&str, Option<&str>) {
    match input.split_once('@') {
        None => (input, None),
        Some((name, "")) => (name, None),
        Some((name, v)) => (name, Some(v)),
    }
}

/// The shape a registry name has: the same one [`orrery_proto::ExtId`] allows,
/// checked here so the error says "not a source" rather than "not an id".
fn is_bare_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

/// How a git source is turned into bytes on disk.
///
/// A trait because plan 15 says these tests run **offline**: the implementation
/// shells out to the system git, and a test points it at a bare repository in a
/// tempdir. There is no code path here that knows what GitHub is beyond
/// expanding a shorthand into a URL.
pub trait GitRunner: Send + Sync {
    /// Clone `url` at `reference` into `dest`, and return the commit sha that
    /// was actually checked out.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Git`] with whatever git said.
    fn clone_at(
        &self,
        url: &str,
        reference: &GitRef,
        dest: &std::path::Path,
    ) -> Result<String, RegistryError>;
}

/// The system `git`, reached the way the ADE already decided to reach it.
///
/// gitoxide has no network features in this tree, so fetch, clone and pull go
/// through the installed git — the same reasoning, and the same conclusion, as
/// the ADE's git backend.
#[derive(Copy, Clone, Debug, Default)]
pub struct SystemGit;

impl GitRunner for SystemGit {
    fn clone_at(
        &self,
        url: &str,
        reference: &GitRef,
        dest: &std::path::Path,
    ) -> Result<String, RegistryError> {
        // Tags come along: a `#v1.2` install has to be able to check one out,
        // and `--no-tags` would make the commonest git source form fail.
        run_git(&["clone", "--quiet", url, &dest.to_string_lossy()], None)?;
        if !matches!(reference, GitRef::Default) {
            run_git(&["checkout", "--quiet", reference.as_str()], Some(dest))?;
        }
        let sha = run_git(&["rev-parse", "HEAD"], Some(dest))?;
        Ok(sha.trim().to_owned())
    }
}

fn run_git(args: &[&str], cwd: Option<&std::path::Path>) -> Result<String, RegistryError> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd.output().map_err(|e| RegistryError::Git {
        args: args.join(" "),
        message: e.to_string(),
    })?;
    if !out.status.success() {
        return Err(RegistryError::Git {
            args: args.join(" "),
            message: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
