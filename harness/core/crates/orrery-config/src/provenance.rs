//! Provenance is not an add-on.
//!
//! Every scalar a layer sets is kept with the layer, the file and the line it
//! was written on, and the values a closer layer shadowed are kept beside it —
//! otherwise `config explain` is a lie.

use std::fmt;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use orrery_proto::Layer;

/// Where one value was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    /// Which layer contributed it.
    pub layer: Layer,
    /// The file it was written in.
    pub file: PathBuf,
    /// The line, 1-based. Zero when the span was not recoverable.
    pub line: u32,
}

impl Origin {
    /// A layer, a file and a line.
    #[must_use]
    pub fn new(layer: Layer, file: impl AsRef<Path>, line: u32) -> Self {
        Self {
            layer,
            file: file.as_ref().to_path_buf(),
            line,
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.file.display())
        } else {
            write!(f, "{}:{}", self.file.display(), self.line)
        }
    }
}

/// One value, and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Slot {
    /// The value as written.
    pub value: toml::Value,
    /// Where it was written.
    pub origin: Origin,
}

/// How a key folds across layers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Fold {
    /// The closest layer wins and the rest are shadowed. How **names** resolve.
    Override,
    /// Every layer's contribution stays in force. How **deny** resolves.
    Union,
}

/// One key's slots, winner first.
#[derive(Clone, Debug)]
struct Entry {
    path: Vec<String>,
    fold: Fold,
    slots: Vec<Slot>,
}

/// Every value the layers set, each with its origin and what it shadowed.
#[derive(Clone, Debug, Default)]
pub struct Provenanced {
    entries: IndexMap<String, Entry>,
}

impl Provenanced {
    /// Nothing set yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a value from one layer.
    ///
    /// An [`Fold::Override`] key puts the newest slot first and pushes the
    /// previous winner down into the shadowed list; a [`Fold::Union`] key keeps
    /// them all in the order they arrived.
    pub fn record(&mut self, path: Vec<String>, fold: Fold, slot: Slot) {
        let key = join(&path);
        let entry = self.entries.entry(key).or_insert_with(|| Entry {
            path,
            fold,
            slots: Vec::new(),
        });
        match fold {
            Fold::Override => entry.slots.insert(0, slot),
            Fold::Union => entry.slots.push(slot),
        }
    }

    /// The value in force for a key.
    #[must_use]
    pub fn winner(&self, key: &str) -> Option<&Slot> {
        self.entries.get(key).and_then(|e| e.slots.first())
    }

    /// The values a closer layer shadowed, nearest first.
    #[must_use]
    pub fn shadowed(&self, key: &str) -> &[Slot] {
        self.entries
            .get(key)
            .map_or(&[][..], |e| e.slots.get(1..).unwrap_or(&[]))
    }

    /// Every slot for a key, in force or not.
    #[must_use]
    pub fn all(&self, key: &str) -> &[Slot] {
        self.entries.get(key).map_or(&[][..], |e| &e.slots)
    }

    /// How a key folds.
    #[must_use]
    pub fn fold(&self, key: &str) -> Option<Fold> {
        self.entries.get(key).map(|e| e.fold)
    }

    /// Every key, in the order it was first seen.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Whether anything set this key.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Every string a union key contributed, flattened out of its arrays, each
    /// with the origin of the array it was written in.
    #[must_use]
    pub fn union_strings(&self, key: &str) -> Vec<(String, &Origin)> {
        let mut out = Vec::new();
        for slot in self.all(key) {
            match &slot.value {
                toml::Value::Array(items) => {
                    for item in items {
                        if let Some(s) = item.as_str() {
                            out.push((s.to_owned(), &slot.origin));
                        }
                    }
                }
                toml::Value::String(s) => out.push((s.clone(), &slot.origin)),
                _ => {}
            }
        }
        out
    }

    /// The winning string for a key.
    #[must_use]
    pub fn str(&self, key: &str) -> Option<&str> {
        self.winner(key).and_then(|s| s.value.as_str())
    }

    /// The winning boolean for a key.
    #[must_use]
    pub fn bool(&self, key: &str) -> Option<bool> {
        self.winner(key).and_then(|s| s.value.as_bool())
    }

    /// The winning array of strings for a key.
    #[must_use]
    pub fn strings(&self, key: &str) -> Vec<String> {
        self.winner(key)
            .and_then(|s| s.value.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    /// Every key under a dotted prefix, prefix included.
    #[must_use]
    pub fn keys_under(&self, prefix: &str) -> Vec<&str> {
        let with_dot = format!("{prefix}.");
        self.entries
            .keys()
            .filter(|k| k.starts_with(&with_dot))
            .map(String::as_str)
            .collect()
    }

    /// The effective document: winners only, rebuilt into nested tables so it
    /// can be deserialised **once, at the end**, which is the whole reason the
    /// merge runs over `toml_edit` rather than `serde`.
    #[must_use]
    pub fn effective(&self) -> toml::Value {
        let mut root = toml::value::Table::new();
        for entry in self.entries.values() {
            let value = match entry.fold {
                Fold::Override => match entry.slots.first() {
                    Some(slot) => slot.value.clone(),
                    None => continue,
                },
                Fold::Union => {
                    let mut items = Vec::new();
                    for slot in &entry.slots {
                        match &slot.value {
                            toml::Value::Array(a) => items.extend(a.iter().cloned()),
                            other => items.push(other.clone()),
                        }
                    }
                    toml::Value::Array(items)
                }
            };
            insert_at(&mut root, &entry.path, value);
        }
        toml::Value::Table(root)
    }
}

fn insert_at(table: &mut toml::value::Table, path: &[String], value: toml::Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut cursor = table;
    for segment in parents {
        let next = cursor
            .entry(segment.clone())
            .or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if !next.is_table() {
            *next = toml::Value::Table(toml::value::Table::new());
        }
        cursor = next.as_table_mut().expect("just made it a table");
    }
    cursor.insert(last.clone(), value);
}

/// A dotted key, quoting any segment that contains a dot.
#[must_use]
pub fn join(path: &[String]) -> String {
    path.iter()
        .map(|s| {
            if s.contains('.') {
                format!("\"{s}\"")
            } else {
                s.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}
