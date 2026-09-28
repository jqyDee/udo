//! How model types are written into SQL columns (rusqlite `ToSql` /
//! `FromSql`). Here, not in `model`: the model knows nothing of databases.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

use crate::model::{
    id::NodeId,
    sessions::{SessionError, SessionId, SessionSource},
};

/// Stored as text: `Display` in, `FromStr` out.
macro_rules! sql_as_text {
    ($($ty:ty),* $(,)?) => {$(
        impl ToSql for $ty {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(self.to_string().into())
            }
        }

        impl FromSql for $ty {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                value
                    .as_str()?
                    .parse::<Self>()
                    .map_err(|e| FromSqlError::Other(e.to_string().into()))
            }
        }
    )*};
}

sql_as_text!(SessionId, NodeId, SessionSource);

impl From<rusqlite::Error> for SessionError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Backend(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    /// Write `value` as a parameter, read it back as a column.
    fn round_trip<T: ToSql + FromSql>(value: T) -> T {
        let conn = Connection::open_in_memory().unwrap();
        conn.query_row("SELECT ?1", [&value], |r| r.get(0)).unwrap()
    }

    /// The SQLite type a value is stored as (`text`, `integer`, ...).
    fn sql_type(value: impl ToSql) -> String {
        let conn = Connection::open_in_memory().unwrap();
        conn.query_row("SELECT typeof(?1)", [value], |r| r.get(0))
            .unwrap()
    }

    /// Read a literal SQL value as `T`.
    fn read<T: FromSql>(sql_value: &str) -> rusqlite::Result<T> {
        let conn = Connection::open_in_memory().unwrap();
        conn.query_row(&format!("SELECT {sql_value}"), [], |r| r.get(0))
    }

    #[test]
    fn ids_round_trip_as_text() {
        let session = SessionId::new();
        let node = NodeId::new();

        assert_eq!(round_trip(session), session);
        assert_eq!(round_trip(node), node);
        assert_eq!(sql_type(session), "text"); // readable in the sqlite CLI
    }

    #[test]
    fn source_round_trips_as_its_name() {
        assert_eq!(round_trip(SessionSource::Manual), SessionSource::Manual);
        assert_eq!(
            read::<String>("'manual'").unwrap(),
            SessionSource::Manual.to_string()
        );
    }

    #[test]
    fn unreadable_values_are_errors() {
        assert!(read::<SessionId>("'not a uuid'").is_err());
        assert!(read::<NodeId>("''").is_err());
        assert!(read::<SessionId>("42").is_err()); // not text at all
        assert!(read::<SessionSource>("'emacs'").is_err());
    }
}
