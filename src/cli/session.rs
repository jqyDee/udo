//! `udo session list / add / edit / split / cut / rm`: correct recorded
//! time. Sessions are named by the end of their ID (`short_id`).

use std::{collections::HashMap, fmt, path::Path};

use chrono::{Local, TimeDelta};
use serde::Serialize;

use super::{
    parse::{SessionTime, session_time},
    report::Report,
    resolve::{path_text, resolve},
};
use crate::{
    DATE_FMT, Res,
    core::Core,
    model::{
        id::NodeId,
        sessions::{Session, SessionId, SessionPatch, SessionQuery, SessionStore},
        time::{Minutes, Time},
        tree::Tree,
    },
};

/// `udo session …`. Times: YYYY-MM-DD HH:MM, now, -D (before now), +D
/// (after the first time of a pair); D like 1h30 or 45m.
#[derive(clap::Subcommand)]
pub enum SessionCommand {
    /// List sessions (default: the last 7 days) and their total
    List(ListArgs),
    /// Record a session by hand
    Add(AddArgs),
    /// Move a session's start or end
    Edit(EditArgs),
    /// Split a session in two
    Split(SplitArgs),
    /// Remove a part of a session (e.g. a break the timer ran through)
    Cut(CutArgs),
    /// Remove a session (asks first unless --yes)
    Rm(SessionRmArgs),
}

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

/// One session as the session commands show it.
#[derive(Serialize)]
pub struct SessionRow {
    /// The short form (`short_id`).
    pub id: String,
    pub full_id: String,
    pub task: String,
    /// The task's path; `None`: the task was removed.
    pub path: Option<String>,
    pub start: Time,
    /// `None`: running.
    pub end: Option<Time>,
    pub minutes: u32,
}

impl SessionRow {
    /// `session` timed up to `now`, its task looked up in `tree`.
    fn of(session: &Session, tree: &Tree, now: Time) -> Self {
        let path = tree
            .rows()
            .into_iter()
            .find(|r| r.node.id() == session.task.id)
            .map(|r| path_text(tree, &r.path));
        Self {
            id: short_id(session.id),
            full_id: session.id.to_string(),
            task: session.task.name.clone(),
            path,
            start: session.start,
            end: session.end,
            minutes: session.duration(now).get(),
        }
    }

    /// `4f9e2c  thu 2026-10-15  14:00-15:12  1h12  uni/cs/lab 3`
    fn line(&self) -> String {
        let start = self.start.with_timezone(&Local);
        let end = match self.end {
            Some(end) => end.with_timezone(&Local).format("%H:%M").to_string(),
            None => "now".into(),
        };
        let range = format!("{}-{end}", start.format("%H:%M"));
        let place = match &self.path {
            Some(path) => path.clone(),
            None => format!("{} (deleted)", self.task),
        };
        format!(
            "{}  {} {}  {range:<11}  {:<5} {place}",
            self.id,
            start.format("%a").to_string().to_lowercase(),
            start.format("%Y-%m-%d"),
            Minutes::new(self.minutes).to_string(),
        )
    }
}

