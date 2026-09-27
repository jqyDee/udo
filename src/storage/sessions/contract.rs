use chrono::{FixedOffset, TimeZone};

use crate::model::{
    id::NodeId,
    sessions::{
        Session, SessionError, SessionId, SessionPatch, SessionQuery, SessionSource, SessionStore,
        TaskRef,
    },
    time::Time,
};

/// 2026-10-15 at `h:m`, offset +02:00. Fixed, never `time::now()`: results
/// must not depend on when the test runs.
fn at(h: u32, m: u32) -> Time {
    FixedOffset::east_opt(2 * 3600)
        .unwrap()
        .with_ymd_and_hms(2026, 10, 15, h, m, 0)
        .unwrap()
}

/// A task with a fresh id, in `uni/cs`.
fn task(name: &str) -> TaskRef {
    TaskRef {
        id: NodeId::new(),
        name: name.into(),
        description: String::new(),
        container_path: "uni/cs".into(),
    }
}

/// The rules every SessionStore must keep, one `#[tokio::test]` per check,
/// so a failing check does not hide the others. `$make` gives a fresh,
/// empty store. Use inside a backend's test module:
///
/// ```ignore
/// super::contract::store_contract!(MemorySessions::default);
/// ```
macro_rules! store_contract {
    ($make:expr) => {
        $crate::storage::sessions::contract::store_contract!(@each $make;
            check_start_then_running,
            check_start_stops_previous,
            check_start_same_task_is_noop,
            check_stop_without_running_is_none,
            check_stop_before_start_errors_and_soft_deletes,
            check_add_is_not_edited,
            check_add_refuses_end_before_start,
            check_add_refuses_overlap,
            check_touching_sessions_do_not_overlap,
            check_edit_marks_edited,
            check_edit_refuses_end_before_start,
            check_edit_refuses_overlap,
            check_edit_end_of_running_errors,
            check_split_inside,
            check_split_outside_errors,
            check_split_on_edge_is_noop,
            check_split_running_errors,
            check_cut_inside,
            check_cut_over_an_edge_trims,
            check_cut_refusals,
            check_cut_running_keeps_running,
            check_delete_hides,
            check_delete_running_stops,
            check_unknown_id_is_not_found,
            check_query_by_task,
            check_query_range_intersects,
            check_query_include_deleted,
        );
    };
    (@each $make:expr; $($check:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $check() {
                $crate::storage::sessions::contract::$check(($make)()).await;
            }
        )*
    };
}
pub(crate) use store_contract;

// --------------- start / stop ---------------

