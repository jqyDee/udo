//! Session rules every backend shares: when times make sense, when two
//! sessions intersect, what an edit, split or cut does. Pure: times in, a
//! decision out; each backend only stores the result.
//!
//! Filtering many sessions (`query`, overlap checks) is done by each
//! backend in its own way (a SQL backend in SQL, for its indexes); there
//! `intersects` is the rule to translate, and the contract checks the
//! translation.

use crate::model::{
    sessions::{SessionError, SessionPatch},
    time::Time,
};

/// A manual session `[start, end)`: the end must be after the start
/// (0-minute sessions included).
pub(super) fn check_span(start: Time, end: Time) -> Result<(), SessionError> {
    if end <= start {
        return Err(SessionError::EndBeforeStart);
    }
    Ok(())
}

/// Does `[start, end)` intersect `[from, to)`? `end: None` = running (open);
/// `from` / `to: None` = unbounded on that side. Half-open: touching is not
/// intersecting (14–15 and 15–16).
pub(super) fn intersects(
    start: Time,
    end: Option<Time>,
    from: Option<Time>,
    to: Option<Time>,
) -> bool {
    let starts_before_to = to.is_none_or(|to| start < to);
    let ends_after_from = from.is_none_or(|from| end.is_none_or(|end| from < end));
    starts_before_to && ends_after_from
}

/// The span of a session `[start, end)` after `patch`. Overlaps with other
/// sessions are the backend's check (it knows them).
pub(super) fn plan_edit(
    start: Time,
    end: Option<Time>,
    patch: &SessionPatch,
) -> Result<(Time, Option<Time>), SessionError> {
    if end.is_none() && patch.end.is_some() {
        return Err(SessionError::Running); // use `stop`
    }
    let start = patch.start.unwrap_or(start);
    let end = patch.end.or(end);
    if end.is_some_and(|end| end <= start) {
        return Err(SessionError::EndBeforeStart);
    }
    Ok((start, end))
}

/// A split: the first piece now ends `at`, a second one runs `at..end`.
pub(super) struct Split {
    pub at: Time,
    pub end: Time,
}

/// Split a session `[start, end)` at `at`. `None`: `at` is on an edge, so
/// nothing changes (it would give a 0-minute half).
pub(super) fn plan_split(
    start: Time,
    end: Option<Time>,
    at: Time,
) -> Result<Option<Split>, SessionError> {
    let Some(end) = end else {
        return Err(SessionError::Running);
    };
    if at < start || at > end {
        return Err(SessionError::OutsideSession);
    }
    if at == start || at == end {
        return Ok(None);
    }
    Ok(Some(Split { at, end }))
}

/// What a cut does to a session.
pub(super) enum Cut {
    /// Over the start: the session now starts here.
    TrimStart(Time),
    /// Over the end: the session now ends here.
    TrimEnd(Time),
    /// Inside: the session now ends at `from`; a new piece runs from `to` to
    /// the old end (and keeps running, if the session was).
    Gap { from: Time, to: Time },
}

/// Cut `[from, to)` out of a session `[start, end)`, clamped to it.
pub(super) fn plan_cut(
    start: Time,
    end: Option<Time>,
    from: Time,
    to: Time,
) -> Result<Cut, SessionError> {
    if to <= from {
        return Err(SessionError::EndBeforeStart); // the range itself is broken
    }
    // clamp to the session; a running one has no end to clamp to
    let from = from.max(start);
    let to = end.map_or(to, |end| to.min(end));
    if from >= to {
        return Err(SessionError::OutsideSession); // no shared moment
    }
    if from == start && Some(to) == end {
        return Err(SessionError::WholeSession); // use `delete`
    }
    Ok(if from == start {
        Cut::TrimStart(to)
    } else if Some(to) == end {
        Cut::TrimEnd(from)
    } else {
        Cut::Gap { from, to }
    })
}