/// The sessions of `args.node` in the range, timed up to `now`.
pub async fn list(core: &Core, cwd: &Path, now: Time, args: &ListArgs) -> Res<SessionList> {
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

#[derive(clap::Args)]
pub struct AddArgs {
    /// The task
    pub node: String,
    /// YYYY-MM-DD HH:MM, now or -D (before now)
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub start: SessionTime,
    /// Like START, or +D (after START)
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub end: SessionTime,
}

#[derive(clap::Args)]
pub struct EditArgs {
    /// The session: any unique end of its ID (lists show the last 6)
    pub id: String,
    /// New start
    #[arg(long, value_parser = session_time, allow_hyphen_values = true)]
    pub start: Option<SessionTime>,
    /// New end
    #[arg(long, value_parser = session_time, allow_hyphen_values = true)]
    pub end: Option<SessionTime>,
}

#[derive(clap::Args)]
pub struct SplitArgs {
    /// The session: any unique end of its ID
    pub id: String,
    /// Where to split it
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub at: SessionTime,
}

#[derive(clap::Args)]
pub struct CutArgs {
    /// The session: any unique end of its ID
    pub id: String,
    /// Start of the part to remove
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub from: SessionTime,
    /// Its end, or +D (after FROM)
    #[arg(value_parser = session_time, allow_hyphen_values = true)]
    pub to: SessionTime,
}

#[derive(clap::Args)]
pub struct SessionRmArgs {
    /// The session: any unique end of its ID
    pub id: String,
    /// Do not ask
    #[arg(long, short)]
    pub yes: bool,
}

/// What a correction left: the sessions it made or changed.
#[derive(Serialize)]
pub struct SessionDone {
    /// `added`, `edited`, `split`, `cut`, `removed`
    pub action: String,
    pub sessions: Vec<SessionRow>,
}

impl SessionDone {
    fn of(action: &str, sessions: &[Session], core: &Core, now: Time) -> Self {
        Self {
            action: action.into(),
            sessions: sessions
                .iter()
                .map(|s| SessionRow::of(s, core.tree(), now))
                .collect(),
        }
    }
}

/// Record a session on `args.node` by hand.
pub async fn add(core: &Core, cwd: &Path, now: Time, args: &AddArgs) -> Res<SessionDone> {
    let path = resolve(core.tree(), Some(&args.node), cwd)?;
    let start = args.start.resolve(now, None)?;
    let end = args.end.resolve(now, Some(start))?;
    let session = core.add_session(&path, start, end).await?;
    Ok(SessionDone::of("added", &[session], core, now))
}

/// Move a session's start and / or end.
pub async fn edit(core: &Core, now: Time, args: &EditArgs) -> Res<SessionDone> {
    if args.start.is_none() && args.end.is_none() {
        return Err("nothing to change: give --start and / or --end".into());
    }
    let session = find(core, &args.id).await?;
    let patch = SessionPatch {
        start: args.start.map(|t| t.resolve(now, None)).transpose()?,
        end: args.end.map(|t| t.resolve(now, None)).transpose()?,
    };
    core.edit_session(session.id, patch).await?;
    let edited = find(core, &session.id.to_string()).await?;
    Ok(SessionDone::of("edited", &[edited], core, now))
}

/// Split a session in two at `args.at`.
pub async fn split(core: &Core, now: Time, args: &SplitArgs) -> Res<SessionDone> {
    let session = find(core, &args.id).await?;
    let at = args.at.resolve(now, None)?;
    let (a, b) = core
        .split_session(session.id, at)
        .await?
        .ok_or("that is the session's start or end: nothing to split")?;
    Ok(SessionDone::of("split", &[a, b], core, now))
}

/// Remove `[from, to)` from a session.
pub async fn cut(core: &Core, now: Time, args: &CutArgs) -> Res<SessionDone> {
    let session = find(core, &args.id).await?;
    let from = args.from.resolve(now, None)?;
    let to = args.to.resolve(now, Some(from))?;
    let left = core.cut_session(session.id, from, to).await?;
    Ok(SessionDone::of("cut", &left, core, now))
}

/// Remove a session (soft: the data stays in `udo.db`). `confirm` gets the
/// question (the CLI: `rm::ask_on_terminal`).
pub async fn rm(
    core: &Core,
    now: Time,
    args: &SessionRmArgs,
    confirm: &mut dyn FnMut(&str) -> Res<bool>,
) -> Res<SessionDone> {
    let session = find(core, &args.id).await?;
    let done = SessionDone::of("removed", std::slice::from_ref(&session), core, now);
    let question = format!("remove session {}?", done.sessions[0].line());
    if !args.yes && !confirm(&question)? {
        return Err("nothing removed".into());
    }
    core.delete_session(session.id).await?;
    Ok(done)
}

impl fmt::Display for SessionDone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", self.action)?;
        for row in &self.sessions {
            write!(f, "\n{}", row.line())?;
        }
        Ok(())
    }
}

impl Report for SessionDone {}

/// Characters of an ID shown in lists (the random end: a UUID v7 starts
/// with its timestamp, so one day's sessions share their first ones).
const SHORT: usize = 6;

/// An ID without dashes, lowercase hex.
fn hex(id: SessionId) -> String {
    id.to_string().replace('-', "")
}

/// The last characters of `id`, as lists show it: `4f9e2c`.
pub fn short_id(id: SessionId) -> String {
    let hex = hex(id);
    hex[hex.len() - SHORT..].to_string()
}

/// The visible session whose ID ends in `ending` (any length, any case).
pub async fn find(core: &Core, ending: &str) -> Res<Session> {
    let sessions = core.sessions().query(&SessionQuery::default()).await?;
    find_in(sessions, ending)
}

