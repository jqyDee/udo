//! `udo session list [NODE] [--from T] [--to T] [--all] [--deleted]`.

use std::{collections::HashMap, fmt, path::Path};

use chrono::TimeDelta;
use serde::Serialize;

use super::row::SessionRow;
use crate::{
    Res,
    cli::{
        parse::{SessionTime, session_time},
        report::Report,
        resolve::resolve,
    },
    core::Core,
    model::{
        id::NodeId,
        sessions::{SessionQuery, SessionStore},
        time::{Minutes, Time},
    },
};

#[derive(clap::Args)]
pub struct ListArgs {
    /// A task, or a container for every task below it; default: the node
    /// of the current folder, else everything
    pub node: Option<String>,
    /// Start of the range (default: 7 days ago)
    #[arg(long, value_parser = session_time, allow_hyphen_values = true)]
    pub from: Option<SessionTime>,
    /// End of the range (default: now); +D counts from --from
    #[arg(long, value_parser = session_time, allow_hyphen_values = true)]
    pub to: Option<SessionTime>,
    /// No range: every session
    #[arg(long, conflicts_with_all = ["from", "to"])]
    pub all: bool,
    /// Also sessions of removed tasks
    #[arg(long)]
    pub deleted: bool,
}

/// `list`'s result: sessions sorted by start, and their total.
#[derive(Serialize)]
pub struct SessionList {
    pub rows: Vec<SessionRow>,
    /// Whole minutes, a running session up to now.
    pub total_minutes: u32,
}

/// The sessions of `args.node` in the range, timed up to `now`.
pub async fn run(core: &Core, cwd: &Path, now: Time, args: &ListArgs) -> Res<SessionList> {
    let tree = core.tree();
    let start = match &args.node {
        Some(node) => resolve(tree, Some(node), cwd)?,
        None => resolve(tree, None, cwd).unwrap_or_default(), // outside udo: everything
    };

    let (from, to) = if args.all {
        (None, None)
    } else {
        // `+D` in --to counts from an explicit --from only
        let from = args.from.map(|t| t.resolve(now, None)).transpose()?;
        let to = args.to.map(|t| t.resolve(now, from)).transpose()?;
        (Some(from.unwrap_or(now - TimeDelta::days(7))), to)
    };
    let query = SessionQuery {
        from,
        to,
        ..Default::default()
    };
    let sessions = core.sessions().query(&query).await?;

    // live tasks at / below the node, and its containers (for removed tasks)
    let mut live: HashMap<NodeId, bool> = HashMap::new(); // task id -> below the node
    let mut containers: Vec<NodeId> = vec![];
    for r in tree.rows() {
        let below = r.path.starts_with(&start);
        if r.node.as_task().is_some() {
            live.insert(r.node.id(), below);
        } else if below {
            containers.push(r.node.id());
        }
    }

    let rows: Vec<SessionRow> = sessions
        .iter()
        .filter(|s| match live.get(&s.task.id) {
            Some(below) => *below,
            // removed task: below the node if its container is; from the
            // root, all of them (their container may be gone too)
            None => args.deleted && (start.is_empty() || containers.contains(&s.task.container_id)),
        })
        .map(|s| SessionRow::of(s, tree, now))
        .collect();
    let total_minutes = rows.iter().map(|r| r.minutes).sum();
    Ok(SessionList {
        rows,
        total_minutes,
    })
}

impl fmt::Display for SessionList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.rows.is_empty() {
            return write!(f, "no sessions");
        }
        for row in &self.rows {
            writeln!(f, "{}", row.line())?;
        }
        let n = self.rows.len();
        let sessions = if n == 1 { "session" } else { "sessions" };
        write!(f, "total: {} in {n} {sessions}", Minutes::new(self.total_minutes))
    }
}

