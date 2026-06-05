use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

#[derive(Clone, Copy, Debug)]
pub enum Change {
    Created,
    Updated,
    Deleted,
}

impl Change {
    pub fn as_str(self) -> &'static str {
        match self {
            Change::Created => "created",
            Change::Updated => "updated",
            Change::Deleted => "deleted",
        }
    }
}

pub fn event_name(entity: &str, change: Change) -> String {
    format!("{entity}://{}", change.as_str())
}

pub fn emit_entity<R: Runtime, T: Serialize + Clone>(
    app: &AppHandle<R>,
    entity: &str,
    change: Change,
    payload: T,
) {
    let _ = app.emit(&event_name(entity, change), payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_event_names() {
        assert_eq!(event_name("project", Change::Created), "project://created");
        assert_eq!(event_name("agent", Change::Deleted), "agent://deleted");
    }
}