/// `find` over `sessions`: exactly one must match.
fn find_in(sessions: Vec<Session>, ending: &str) -> Res<Session> {
    let ending = ending.trim().to_lowercase().replace('-', "");
    if ending.is_empty() {
        return Err("no session ID given".into());
    }
    let mut found: Vec<Session> = sessions
        .into_iter()
        .filter(|s| hex(s.id).ends_with(&ending))
        .collect();
    match found.len() {
        0 => Err(format!("no session ID ends in {ending:?}").into()),
        1 => Ok(found.remove(0)),
        _ => {
            // longer endings, so the next try can pick one
            let mut msg = format!("several sessions end in {ending:?}, type more of the ID:");
            for s in &found {
                let hex = hex(s.id);
                let start = s.start.with_timezone(&Local).format(DATE_FMT);
                msg.push_str(&format!("\n  {}  {start}  {}", &hex[hex.len() - 12..], s.task.name));
            }
            Err(msg.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use clap::Parser;

    use super::*;
    use crate::{
        cli::{Cli, report::render},
        model::time::{Minutes, local_to_fixed},
        test_util::{at, core, dt},
    };

    /// `session` with its id replaced by `id` (a UUID text).
    fn with_id(session: &Session, id: &str) -> Session {
        Session {
            id: id.parse().unwrap(),
            ..session.clone()
        }
    }

    #[tokio::test]
    async fn short_id_is_the_last_six_hex_characters() {
        let (_tmp, core) = core().await;
        let s = core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();
        let s = with_id(&s, "01890000-0000-7000-8000-0000004f9e2c");

        assert_eq!(short_id(s.id), "4f9e2c");
    }

    #[tokio::test]
    async fn find_takes_any_unique_ending() {
        let (_tmp, core) = core().await;
        let a = core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();
        let sessions = vec![
            with_id(&a, "01890000-0000-7000-8000-0000004f9e2c"),
            with_id(&a, "01890000-0000-7000-8000-000000123456"),
        ];

        let found = find_in(sessions, "E2C").unwrap(); // case does not matter

        assert_eq!(short_id(found.id), "4f9e2c");
    }

    #[tokio::test]
    async fn find_without_a_match_is_an_error() {
        let (_tmp, core) = core().await;
        core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();

        let err = find(&core, "zzz").await.unwrap_err().to_string();

        assert!(err.contains("no session") && err.contains("zzz"), "{err}");
    }

    #[tokio::test]
    async fn find_with_several_matches_lists_them() {
        let (_tmp, core) = core().await;
        let a = core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();
        let sessions = vec![
            with_id(&a, "01890000-0000-7000-8000-0000004f9e2c"),
            with_id(&a, "01890000-0000-7000-8000-0000014f9e2c"),
        ];

        let err = find_in(sessions, "4f9e2c").unwrap_err().to_string();

        assert!(err.contains("00004f9e2c") && err.contains("00014f9e2c"), "{err}");
        // each line names the task, to tell them apart
        assert!(err.lines().skip(1).all(|l| l.ends_with("  a")), "{err}");
    }

    #[tokio::test]
    async fn find_skips_removed_sessions() {
        let (_tmp, core) = core().await;
        let s = core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();
        core.delete_session(s.id).await.unwrap();

        assert!(find(&core, &short_id(s.id)).await.is_err());
    }

    // ---------- list (core: root: [a, ws: [b]]) ----------

    /// 2026-10-15 at `h:m` local time, so the text is the same in any zone.
    fn local(h: u32, m: u32) -> Time {
        local_to_fixed(dt(2026, 10, 15, h, m)).unwrap()
    }

    fn now() -> Time {
        local(20, 0)
    }

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

        let list = list(&core, tmp.path(), now(), &list_args(None))
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
        let list = list(&core, tmp.path(), now(), &args).await.unwrap();

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
        let list = list(&core, tmp.path(), now(), &args).await.unwrap();

        assert_eq!(tasks(&list), vec!["b"]);
    }

    #[tokio::test]
    async fn list_to_plus_d_without_from_is_refused() {
        let (tmp, core) = core().await;

        let args = ListArgs {
            to: Some(SessionTime::After(Minutes::new(60))),
            ..list_args(None)
        };
        let Err(err) = list(&core, tmp.path(), now(), &args).await else {
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

        let list = list(&core, tmp.path(), now(), &list_args(Some("ws")))
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

        let hidden = list(&core, tmp.path(), now(), &list_args(None))
            .await
            .unwrap();
        let args = ListArgs {
            deleted: true,
            ..list_args(None)
        };
        let shown = list(&core, tmp.path(), now(), &args).await.unwrap();

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
        let shown = list(&core, tmp.path(), now(), &args).await.unwrap();

        assert_eq!(tasks(&shown), vec!["b"]);
    }

    #[tokio::test]
    async fn a_running_session_counts_up_to_now() {
        let (tmp, mut core) = core().await;
        core.start(&[0], local(19, 0)).await.unwrap();

        let list = list(&core, tmp.path(), now(), &list_args(None))
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

        let list = list(&core, tmp.path(), now(), &list_args(None))
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

        let list = list(&core, tmp.path(), now(), &list_args(None))
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

        let list = list(&core, tmp.path(), now(), &list_args(None))
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

    // ---------- add / edit / split / cut / rm ----------

    /// `SessionTime` for 2026-10-15 `h:m` local.
    fn t(h: u32, m: u32) -> SessionTime {
        SessionTime::At(dt(2026, 10, 15, h, m))
    }

    fn add_args(node: &str, start: SessionTime, end: SessionTime) -> AddArgs {
        AddArgs {
            node: node.into(),
            start,
            end,
        }
    }

    /// (start, end) of each resulting session.
    fn spans(done: &SessionDone) -> Vec<(Time, Option<Time>)> {
        done.sessions.iter().map(|r| (r.start, r.end)).collect()
    }

    /// A 09:00-12:00 session on task "a"; its short id.
    async fn nine_to_twelve(core: &Core) -> String {
        short_id(
            core.add_session(&[0], local(9, 0), local(12, 0))
                .await
                .unwrap()
                .id,
        )
    }

    #[tokio::test]
    async fn add_records_a_session_and_shows_it() {
        let (tmp, core) = core().await;

        let done = add(&core, tmp.path(), now(), &add_args("ws/b", t(14, 0), t(15, 12)))
            .await
            .unwrap();

        assert_eq!(spans(&done), vec![(local(14, 0), Some(local(15, 12)))]);
        let id = &done.sessions[0].id;
        assert_eq!(
            done.to_string(),
            format!("added:\n{id}  thu 2026-10-15  14:00-15:12  1h12  ws/b")
        );
    }

    #[tokio::test]
    async fn add_end_plus_d_counts_from_the_start() {
        let (tmp, core) = core().await;
        let end = SessionTime::After(Minutes::new(90));

        let done = add(&core, tmp.path(), now(), &add_args("a", t(14, 0), end))
            .await
            .unwrap();

        assert_eq!(spans(&done), vec![(local(14, 0), Some(local(15, 30)))]);
    }

    #[tokio::test]
    async fn add_start_plus_d_is_refused() {
        let (tmp, core) = core().await;
        let start = SessionTime::After(Minutes::new(90));

        let result = add(&core, tmp.path(), now(), &add_args("a", start, t(15, 0))).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_passes_the_store_refusal_through() {
        let (tmp, core) = core().await;
        nine_to_twelve(&core).await;

        let Err(err) = add(&core, tmp.path(), now(), &add_args("a", t(11, 0), t(13, 0))).await
        else {
            panic!("an overlapping session was added");
        };

        assert_eq!(err.to_string(), "overlaps another session");
    }

    #[tokio::test]
    async fn edit_moves_start_and_end() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let args = EditArgs {
            id,
            start: Some(t(8, 30)),
            end: Some(SessionTime::Ago(Minutes::new(9 * 60))), // 11:00
        };
        let done = edit(&core, now(), &args).await.unwrap();

        assert_eq!(spans(&done), vec![(local(8, 30), Some(local(11, 0)))]);
        assert!(done.to_string().starts_with("edited:\n"));
    }

    #[tokio::test]
    async fn edit_without_a_change_is_refused() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let args = EditArgs {
            id,
            start: None,
            end: None,
        };
        assert!(edit(&core, now(), &args).await.is_err());
    }

    #[tokio::test]
    async fn split_shows_both_pieces() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let done = split(&core, now(), &SplitArgs { id, at: t(10, 0) })
            .await
            .unwrap();

        assert_eq!(
            spans(&done),
            vec![
                (local(9, 0), Some(local(10, 0))),
                (local(10, 0), Some(local(12, 0)))
            ]
        );
    }

    #[tokio::test]
    async fn split_outside_the_session_is_refused() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let Err(err) = split(&core, now(), &SplitArgs { id, at: t(13, 0) }).await else {
            panic!("split outside the session");
        };

        assert_eq!(err.to_string(), "time outside of session time");
    }

    #[tokio::test]
    async fn split_on_the_start_is_refused() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        assert!(
            split(&core, now(), &SplitArgs { id, at: t(9, 0) })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cut_leaves_the_pieces_around_it() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let args = CutArgs {
            id,
            from: t(10, 0),
            to: SessionTime::After(Minutes::new(45)),
        };
        let done = cut(&core, now(), &args).await.unwrap();

        assert_eq!(
            spans(&done),
            vec![
                (local(9, 0), Some(local(10, 0))),
                (local(10, 45), Some(local(12, 0)))
            ]
        );
    }

    #[tokio::test]
    async fn cut_on_a_running_session_keeps_it_running() {
        let (_tmp, mut core) = core().await;
        let running = core.start(&[0], local(19, 0)).await.unwrap();

        let args = CutArgs {
            id: short_id(running.id),
            from: t(19, 15),
            to: t(19, 30),
        };
        let done = cut(&core, now(), &args).await.unwrap();

        assert_eq!(done.sessions.last().unwrap().end, None);
        assert!(core.sessions().running().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn rm_asks_then_removes() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;
        let mut asked = vec![];
        let mut yes = |q: &str| -> Res<bool> {
            asked.push(q.to_string());
            Ok(true)
        };

        let args = SessionRmArgs {
            id: id.clone(),
            yes: false,
        };
        let done = rm(&core, now(), &args, &mut yes).await.unwrap();

        assert_eq!(asked.len(), 1);
        assert!(asked[0].contains(&id), "{}", asked[0]);
        assert!(done.to_string().starts_with("removed:\n"));
        assert!(find(&core, &id).await.is_err()); // gone from every list
    }

    #[tokio::test]
    async fn rm_answered_no_keeps_the_session() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;
        let mut no = |_: &str| -> Res<bool> { Ok(false) };

        let args = SessionRmArgs {
            id: id.clone(),
            yes: false,
        };
        let Err(err) = rm(&core, now(), &args, &mut no).await else {
            panic!("removed without a yes");
        };

        assert_eq!(err.to_string(), "nothing removed");
        assert!(find(&core, &id).await.is_ok());
    }

    #[tokio::test]
    async fn rm_yes_does_not_ask() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;
        let mut never = |_: &str| -> Res<bool> { panic!("asked despite --yes") };

        let args = SessionRmArgs { id, yes: true };
        assert!(rm(&core, now(), &args, &mut never).await.is_ok());
    }

    #[tokio::test]
    async fn done_json_has_the_action_and_full_rows() {
        let (_tmp, core) = core().await;
        let id = nine_to_twelve(&core).await;

        let done = split(&core, now(), &SplitArgs { id, at: t(10, 0) })
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_str(&render(&done, true).unwrap()).unwrap();

        assert_eq!(json["action"], "split");
        assert_eq!(json["sessions"].as_array().unwrap().len(), 2);
        assert_eq!(json["sessions"][0]["full_id"].as_str().unwrap().len(), 36);
    }

    // ---------- parsing ----------

    fn parses(args: &[&str]) -> bool {
        let mut argv = vec!["udo", "session"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).is_ok()
    }

    #[test]
    fn session_commands_parse() {
        assert!(parses(&["list"]));
        assert!(parses(&["list", "uni", "--from", "-2h", "--to", "+1h", "--deleted"]));
        assert!(parses(&["add", "lab 3", "2026-10-15 14:00", "+1h30"]));
        assert!(parses(&["edit", "4f9e2c", "--start", "-45m"]));
        assert!(parses(&["split", "4f9e2c", "2026-10-15 12:00"]));
        assert!(parses(&["cut", "4f9e2c", "2026-10-15 12:00", "+45m"]));
        assert!(parses(&["rm", "4f9e2c", "--yes"]));
        // -D as a value, not a flag
        assert!(parses(&["add", "lab 3", "-1h", "now"]));
        assert!(parses(&["split", "4f9e2c", "-30m"]));
        assert!(parses(&["cut", "4f9e2c", "-1h", "-45m"]));
        assert!(parses(&["edit", "4f9e2c", "--end", "-5m"]));
    }

    #[test]
    fn missing_or_wrong_arguments_are_refused() {
        assert!(!parses(&["add", "lab 3", "2026-10-15 14:00"])); // no END
        assert!(!parses(&["add", "lab 3", "14:00", "15:00"])); // no date
        assert!(!parses(&["list", "--all", "--from", "-1h"]));
        assert!(!parses(&["cut", "4f9e2c", "now"]));
        assert!(!parses(&["rm"]));
    }

    #[tokio::test]
    async fn an_empty_ending_is_refused() {
        let (_tmp, core) = core().await;
        core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();

        assert!(find(&core, "").await.is_err());
    }
}