/// `start` gives a running session; `running` returns it.
pub(super) async fn check_start_then_running(s: impl SessionStore) {
    let t = task("lab 3");
    let started = s
        .start(t.clone(), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();

    assert_eq!(started.task, t);
    assert_eq!(started.start, at(14, 0));
    assert_eq!(started.end, None);
    assert!(!started.edited);
    assert_eq!(s.running().await.unwrap(), Some(started));
}

/// Starting another task stops the running session at `at` (one timer).
pub(super) async fn check_start_stops_previous(s: impl SessionStore) {
    let first = s
        .start(task("lab 3"), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();
    let second = s
        .start(task("reading"), SessionSource::Manual, at(15, 0))
        .await
        .unwrap();

    assert_eq!(s.running().await.unwrap(), Some(second));
    let stopped = s.query(&SessionQuery::default()).await.unwrap();
    let first = stopped.iter().find(|x| x.id == first.id).unwrap();
    assert_eq!(first.end, Some(at(15, 0)));
}

/// Starting the task that is already running changes nothing.
pub(super) async fn check_start_same_task_is_noop(s: impl SessionStore) {
    let t = task("lab 3");
    let first = s
        .start(t.clone(), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();
    let again = s.start(t, SessionSource::Manual, at(15, 0)).await.unwrap();

    assert_eq!(again, first);
    assert_eq!(s.query(&SessionQuery::default()).await.unwrap().len(), 1);
}

/// `stop` without a running session is `Ok(None)`.
pub(super) async fn check_stop_without_running_is_none(s: impl SessionStore) {
    assert_eq!(s.stop(at(14, 0)).await, Ok(None));
}

/// `stop` before the start: `EndBeforeStart`, the session is soft-deleted.
pub(super) async fn check_stop_before_start_errors_and_soft_deletes(s: impl SessionStore) {
    let started = s
        .start(task("lab 3"), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();

    assert_eq!(s.stop(at(13, 0)).await, Err(SessionError::EndBeforeStart));
    assert_eq!(s.running().await.unwrap(), None);
    assert!(s.query(&SessionQuery::default()).await.unwrap().is_empty());

    let all = SessionQuery {
        include_deleted: true,
        ..Default::default()
    };
    let kept = s.query(&all).await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, started.id);
    assert!(kept[0].deleted_at.is_some());
}

// --------------- add ---------------

/// A manual session has source `manual` and is not edited.
pub(super) async fn check_add_is_not_edited(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(10, 30)).await.unwrap();

    assert_eq!(added.source, SessionSource::Manual);
    assert!(!added.edited);
    assert_eq!(added.end, Some(at(10, 30)));
    assert_eq!(s.running().await.unwrap(), None); // finished, not running
    assert_eq!(
        s.query(&SessionQuery::default()).await.unwrap(),
        vec![added]
    );
}

/// `add` with end before (or on) start: `EndBeforeStart`, nothing stored.
pub(super) async fn check_add_refuses_end_before_start(s: impl SessionStore) {
    let t = task("lab 3");
    assert_eq!(
        s.add(t.clone(), at(10, 0), at(9, 0)).await,
        Err(SessionError::EndBeforeStart)
    );
    assert_eq!(
        s.add(t, at(10, 0), at(10, 0)).await,
        Err(SessionError::EndBeforeStart)
    );
    assert!(s.query(&SessionQuery::default()).await.unwrap().is_empty());
}

/// `add` over an existing session: `Overlap`. A running one counts as open-ended.
pub(super) async fn check_add_refuses_overlap(s: impl SessionStore) {
    s.add(task("lab 3"), at(9, 0), at(11, 0)).await.unwrap();

    assert_eq!(
        s.add(task("a"), at(10, 0), at(12, 0)).await,
        Err(SessionError::Overlap)
    ); // tail
    assert_eq!(
        s.add(task("b"), at(8, 0), at(12, 0)).await,
        Err(SessionError::Overlap)
    ); // around
    assert_eq!(
        s.add(task("c"), at(9, 30), at(10, 0)).await,
        Err(SessionError::Overlap)
    ); // inside

    s.start(task("d"), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();
    assert_eq!(
        s.add(task("e"), at(15, 0), at(16, 0)).await,
        Err(SessionError::Overlap)
    ); // after a running start
}

/// Half-open intervals: 14:00–15:00 and 15:00–16:00 both fit.
pub(super) async fn check_touching_sessions_do_not_overlap(s: impl SessionStore) {
    s.add(task("a"), at(14, 0), at(15, 0)).await.unwrap();
    s.add(task("b"), at(15, 0), at(16, 0)).await.unwrap();
    s.add(task("c"), at(13, 0), at(14, 0)).await.unwrap();
    assert_eq!(s.query(&SessionQuery::default()).await.unwrap().len(), 3);
}

// --------------- edit ---------------

/// All sessions (not deleted), sorted by start.
async fn visible(s: &impl SessionStore) -> Vec<Session> {
    s.query(&SessionQuery::default()).await.unwrap()
}

/// All sessions, deleted ones too, sorted by start.
async fn everything(s: &impl SessionStore) -> Vec<Session> {
    let all = SessionQuery {
        include_deleted: true,
        ..Default::default()
    };
    s.query(&all).await.unwrap()
}

fn patch(start: Option<Time>, end: Option<Time>) -> SessionPatch {
    SessionPatch { start, end }
}

/// After `edit` the session is edited and has the new times.
pub(super) async fn check_edit_marks_edited(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(11, 0)).await.unwrap();

    s.edit(added.id, patch(None, Some(at(10, 0))))
        .await
        .unwrap();

    let edited = &visible(&s).await[0];
    assert!(edited.edited);
    assert_eq!(edited.start, at(9, 0)); // untouched
    assert_eq!(edited.end, Some(at(10, 0)));
}

/// `edit` to an end not after the start: `EndBeforeStart`, nothing changed.
pub(super) async fn check_edit_refuses_end_before_start(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(11, 0)).await.unwrap();

    let err = Err(SessionError::EndBeforeStart);
    assert_eq!(s.edit(added.id, patch(None, Some(at(8, 0)))).await, err);
    assert_eq!(s.edit(added.id, patch(Some(at(11, 0)), None)).await, err);
    assert_eq!(visible(&s).await, vec![added]);
}

/// `edit` into another session: `Overlap`, nothing changed. A session never
/// collides with itself.
pub(super) async fn check_edit_refuses_overlap(s: impl SessionStore) {
    let a = s.add(task("a"), at(9, 0), at(10, 0)).await.unwrap();
    let b = s.add(task("b"), at(11, 0), at(12, 0)).await.unwrap();

    let err = Err(SessionError::Overlap);
    assert_eq!(s.edit(a.id, patch(None, Some(at(11, 30)))).await, err);
    assert_eq!(visible(&s).await, vec![a.clone(), b]);

    s.edit(a.id, patch(Some(at(9, 30)), None)).await.unwrap(); // inside its own range
    assert_eq!(visible(&s).await[0].start, at(9, 30));
}

/// Setting the end of a running session: `Running` (use `stop`). Moving its
/// start is fine.
pub(super) async fn check_edit_end_of_running_errors(s: impl SessionStore) {
    let running = s
        .start(task("lab 3"), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();

    let end = patch(None, Some(at(15, 0)));
    assert_eq!(s.edit(running.id, end).await, Err(SessionError::Running));

    s.edit(running.id, patch(Some(at(13, 30)), None))
        .await
        .unwrap();
    let now = s.running().await.unwrap().unwrap();
    assert_eq!(now.start, at(13, 30));
    assert_eq!(now.end, None);
}

// --------------- split ---------------

/// `at` strictly inside: two sessions meeting at `at`, both edited.
pub(super) async fn check_split_inside(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(11, 0)).await.unwrap();

    let (first, second) = s.split(added.id, at(10, 0)).await.unwrap().unwrap();

    assert_eq!(first.id, added.id); // the original keeps its id
    assert_ne!(second.id, added.id);
    assert_eq!((first.start, first.end), (at(9, 0), Some(at(10, 0))));
    assert_eq!((second.start, second.end), (at(10, 0), Some(at(11, 0))));
    assert_eq!(second.task, added.task);
    assert_eq!(second.source, added.source);
    assert!(first.edited && second.edited);
    assert_eq!(visible(&s).await, vec![first, second]);
}

/// `at` before the start or after the end: `OutsideSession`.
pub(super) async fn check_split_outside_errors(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(11, 0)).await.unwrap();

    let err = Err(SessionError::OutsideSession);
    assert_eq!(s.split(added.id, at(8, 0)).await, err);
    assert_eq!(s.split(added.id, at(12, 0)).await, err);
    assert_eq!(visible(&s).await, vec![added]);
}

