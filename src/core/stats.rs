//! `udo stats`' input: the sessions of the span in the scope, run through the
//! stats engine. Where `Core` picks the local zone for the engine; the engine
//! itself takes the zone as a parameter.

use chrono::Local;

use crate::{
    Res,
    core::Core,
    model::{
        sessions::{Session, SessionQuery, SessionStore, in_scope},
        time::Time,
    },
    stats::{
        self, Ctx, Query, Rows,
        sources::{SessionsSource, Source},
    },
};

impl Core {
    /// The stats of `query` for the node at `scope` (a path; empty: all).
    /// Running sessions count up to `now`. Callers pass `time::now()`.
    pub async fn stats(&self, query: &Query, scope: &[usize], now: Time) -> Res<Rows> {
        let found = self
            .sessions()
            .query(&SessionQuery {
                from: Some(query.from),
                to: Some(query.to),
                include_deleted: false,
                ..Default::default()
            })
            .await?;
        let keep = in_scope(self.tree(), scope, query.include_removed_tasks);
        let sessions: Vec<Session> = found.into_iter().filter(|s| keep(s)).collect();
        let intervals = SessionsSource(&sessions).intervals(query.from, query.to, now);
        let ctx = Ctx {
            tree: self.tree(),
            scope,
            tz: Local,
        };
        Ok(stats::run(query, &intervals, &ctx))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        stats::MeasureSpec,
        test_util::{core, local},
    };

    /// Minutes and sessions over the whole day of 2026-10-15, no dimensions.
    fn query(include_removed_tasks: bool) -> Query {
        Query {
            from: local(0, 0),
            to: local(23, 59),
            dims: vec![],
            measures: vec![MeasureSpec::Minutes, MeasureSpec::Sessions],
            include_removed_tasks,
        }
    }

    #[tokio::test]
    async fn core_stats_sums_sessions() {
        let (_tmp, core) = core().await;
        core.add_session(&[0], local(9, 0), local(10, 0), local(23, 0))
            .await
            .unwrap(); // a: 60
        core.add_session(&[1, 0], local(12, 0), local(13, 30), local(23, 0))
            .await
            .unwrap(); // ws/b: 90

        let rows = core.stats(&query(false), &[], local(23, 0)).await.unwrap();

        assert_eq!(rows.total.values["minutes"], json!(150));
        assert_eq!(rows.total.values["sessions"], json!(2));
    }

    #[tokio::test]
    async fn deleted_sessions_never_count() {
        let (_tmp, core) = core().await;
        core.add_session(&[0], local(9, 0), local(10, 0), local(23, 0))
            .await
            .unwrap();
        let deleted = core
            .add_session(&[1, 0], local(12, 0), local(13, 0), local(23, 0))
            .await
            .unwrap();
        core.delete_session(deleted.id).await.unwrap();

        let rows = core.stats(&query(true), &[], local(23, 0)).await.unwrap();

        assert_eq!(rows.total.values["minutes"], json!(60));
        assert_eq!(rows.total.values["sessions"], json!(1));
    }
}
