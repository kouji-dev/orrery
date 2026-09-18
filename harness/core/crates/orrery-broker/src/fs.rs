//! Reading and writing, behind a token and under a ceiling.

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt as _;

use crate::error::BrokerError;

/// A write that is all-or-nothing.
///
/// Writes land in a temporary file beside the target and are renamed over it on
/// [`WriteHandle::commit`]. Anything else — an explicit
/// [`WriteHandle::cancel`], a dropped handle, a cancelled turn, a panic — leaves
/// the original exactly as it was and takes the temporary file with it.
#[derive(Debug)]
pub struct WriteHandle {
    target: PathBuf,
    temp: PathBuf,
    file: Option<tokio::fs::File>,
    atomic: bool,
    committed: bool,
}

impl WriteHandle {
    pub(crate) async fn open(target: &Path, atomic: bool) -> Result<Self, BrokerError> {
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| BrokerError::io(parent, e))?;
            }
        }
        let temp = temp_beside(target);
        let path = if atomic { &temp } else { target };
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await
            .map_err(|e| BrokerError::io(path, e))?;
        Ok(Self {
            target: target.to_path_buf(),
            temp,
            file: Some(file),
            atomic,
            committed: false,
        })
    }

    /// Where the bytes will end up.
    #[must_use]
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// The temporary file, while there is one.
    #[must_use]
    pub fn temp_path(&self) -> &Path {
        &self.temp
    }

    /// Append bytes.
    ///
    /// # Errors
    ///
    /// When the write fails, or the handle has already been committed.
    pub async fn write_all(&mut self, bytes: &[u8]) -> Result<(), BrokerError> {
        let Some(file) = self.file.as_mut() else {
            return Err(BrokerError::Cancelled);
        };
        file.write_all(bytes)
            .await
            .map_err(|e| BrokerError::io(&self.target, e))
    }

    /// Make the write visible. Consumes the handle.
    ///
    /// # Errors
    ///
    /// When the flush or the rename fails; the original is untouched either way.
    pub async fn commit(mut self) -> Result<(), BrokerError> {
        if let Some(mut file) = self.file.take() {
            file.flush()
                .await
                .map_err(|e| BrokerError::io(&self.target, e))?;
            file.sync_all()
                .await
                .map_err(|e| BrokerError::io(&self.target, e))?;
            drop(file);
        }
        if self.atomic {
            tokio::fs::rename(&self.temp, &self.target)
                .await
                .map_err(|e| BrokerError::io(&self.target, e))?;
        }
        self.committed = true;
        Ok(())
    }

    /// Throw the write away. Consumes the handle.
    pub async fn cancel(mut self) {
        self.file = None;
        let temp = std::mem::take(&mut self.temp);
        self.committed = true; // nothing left for `Drop` to clean up
        let _ = tokio::fs::remove_file(temp).await;
    }
}

impl Drop for WriteHandle {
    /// A dropped handle is a cancelled write. The temporary file goes with it,
    /// so a cancelled turn leaves nothing behind for the next one to trip over.
    fn drop(&mut self) {
        self.file = None;
        if !self.committed && self.atomic {
            let _ = std::fs::remove_file(&self.temp);
        }
    }
}

fn temp_beside(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        ".orrery-{}.tmp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    target.with_file_name(name)
}