/// `at` exactly on start or end: `Ok(None)`, nothing changed.
pub(super) async fn check_split_on_edge_is_noop(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(11, 0)).await.unwrap();

    assert_eq!(s.split(added.id, at(9, 0)).await, Ok(None));
    assert_eq!(s.split(added.id, at(11, 0)).await, Ok(None));
    assert_eq!(visible(&s).await, vec![added]); // not even marked edited
}

/// Splitting a running session: `Running`.
pub(super) async fn check_split_running_errors(s: impl SessionStore) {
    let running = s
        .start(task("lab 3"), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();

    assert_eq!(
        s.split(running.id, at(15, 0)).await,
        Err(SessionError::Running)
    );
    assert_eq!(s.running().await.unwrap(), Some(running));
}

// --------------- cut ---------------

/// Cut inside: two pieces around the gap; the first keeps the id.
pub(super) async fn check_cut_inside(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(17, 0)).await.unwrap();

    let left = s.cut(added.id, at(12, 0), at(13, 0)).await.unwrap();

    assert_eq!(left.len(), 2);
    let (first, second) = (&left[0], &left[1]);
    assert_eq!(first.id, added.id);
    assert_ne!(second.id, added.id);
    assert_eq!((first.start, first.end), (at(9, 0), Some(at(12, 0))));
    assert_eq!((second.start, second.end), (at(13, 0), Some(at(17, 0))));
    assert_eq!(second.task, added.task);
    assert!(first.edited && second.edited);
    assert_eq!(visible(&s).await, left);
}

