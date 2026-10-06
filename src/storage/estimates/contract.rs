//! The rules every `EstimateStore` must keep, checked against each backend
//! (`estimate_store_contract!`): append-only, a repeat of the task's latest
//! row skipped, timed by the store's clock, `last_of` the latest row of a
//! task, `of_tasks` filtered and oldest first, every field back as written.

use std::{
    cell::Cell,
    sync::atomic::{AtomicI64, Ordering},
};

use chrono::{FixedOffset, TimeDelta};

use crate::{
    estimate::store::{EstimateStore, NewEstimate, Reason, Recorded}, model::{
        id::NodeId,
        time::{Minutes, Time},
    }, test_util::{at, parse_time},
};

/// The stores' clock in the contract: 20:00, one second later on every
/// call, so rows written one after the other have different times (a
/// `Clock` is a plain `fn`, it cannot count per store). Shared by all
/// tests, which only ever need "later than before".
pub(super) fn ticking() -> Time {
    static TICKS: AtomicI64 = AtomicI64::new(0);
    at(20, 0) + TimeDelta::seconds(TICKS.fetch_add(1, Ordering::Relaxed))
}

/// A clock that never moves: every row gets the same time, as two rows
/// written within one millisecond do (a task created and started at once).
pub(super) fn standing() -> Time {
    at(20, 0)
}

/// A clock over the autumn DST switch (2026-10-25, Vienna): 40 minutes
/// later on every call, alternately in summer (+02:00) and winter time
/// (+01:00), so every second call is earlier on the wall clock than the one
/// before (02:30+02:00, then 02:10+01:00). Counts per thread, not shared
/// like `ticking`: the alternation must not interleave with another test
/// (each `#[tokio::test]` runs on its own thread, and the stores read the
/// clock there, before any `spawn_blocking`).
pub(super) fn switching() -> Time {
    thread_local! {
        static CALLS: Cell<i32> = const { Cell::new(0) };
    }
    let n = CALLS.with(|c| c.replace(c.get() + 1));
    let utc = parse_time("2026-10-25T00:30:00+00:00") + TimeDelta::minutes(40 * i64::from(n));
    let hours = if n % 2 == 0 { 2 } else { 1 };
    utc.with_timezone(&FixedOffset::east_opt(hours * 3600).unwrap())
}

/// A learned estimate of `task`: `minutes`, 1 done task, prior 1h30,
/// written when the task got its first session.
fn row(task: NodeId, minutes: u32) -> NewEstimate {
    NewEstimate {
        task,
        minutes: Minutes::new(minutes),
        method: "average".into(),
        version: 1,
        done_tasks: 1,
        open_tasks: 0,
        prior: Some(Minutes::new(90)),
        reason: Reason::Started,
    }
}

/// `record` a row that must be written (not a repeat): the stored row.
async fn write(s: &impl EstimateStore, row: NewEstimate) -> Recorded {
    s.record(row)
        .await
        .unwrap()
        .expect("written, not skipped as a repeat")
}

/// The rules every EstimateStore must keep, one `#[tokio::test]` per check,
/// so a failing check does not hide the others. `$make` takes a `Clock` and
/// gives a fresh, empty store using it (most checks with `ticking`, the
/// one about equal times with `standing`, the one about the DST switch with
/// `switching`). Use inside a test module, on the `Estimates` enum:
///
/// ```ignore
/// contract::estimate_store_contract!(|clock: Clock| Estimates::Memory(MemoryEstimates::new(clock)));
/// ```
macro_rules! estimate_store_contract {
    ($make:expr) => {
        $crate::storage::estimates::contract::estimate_store_contract!(@each $make, ticking;
            check_record_gives_an_id_and_the_clocks_time,
            check_record_skips_a_repeat,
            check_record_ignores_the_reason,
            check_record_writes_a_change,
            check_record_compares_with_the_latest_only,
            check_last_of_is_the_latest_row,
            check_last_of_ignores_other_tasks,
            check_last_of_without_rows_is_none,
            check_of_tasks_filters_and_sorts_oldest_first,
            check_of_tasks_asked_twice_gives_each_row_once,
            check_of_no_tasks_is_empty,
            check_rows_keep_every_field,
        );
        $crate::storage::estimates::contract::estimate_store_contract!(@each $make, standing;
            check_equal_times_order_by_id,
        );
        $crate::storage::estimates::contract::estimate_store_contract!(@each $make, switching;
            check_order_follows_the_instant_not_the_wall_clock,
        );
    };
    (@each $make:expr, $clock:ident; $($check:ident),* $(,)?) => {
        $(
            #[tokio::test]
            async fn $check() {
                $crate::storage::estimates::contract::$check(
                    ($make)($crate::storage::estimates::contract::$clock),
                )
                .await;
            }
        )*
    };
}
pub(super) use estimate_store_contract;

