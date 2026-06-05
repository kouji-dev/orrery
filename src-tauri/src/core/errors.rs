use thiserror::Error;

// ---- domain error: DB ----
#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error), // rusqlite::Error -> DbError automatically
    #[error("not found: {0}")]
    NotFound(String),
    #[error("insert error: {0}")]
    InsertError(String),
}

// ---- domain error: X (e.g. Project) ----
#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("invalid project: {0}")]
    Invalid(String),
    #[error("required field missing: {0}")]
    Required(String),
    #[error("project already exists: {0}")]
    Exists(String),
    #[error("not found: {0}")]
    NotFound(String),
}

// ---- base error: everything rolls up into this ----
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Db(#[from] DbError), // DbError -> AppError
    #[error(transparent)]
    Project(#[from] ProjectError), // ProjectError -> AppError
    #[error("{0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl serde::Serialize for AppError {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (kind, message) = match self {
            AppError::Project(ProjectError::NotFound(m)) => ("notFound", m.clone()),
            AppError::Project(e) => ("project", e.to_string()),
            AppError::Db(e) => ("db", e.to_string()),
            AppError::Other(m) => ("other", m.clone()),
        };
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", kind)?;
        st.serialize_field("message", &message)?;
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_kind_and_message() {
        let err = AppError::Project(ProjectError::NotFound("p1".into()));
        let json = serde_json::to_string(&err).unwrap();
        assert_eq!(json, r#"{"kind":"notFound","message":"p1"}"#);
    }

    #[test]
    fn db_error_serializes_with_db_kind() {
        let err = AppError::Other("boom".into());
        let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&err).unwrap()).unwrap();
        assert_eq!(v["kind"], "other");
        assert_eq!(v["message"], "boom");
    }
}