/// Cut over the start or the end: one trimmed piece, same id. Clamped.
pub(super) async fn check_cut_over_an_edge_trims(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(17, 0)).await.unwrap();

    let left = s.cut(added.id, at(8, 0), at(10, 0)).await.unwrap(); // over the start
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].id, added.id);
    assert_eq!((left[0].start, left[0].end), (at(10, 0), Some(at(17, 0))));
    assert!(left[0].edited);

    let left = s.cut(added.id, at(16, 0), at(18, 0)).await.unwrap(); // over the end
    assert_eq!((left[0].start, left[0].end), (at(10, 0), Some(at(16, 0))));
    assert_eq!(visible(&s).await, left);
}

/// No overlap with the session: `OutsideSession`; all of it: `WholeSession`;
/// `to` not after `from`: `EndBeforeStart`. Nothing changed each time.
pub(super) async fn check_cut_refusals(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(17, 0)).await.unwrap();
    let cut = |from, to| s.cut(added.id, from, to);

    let outside = Err(SessionError::OutsideSession);
    assert_eq!(cut(at(18, 0), at(19, 0)).await, outside);
    assert_eq!(cut(at(7, 0), at(9, 0)).await, outside); // only touches the start
    let whole = Err(SessionError::WholeSession);
    assert_eq!(cut(at(8, 0), at(18, 0)).await, whole);
    assert_eq!(cut(at(9, 0), at(17, 0)).await, whole);
    let backwards = Err(SessionError::EndBeforeStart);
    assert_eq!(cut(at(13, 0), at(12, 0)).await, backwards);
    assert_eq!(cut(at(12, 0), at(12, 0)).await, backwards);

    assert_eq!(visible(&s).await, vec![added.clone()]);
}

/// Cutting a running session (lunch break): the last piece keeps running.
pub(super) async fn check_cut_running_keeps_running(s: impl SessionStore) {
    let running = s
        .start(task("lab 3"), SessionSource::Manual, at(9, 0))
        .await
        .unwrap();

    let left = s.cut(running.id, at(12, 0), at(13, 0)).await.unwrap();

    assert_eq!(left.len(), 2);
    assert_eq!((left[0].start, left[0].end), (at(9, 0), Some(at(12, 0))));
    assert_eq!((left[1].start, left[1].end), (at(13, 0), None));
    assert_eq!(s.running().await.unwrap(), Some(left[1].clone()));

    // over the start of the running piece: trimmed, still running
    let trimmed = s.cut(left[1].id, at(12, 30), at(13, 30)).await.unwrap();
    assert_eq!((trimmed[0].start, trimmed[0].end), (at(13, 30), None));
    assert_eq!(s.running().await.unwrap(), Some(trimmed[0].clone()));
}

// --------------- delete ---------------

/// A deleted session is gone from `query`, but not removed.
pub(super) async fn check_delete_hides(s: impl SessionStore) {
    let added = s.add(task("lab 3"), at(9, 0), at(10, 0)).await.unwrap();

    s.delete(added.id, at(12, 0)).await.unwrap();

    assert!(visible(&s).await.is_empty());
    let kept = everything(&s).await;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].id, added.id);
    assert_eq!(kept[0].deleted_at, Some(at(12, 0)));
}