// --------------- record ---------------

/// `record` returns the row as written, with a fresh id and the store's
/// time (not one passed in: rows are bookkeeping, like `created_at`).
pub(super) async fn check_record_gives_an_id_and_the_clocks_time(s: impl EstimateStore) {
    let before = ticking();
    let task = NodeId::new();
    let a = row(task, 180);

    let first = write(&s, a.clone()).await;
    let second = write(&s, row(task, 185)).await;

    assert_eq!(first.estimate, a);
    assert_ne!(first.id, second.id);
    assert!(first.at > before, "{} not after {before}", first.at);
    assert!(second.at > first.at);
}

/// The same minutes and method again: skipped, nothing written.
pub(super) async fn check_record_skips_a_repeat(s: impl EstimateStore) {
    let task = NodeId::new();
    let first = write(&s, row(task, 185)).await;

    let again = s.record(row(task, 185)).await.unwrap();

    assert_eq!(again, None);
    assert_eq!(s.of_tasks(&[task]).await.unwrap(), vec![first]);
}

/// A task started with the estimate it was created with: the `started`
/// row would only repeat the `created` one, so it is skipped.
pub(super) async fn check_record_ignores_the_reason(s: impl EstimateStore) {
    let task = NodeId::new();
    let created = NewEstimate {
        reason: Reason::Created,
        ..row(task, 185)
    };
    write(&s, created).await;

    let started = s.record(row(task, 185)).await.unwrap();

    assert_eq!(started, None);
}

/// Other minutes or another method is something new: written, appended
/// beside the earlier rows (nothing is merged or replaced).
pub(super) async fn check_record_writes_a_change(s: impl EstimateStore) {
    let task = NodeId::new();
    write(&s, row(task, 180)).await;

    let minutes = s.record(row(task, 185)).await.unwrap();
    let method = NewEstimate {
        method: "prior".into(),
        ..row(task, 185)
    };
    let method = s.record(method).await.unwrap();

    assert!(minutes.is_some());
    assert!(method.is_some());
    assert_eq!(s.of_tasks(&[task]).await.unwrap().len(), 3);
}

/// Only the task's latest row counts: back to an older value is a change,
/// and another task's row does not stop a task's first one.
pub(super) async fn check_record_compares_with_the_latest_only(s: impl EstimateStore) {
    let (a, b) = (NodeId::new(), NodeId::new());
    write(&s, row(a, 180)).await;
    write(&s, row(a, 185)).await;

    let back = s.record(row(a, 180)).await.unwrap();
    let other = s.record(row(b, 180)).await.unwrap();

    assert!(back.is_some(), "180 repeats an older row, not the latest");
    assert!(other.is_some(), "a's rows do not count for b");
}

// --------------- last_of ---------------

pub(super) async fn check_last_of_is_the_latest_row(s: impl EstimateStore) {
    let task = NodeId::new();
    write(&s, row(task, 180)).await;
    let latest = write(&s, row(task, 185)).await;

    assert_eq!(s.last_of(task).await.unwrap(), Some(latest));
}

/// A later row of another task does not count.
pub(super) async fn check_last_of_ignores_other_tasks(s: impl EstimateStore) {
    let (a, b) = (NodeId::new(), NodeId::new());
    let of_a = write(&s, row(a, 180)).await;
    write(&s, row(b, 60)).await;

    assert_eq!(s.last_of(a).await.unwrap(), Some(of_a));
}

pub(super) async fn check_last_of_without_rows_is_none(s: impl EstimateStore) {
    write(&s, row(NodeId::new(), 180)).await; // another task's

    assert_eq!(s.last_of(NodeId::new()).await.unwrap(), None);
}

// --------------- of_tasks ---------------

