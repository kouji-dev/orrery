//! The five layers: where they live, and reading them into spanned documents.

use std::path::{Path, PathBuf};

use orrery_proto::Layer;

use crate::error::ConfigError;

/// The file name every layer uses under its directory.
pub const CONFIG_FILE: &str = "config.toml";
/// The directory a workspace or a project keeps its configuration in.
pub const CONFIG_DIR: &str = ".orrery";

/// One config file, read.
#[derive(Clone, Debug)]
pub struct LayerFile {
    /// Which layer it is.
    pub layer: Layer,
    /// Where it was read from.
    pub path: PathBuf,
    /// Its text, kept so every span can still be turned into a line.
    pub text: String,
    /// How deep a project file is: 0 for everything else, larger the closer to
    /// the working directory. Nested projects resolve closest-first on this.
    pub depth: u32,
}

impl LayerFile {
    /// A layer file from text, for tests and for defaults built in code.
    #[must_use]
    pub fn new(layer: Layer, path: impl AsRef<Path>, text: impl Into<String>) -> Self {
        Self {
            layer,
            path: path.as_ref().to_path_buf(),
            text: text.into(),
            depth: 0,
        }
    }

    /// Read a file, or `Ok(None)` when it is simply not there.
    ///
    /// # Errors
    ///
    /// When the file exists and cannot be read.
    pub fn read(layer: Layer, path: impl AsRef<Path>) -> Result<Option<Self>, ConfigError> {
        let path = path.as_ref();
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(Self::new(layer, path, text))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ConfigError::Io {
                file: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Parse it, keeping every span.
    ///
    /// # Errors
    ///
    /// When it is not valid TOML. The error names the file and the line.
    pub fn document(&self) -> Result<toml_edit::ImDocument<&str>, ConfigError> {
        toml_edit::ImDocument::parse(self.text.as_str()).map_err(|e| ConfigError::Syntax {
            file: self.path.clone(),
            line: e.span().map_or(0, |s| line_at(&self.text, s.start)),
            message: e.message().to_owned(),
        })
    }
}

/// The 1-based line a byte offset falls on.
#[must_use]
pub fn line_at(text: &str, offset: usize) -> u32 {
    let end = offset.min(text.len());
    u32::try_from(text[..end].bytes().filter(|b| *b == b'\n').count() + 1).unwrap_or(0)
}

/// The 1-based line a `toml` deserialise error points at, or 0 when it points
/// at nothing.
///
/// A `:0` in a message is the shape this crate keeps trying to be rid of: it
/// tells a person to look at a line that does not exist. Every `toml::de::Error`
/// that has a span can name a real line, and this is how.
#[must_use]
pub fn line_of_toml(text: &str, error: &toml::de::Error) -> u32 {
    error.span().map_or(0, |span| line_at(text, span.start))
}

/// Where each layer's file lives.
///
/// Built either from the OS defaults or, in tests and in sandboxes,
/// [`ConfigPaths::sandboxed`], which looks at nothing outside the directories
/// it is handed.
#[derive(Clone, Debug)]
pub struct ConfigPaths {
    /// The OS-managed file. IT writes it; nothing else may.
    pub managed: Option<PathBuf>,
    /// The organisation layer, as a **local file**.
    ///
    /// Open question 1, decided: phase 5 ships the layer loadable from a local
    /// path only. Signed fetch from the registry is plan 15's, and this field
    /// is where it will land.
    pub org: Option<PathBuf>,
    /// The user's own directory, `~/.orrery`. Holds `config.toml` and the trust
    /// store, and is never inside a workspace.
    pub user_dir: Option<PathBuf>,
    /// The workspace root.
    pub workspace_root: PathBuf,
    /// Where the session was started, which decides which nested projects apply.
    pub cwd: PathBuf,
}

impl ConfigPaths {
    /// The OS defaults for a workspace.
    #[must_use]
    pub fn for_workspace(root: impl AsRef<Path>) -> Self {
        let root = canonical(root.as_ref());
        Self {
            managed: Some(managed_path()),
            org: user_dir().map(|d| d.join("org.toml")),
            user_dir: user_dir(),
            cwd: root.clone(),
            workspace_root: root,
        }
    }

    /// Paths that touch nothing outside what they are handed: no
    /// `%ProgramData%`, no real home. What every test uses.
    #[must_use]
    pub fn sandboxed(root: impl AsRef<Path>, user_dir: impl AsRef<Path>) -> Self {
        let root = canonical(root.as_ref());
        Self {
            managed: None,
            org: None,
            user_dir: Some(user_dir.as_ref().to_path_buf()),
            cwd: root.clone(),
            workspace_root: root,
        }
    }

    /// Start the session somewhere below the root.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl AsRef<Path>) -> Self {
        self.cwd = canonical(cwd.as_ref());
        self
    }

    /// Point the managed layer at a file.
    #[must_use]
    pub fn with_managed(mut self, path: impl AsRef<Path>) -> Self {
        self.managed = Some(path.as_ref().to_path_buf());
        self
    }

    /// Point the organisation layer at a local file.
    #[must_use]
    pub fn with_org(mut self, path: impl AsRef<Path>) -> Self {
        self.org = Some(path.as_ref().to_path_buf());
        self
    }

    /// The user's config file.
    #[must_use]
    pub fn user_file(&self) -> Option<PathBuf> {
        self.user_dir.as_ref().map(|d| d.join(CONFIG_FILE))
    }

    /// The workspace's config file.
    #[must_use]
    pub fn workspace_file(&self) -> PathBuf {
        self.workspace_root.join(CONFIG_DIR).join(CONFIG_FILE)
    }

    /// Every nested project config between the workspace root — exclusive — and
    /// the working directory, **farthest first**, so the closest is merged last.
    #[must_use]
    pub fn project_files(&self) -> Vec<PathBuf> {
        let mut chain = Vec::new();
        let mut cursor = Some(self.cwd.as_path());
        while let Some(dir) = cursor {
            if dir == self.workspace_root {
                break;
            }
            chain.push(dir.to_path_buf());
            cursor = dir.parent();
        }
        chain.reverse();
        chain
            .into_iter()
            .map(|d| d.join(CONFIG_DIR).join(CONFIG_FILE))
            .collect()
    }

    /// The layers that need no trust decision: managed, organisation and user.
    ///
    /// Step 1 of the startup order, and the only thing the trust decision is
    /// ever allowed to see.
    ///
    /// # Errors
    ///
    /// When a file exists and cannot be read.
    pub fn collect_base(&self) -> Result<Vec<LayerFile>, ConfigError> {
        let mut out = Vec::new();
        for (layer, path) in [
            (Layer::Managed, self.managed.clone()),
            (Layer::Org, self.org.clone()),
            (Layer::User, self.user_file()),
        ] {
            if let Some(path) = path {
                if let Some(file) = LayerFile::read(layer, &path)? {
                    out.push(file);
                }
            }
        }
        Ok(out)
    }

    /// The workspace and project layers, farthest first.
    ///
    /// # Errors
    ///
    /// When a file exists and cannot be read.
    pub fn collect_local(&self) -> Result<Vec<LayerFile>, ConfigError> {
        let mut out = Vec::new();
        if let Some(file) = LayerFile::read(Layer::Workspace, self.workspace_file())? {
            out.push(file);
        }
        for (i, path) in self.project_files().into_iter().enumerate() {
            if let Some(mut file) = LayerFile::read(Layer::Project, &path)? {
                file.depth = u32::try_from(i + 1).unwrap_or(u32::MAX);
                out.push(file);
            }
        }
        Ok(out)
    }

    /// Every layer, in merge order. Only for callers that have already decided
    /// the project is trusted — [`crate::resolve`] is the one that decides.
    ///
    /// # Errors
    ///
    /// When a file exists and cannot be read.
    pub fn collect(&self) -> Result<Vec<LayerFile>, ConfigError> {
        let mut out = self.collect_base()?;
        out.extend(self.collect_local()?);
        Ok(out)
    }
}

/// The OS-managed path for this platform.
#[must_use]
pub fn managed_path() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
        base.join("Orrery").join("managed.toml")
    }
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/Orrery/managed.toml")
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        PathBuf::from("/etc/orrery/managed.toml")
    }
}

/// `~/.orrery`, when a home directory is known.
#[must_use]
pub fn user_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    Some(home.join(CONFIG_DIR))
}

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
