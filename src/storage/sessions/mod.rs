#[cfg(test)]
mod contract;
#[cfg(test)]
mod differential;
pub mod memory;
mod rules;
pub mod sqlite;

use memory::MemorySessions;
use sqlite::SqliteSessions;

use crate::model::{
    sessions::{
        Owner, Session, SessionError, SessionId, SessionPatch, SessionQuery, SessionSource,
        SessionStore, TaskRef,
    },
    time::Time,
};

/// The configured session backend. `SessionStore` has `async fn`s, so it is
/// not `dyn`-compatible: this enum picks the backend at runtime instead. A
/// new backend = one variant + one arm in `dispatch!`.
pub enum Sessions {
    Memory(MemorySessions),
    Sqlite(SqliteSessions),
}

/// Run the same call on whichever backend `self` is.
macro_rules! dispatch {
    ($self:ident, $s:ident => $call:expr) => {
        match $self {
            Self::Memory($s) => $call.await,
            Self::Sqlite($s) => $call.await,
        }
    };
}

impl SessionStore for Sessions {
    async fn start(
        &self,
        task: TaskRef,
        source: SessionSource,
        owner: Owner,
        at: Time,
    ) -> Result<Session, SessionError> {
        dispatch!(self, s => s.start(task, source, owner, at))
    }

    async fn stop(&self, owner: Option<&Owner>, at: Time) -> Result<Option<Session>, SessionError> {
        dispatch!(self, s => s.stop(owner, at))
    }

    async fn running(&self) -> Result<Option<Session>, SessionError> {
        dispatch!(self, s => s.running())
    }

    async fn query(&self, q: &SessionQuery) -> Result<Vec<Session>, SessionError> {
        dispatch!(self, s => s.query(q))
    }

    async fn add(&self, task: TaskRef, start: Time, end: Time) -> Result<Session, SessionError> {
        dispatch!(self, s => s.add(task, start, end))
    }

    async fn split(
        &self,
        id: SessionId,
        at: Time,
    ) -> Result<Option<(Session, Session)>, SessionError> {
        dispatch!(self, s => s.split(id, at))
    }

    async fn cut(&self, id: SessionId, from: Time, to: Time) -> Result<Vec<Session>, SessionError> {
        dispatch!(self, s => s.cut(id, from, to))
    }

    async fn edit(&self, id: SessionId, patch: SessionPatch) -> Result<(), SessionError> {
        dispatch!(self, s => s.edit(id, patch))
    }

    async fn delete(&self, id: SessionId) -> Result<(), SessionError> {
        dispatch!(self, s => s.delete(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::time::Clock, storage::sqlite};

    /// Every method reaches the backend with its own arguments.
    mod memory {
        use super::*;

        contract::store_contract!(|clock: Clock| Sessions::Memory(MemorySessions::new(clock)));
    }

    mod sqlite_in_memory {
        use super::*;

        contract::store_contract!(|clock: Clock| {
            Sessions::Sqlite(SqliteSessions::new(
                sqlite::open_in_memory().unwrap(),
                clock,
            ))
        });
    }
}