/// Deleting the running session stops it; nothing runs afterwards, and a new
/// timer can start.
pub(super) async fn check_delete_running_stops(s: impl SessionStore) {
    let running = s
        .start(task("lab 3"), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();

    s.delete(running.id, at(15, 0)).await.unwrap();

    assert_eq!(s.running().await.unwrap(), None);
    let kept = &everything(&s).await[0];
    assert_eq!(kept.end, Some(at(15, 0)));
    assert_eq!(kept.deleted_at, Some(at(15, 0)));
    s.start(task("reading"), SessionSource::Manual, at(16, 0))
        .await
        .unwrap();
}

/// `edit`, `split`, `delete` with an unknown or deleted id: `NotFound`.
pub(super) async fn check_unknown_id_is_not_found(s: impl SessionStore) {
    let err = Err(SessionError::NotFound);
    let ghost = SessionId::new();
    assert_eq!(s.edit(ghost, patch(Some(at(9, 0)), None)).await, err);
    assert_eq!(s.split(ghost, at(9, 0)).await, Err(SessionError::NotFound));
    assert_eq!(s.delete(ghost, at(9, 0)).await, err);

    let added = s.add(task("lab 3"), at(9, 0), at(10, 0)).await.unwrap();
    s.delete(added.id, at(12, 0)).await.unwrap();
    assert_eq!(s.edit(added.id, patch(Some(at(9, 30)), None)).await, err);
    assert_eq!(s.delete(added.id, at(13, 0)).await, err);
}

// --------------- query ---------------

/// `tasks` keeps only sessions of those tasks.
pub(super) async fn check_query_by_task(s: impl SessionStore) {
    let (a, b) = (task("a"), task("b"));
    s.add(a.clone(), at(9, 0), at(10, 0)).await.unwrap();
    s.add(b.clone(), at(10, 0), at(11, 0)).await.unwrap();
    s.add(a.clone(), at(11, 0), at(12, 0)).await.unwrap();

    let only = |ids: Vec<NodeId>| SessionQuery {
        tasks: Some(ids),
        ..Default::default()
    };
    let of_a = s.query(&only(vec![a.id])).await.unwrap();
    assert_eq!(of_a.len(), 2);
    assert!(of_a.iter().all(|x| x.task.id == a.id));
    assert_eq!(s.query(&only(vec![a.id, b.id])).await.unwrap().len(), 3);
    assert!(s.query(&only(vec![])).await.unwrap().is_empty());
}

/// `from` / `to` keep every session that intersects `[from, to)`; results
/// are sorted by start.
pub(super) async fn check_query_range_intersects(s: impl SessionStore) {
    let c = s.add(task("c"), at(12, 0), at(13, 0)).await.unwrap(); // added out of order
    let a = s.add(task("a"), at(9, 0), at(10, 0)).await.unwrap();
    let b = s.add(task("b"), at(10, 30), at(11, 0)).await.unwrap();
    let running = s
        .start(task("d"), SessionSource::Manual, at(14, 0))
        .await
        .unwrap();

    let range = |from, to| SessionQuery {
        from,
        to,
        ..Default::default()
    };
    let ids = |found: Vec<Session>| found.into_iter().map(|x| x.id).collect::<Vec<_>>();

    // 9:30–10:30: a sticks in from before; b starts exactly at `to` (excluded)
    let found = s
        .query(&range(Some(at(9, 30)), Some(at(10, 30))))
        .await
        .unwrap();
    assert_eq!(ids(found), vec![a.id]);
    // 10:00–: a ends exactly at `from` (excluded); the running one is open-ended
    let found = s.query(&range(Some(at(10, 0)), None)).await.unwrap();
    assert_eq!(ids(found), vec![b.id, c.id, running.id]);
    // everything, sorted by start
    assert_eq!(ids(visible(&s).await), vec![a.id, b.id, c.id, running.id]);
}

/// Deleted sessions only with `include_deleted`.
pub(super) async fn check_query_include_deleted(s: impl SessionStore) {
    let a = s.add(task("a"), at(9, 0), at(10, 0)).await.unwrap();
    let b = s.add(task("b"), at(10, 0), at(11, 0)).await.unwrap();

    s.delete(a.id, at(12, 0)).await.unwrap();

    assert_eq!(visible(&s).await, vec![b.clone()]);
    let ids: Vec<_> = everything(&s).await.into_iter().map(|x| x.id).collect();
    assert_eq!(ids, vec![a.id, b.id]);
}
