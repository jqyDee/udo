//! The rules every `EstimateStore` must keep, checked against each backend
//! (`estimate_store_contract!`): append-only, timed by the store's clock,
//! `last_of` the latest row of a task, `of_tasks` filtered and oldest first,
//! every field back as written.

use std::sync::atomic::{AtomicI64, Ordering};

use chrono::TimeDelta;

use crate::{
    model::{
        estimate_store::{EstimateStore, NewEstimate, Reason, Recorded},
        id::NodeId,
        time::{Minutes, Time},
    },
    test_util::at,
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

/// The rules every EstimateStore must keep, one `#[tokio::test]` per check,
/// so a failing check does not hide the others. `$make` takes a `Clock` and
/// gives a fresh, empty store using it (most checks with `ticking`, the
/// one about equal times with `standing`). Use inside a backend's test
/// module:
///
/// ```ignore
/// super::contract::estimate_store_contract!(MemoryEstimates::new);
/// ```
macro_rules! estimate_store_contract {
    ($make:expr) => {
        $crate::storage::estimates::contract::estimate_store_contract!(@each $make, ticking;
            check_record_gives_an_id_and_the_clocks_time,
            check_record_appends_never_replaces,
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
pub(crate) use estimate_store_contract;

// --------------- record ---------------

/// `record` returns the row as written, with a fresh id and the store's
/// time (not one passed in: rows are bookkeeping, like `created_at`).
pub(super) async fn check_record_gives_an_id_and_the_clocks_time(s: impl EstimateStore) {
    let before = ticking();
    let a = row(NodeId::new(), 185);

    let first = s.record(a.clone()).await.unwrap();
    let second = s.record(a.clone()).await.unwrap();

    assert_eq!(first.estimate, a);
    assert_ne!(first.id, second.id);
    assert!(first.at > before, "{} not after {before}", first.at);
    assert!(second.at > first.at);
}

/// The same row twice is two rows: append-only, nothing is merged or
/// replaced (deduplicating is `Core`'s rule, not the store's).
pub(super) async fn check_record_appends_never_replaces(s: impl EstimateStore) {
    let task = NodeId::new();

    s.record(row(task, 185)).await.unwrap();
    s.record(row(task, 185)).await.unwrap();

    assert_eq!(s.of_tasks(&[task]).await.unwrap().len(), 2);
}

// --------------- last_of ---------------

pub(super) async fn check_last_of_is_the_latest_row(s: impl EstimateStore) {
    let task = NodeId::new();
    s.record(row(task, 180)).await.unwrap();
    let latest = s.record(row(task, 185)).await.unwrap();

    assert_eq!(s.last_of(task).await.unwrap(), Some(latest));
}

/// A later row of another task does not count.
pub(super) async fn check_last_of_ignores_other_tasks(s: impl EstimateStore) {
    let (a, b) = (NodeId::new(), NodeId::new());
    let of_a = s.record(row(a, 180)).await.unwrap();
    s.record(row(b, 60)).await.unwrap();

    assert_eq!(s.last_of(a).await.unwrap(), Some(of_a));
}

pub(super) async fn check_last_of_without_rows_is_none(s: impl EstimateStore) {
    s.record(row(NodeId::new(), 180)).await.unwrap(); // another task's

    assert_eq!(s.last_of(NodeId::new()).await.unwrap(), None);
}

// --------------- of_tasks ---------------

/// Only the asked tasks, oldest first, whatever order they are asked in.
pub(super) async fn check_of_tasks_filters_and_sorts_oldest_first(s: impl EstimateStore) {
    let (a, b, c) = (NodeId::new(), NodeId::new(), NodeId::new());
    let a1 = s.record(row(a, 180)).await.unwrap();
    s.record(row(b, 60)).await.unwrap();
    let c1 = s.record(row(c, 90)).await.unwrap();
    let a2 = s.record(row(a, 185)).await.unwrap();

    let rows = s.of_tasks(&[c, a]).await.unwrap();

    assert_eq!(rows, vec![a1, c1, a2]);
}

/// A task asked for twice still gives each of its rows once (a backend that
/// queries per task and merges would repeat them).
pub(super) async fn check_of_tasks_asked_twice_gives_each_row_once(s: impl EstimateStore) {
    let (a, b) = (NodeId::new(), NodeId::new());
    let a1 = s.record(row(a, 180)).await.unwrap();
    let b1 = s.record(row(b, 60)).await.unwrap();
    let a2 = s.record(row(a, 185)).await.unwrap();

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
    let one = s.record(row(task, 180)).await.unwrap();
    let two = s.record(row(task, 185)).await.unwrap();
    assert_eq!(one.at, two.at, "the standing clock gives one time");
    let (first, latest) = if one.id < two.id {
        (one, two)
    } else {
        (two, one)
    };

    assert_eq!(s.last_of(task).await.unwrap(), Some(latest.clone()));
    assert_eq!(s.of_tasks(&[task]).await.unwrap(), vec![first, latest]);
}

pub(super) async fn check_of_no_tasks_is_empty(s: impl EstimateStore) {
    s.record(row(NodeId::new(), 180)).await.unwrap();

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

    let first = s.record(from_prior).await.unwrap();
    let second = s.record(learned_without_prior).await.unwrap();

    assert_eq!(
        s.of_tasks(&[task]).await.unwrap(),
        vec![first, second.clone()]
    );
    assert_eq!(s.last_of(task).await.unwrap(), Some(second));
}