impl Report for SessionList {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::{commands::session::id::short_id, commands::session::testing::now, report::render},
        test_util::{core, dt, local},
    };

    fn list_args(node: Option<&str>) -> ListArgs {
        ListArgs {
            node: node.map(Into::into),
            from: None,
            to: None,
            all: false,
            deleted: false,
        }
    }

    fn tasks(list: &SessionList) -> Vec<&str> {
        list.rows.iter().map(|r| r.task.as_str()).collect()
    }

    #[tokio::test]
    async fn list_defaults_to_the_last_seven_days() {
        let (tmp, core) = core().await;
        let week = TimeDelta::days(7);
        core.add_session(&[0], local(9, 0) - week - week, local(10, 0) - week - week)
            .await
            .unwrap(); // two weeks ago: out
        core.add_session(&[0], local(19, 0) - week, local(21, 0) - week)
            .await
            .unwrap(); // over the edge: in
        core.add_session(&[1, 0], local(9, 0), local(10, 0))
            .await
            .unwrap();

        let list = run(&core, tmp.path(), now(), &list_args(None))
            .await
            .unwrap();

        assert_eq!(tasks(&list), vec!["a", "b"]);
        assert_eq!(list.total_minutes, 120 + 60);
    }

    #[tokio::test]
    async fn list_all_drops_the_range() {
        let (tmp, core) = core().await;
        let month = TimeDelta::days(30);
        core.add_session(&[0], local(9, 0) - month, local(10, 0) - month)
            .await
            .unwrap();

        let args = ListArgs {
            all: true,
            ..list_args(None)
        };
        let list = run(&core, tmp.path(), now(), &args).await.unwrap();

        assert_eq!(tasks(&list), vec!["a"]);
    }

    #[tokio::test]
    async fn list_from_and_to_narrow_the_range() {
        let (tmp, core) = core().await;
        core.add_session(&[0], local(9, 0), local(10, 0))
            .await
            .unwrap();
        core.add_session(&[1, 0], local(12, 0), local(13, 0))
            .await
            .unwrap();

        let args = ListArgs {
            from: Some(SessionTime::At(dt(2026, 10, 15, 11, 0))),
            to: Some(SessionTime::After(Minutes::new(90))), // 12:30
            ..list_args(None)
        };
        let list = run(&core, tmp.path(), now(), &args).await.unwrap();

        assert_eq!(tasks(&list), vec!["b"]);
    }

    #[tokio::test]
    async fn list_to_plus_d_without_from_is_refused() {
        let (tmp, core) = core().await;

        let args = ListArgs {
            to: Some(SessionTime::After(Minutes::new(60))),
            ..list_args(None)
        };
        let Err(err) = run(&core, tmp.path(), now(), &args).await else {
            panic!("--to +1h without --from was accepted");
        };

        assert!(err.to_string().contains("second time"), "{err}");
    }

    #[tokio::test]
    async fn list_of_a_container_has_the_tasks_below_it() {
        let (tmp, core) = core().await;
        core.add_session(&[0], local(9, 0), local(10, 0))
            .await
            .unwrap();
        core.add_session(&[1, 0], local(12, 0), local(13, 0))
            .await
            .unwrap();

        let list = run(&core, tmp.path(), now(), &list_args(Some("ws")))
            .await
            .unwrap();

        assert_eq!(tasks(&list), vec!["b"]);
        assert_eq!(list.rows[0].path.as_deref(), Some("ws/b"));
    }

    #[tokio::test]
    async fn sessions_of_a_deleted_task_only_show_with_deleted() {
        let (tmp, mut core) = core().await;
        core.add_session(&[0], local(9, 0), local(10, 0))
            .await
            .unwrap();
        core.delete(&[0], now()).await.unwrap(); // task "a" is gone

        let hidden = run(&core, tmp.path(), now(), &list_args(None))
            .await
            .unwrap();
        let args = ListArgs {
            deleted: true,
            ..list_args(None)
        };
        let shown = run(&core, tmp.path(), now(), &args).await.unwrap();

        assert!(hidden.rows.is_empty());
        assert_eq!(tasks(&shown), vec!["a"]);
        assert_eq!(shown.rows[0].path, None);
        assert!(shown.to_string().contains("a (deleted)"), "{shown}");
    }

    #[tokio::test]
    async fn deleted_shows_tasks_whose_container_is_gone_too() {
        let (tmp, mut core) = core().await;
        core.add_session(&[1, 0], local(9, 0), local(10, 0))
            .await
            .unwrap();
        core.delete(&[1], now()).await.unwrap(); // ws and its task "b"

        let args = ListArgs {
            deleted: true,
            ..list_args(None)
        };
        let shown = run(&core, tmp.path(), now(), &args).await.unwrap();

        assert_eq!(tasks(&shown), vec!["b"]);
    }

    #[tokio::test]
    async fn a_running_session_counts_up_to_now() {
        let (tmp, mut core) = core().await;
        core.start(&[0], local(19, 0)).await.unwrap();

        let list = run(&core, tmp.path(), now(), &list_args(None))
            .await
            .unwrap();

        assert_eq!(list.rows[0].end, None);
        assert_eq!(list.total_minutes, 60);
        assert!(list.to_string().contains("19:00-now"), "{list}");
    }

    #[tokio::test]
    async fn list_text_is_one_line_per_session_and_a_total() {
        let (tmp, core) = core().await;
        let a = core
            .add_session(&[0], local(9, 0), local(10, 12))
            .await
            .unwrap();
        let b = core
            .add_session(&[1, 0], local(14, 0), local(14, 45))
            .await
            .unwrap();

        let list = run(&core, tmp.path(), now(), &list_args(None))
            .await
            .unwrap();

        let expected = format!(
            "{}  thu 2026-10-15  09:00-10:12  1h12  a\n\
             {}  thu 2026-10-15  14:00-14:45  45m   ws/b\n\
             total: 1h57 in 2 sessions",
            short_id(a.id),
            short_id(b.id)
        );
        assert_eq!(list.to_string(), expected);
    }

    #[tokio::test]
    async fn an_empty_list_says_so() {
        let (tmp, core) = core().await;

        let list = run(&core, tmp.path(), now(), &list_args(None))
            .await
            .unwrap();

        assert_eq!(list.to_string(), "no sessions");
    }

    #[tokio::test]
    async fn list_json_has_full_ids_and_rfc3339_times() {
        let (tmp, core) = core().await;
        let a = core
            .add_session(&[0], local(9, 0), local(10, 0))
            .await
            .unwrap();

        let list = run(&core, tmp.path(), now(), &list_args(None))
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&render(&list, true).unwrap()).unwrap();

        let row = &json["rows"][0];
        assert_eq!(row["full_id"], a.id.to_string());
        assert_eq!(row["id"], short_id(a.id));
        let start: Time = row["start"].as_str().unwrap().parse().unwrap();
        assert_eq!(start, local(9, 0));
        assert_eq!(json["total_minutes"], 60);
    }
}