/// Only the asked tasks, oldest first, whatever order they are asked in.
pub(super) async fn check_of_tasks_filters_and_sorts_oldest_first(s: impl EstimateStore) {
    let (a, b, c) = (NodeId::new(), NodeId::new(), NodeId::new());
    let a1 = write(&s, row(a, 180)).await;
    write(&s, row(b, 60)).await;
    let c1 = write(&s, row(c, 90)).await;
    let a2 = write(&s, row(a, 185)).await;

    let rows = s.of_tasks(&[c, a]).await.unwrap();

    assert_eq!(rows, vec![a1, c1, a2]);
}

/// A task asked for twice still gives each of its rows once (a backend that
/// queries per task and merges would repeat them).
pub(super) async fn check_of_tasks_asked_twice_gives_each_row_once(s: impl EstimateStore) {
    let (a, b) = (NodeId::new(), NodeId::new());
    let a1 = write(&s, row(a, 180)).await;
    let b1 = write(&s, row(b, 60)).await;
    let a2 = write(&s, row(a, 185)).await;

    let rows = s.of_tasks(&[a, b, a]).await.unwrap();

    assert_eq!(rows, vec![a1, b1, a2]);
}

// --------------- equal times ---------------

/// Rows with the same time (one millisecond, `standing` clock) are ordered
/// by id, the second key, in `last_of` and `of_tasks` alike, so every
/// backend picks the same "latest". Which of the two has the larger id is
/// up to the ids; the rule is that both methods agree with it.
pub(super) async fn check_equal_times_order_by_id(s: impl EstimateStore) {
    let task = NodeId::new();
    let one = write(&s, row(task, 180)).await;
    let two = write(&s, row(task, 185)).await;
    assert_eq!(one.at, two.at, "the standing clock gives one time");
    let (first, latest) = if one.id < two.id {
        (one, two)
    } else {
        (two, one)
    };

    assert_eq!(s.last_of(task).await.unwrap(), Some(latest.clone()));
    assert_eq!(s.of_tasks(&[task]).await.unwrap(), vec![first, latest]);
}

/// Across the DST switch the wall clock goes back (02:30, then 02:10 an
/// hour-offset later), real time goes on: the later row is the later one
/// by instant, in `last_of` and `of_tasks` alike. Sorting by local time
/// (or by a formatted string) would pick the wrong "latest".
pub(super) async fn check_order_follows_the_instant_not_the_wall_clock(s: impl EstimateStore) {
    let task = NodeId::new();
    let summer = write(&s, row(task, 180)).await;
    let winter = write(&s, row(task, 185)).await;
    assert!(
        winter.at > summer.at && winter.at.naive_local() < summer.at.naive_local(),
        "the switching clock gives a later instant at an earlier wall time"
    );

    assert_eq!(s.last_of(task).await.unwrap(), Some(winter.clone()));
    assert_eq!(s.of_tasks(&[task]).await.unwrap(), vec![summer, winter]);
}

pub(super) async fn check_of_no_tasks_is_empty(s: impl EstimateStore) {
    write(&s, row(NodeId::new(), 180)).await;

    assert_eq!(s.of_tasks(&[]).await.unwrap(), Vec::<Recorded>::new());
}

// --------------- fields ---------------

/// Every field comes back exactly as written, both reasons, a missing prior
/// and the time with its offset included: what `record` returned is what
/// the store holds.
pub(super) async fn check_rows_keep_every_field(s: impl EstimateStore) {
    let task = NodeId::new();
    let from_prior = NewEstimate {
        task,
        minutes: Minutes::new(96),
        method: "prior".into(),
        version: 1,
        done_tasks: 0,
        open_tasks: 0,
        prior: Some(Minutes::new(96)),
        reason: Reason::Created,
    };
    let learned_without_prior = NewEstimate {
        task,
        minutes: Minutes::new(107),
        method: "average".into(),
        version: 2,
        done_tasks: 1,
        open_tasks: 1,
        prior: None,
        reason: Reason::Started,
    };

    let first = write(&s, from_prior).await;
    let second = write(&s, learned_without_prior).await;

    assert_eq!(
        s.of_tasks(&[task]).await.unwrap(),
        vec![first, second.clone()]
    );
    assert_eq!(s.last_of(task).await.unwrap(), Some(second));
}
