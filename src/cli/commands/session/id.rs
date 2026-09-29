//! Session IDs on the command line: shown by their last characters, found
//! by any unique ending.

use chrono::Local;

use crate::{
    DATE_FMT, Res,
    core::Core,
    model::sessions::{Session, SessionId, SessionQuery, SessionStore},
};

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
    use super::*;
    use crate::test_util::{at, core};

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

    #[tokio::test]
    async fn an_empty_ending_is_refused() {
        let (_tmp, core) = core().await;
        core.add_session(&[0], at(9, 0), at(10, 0)).await.unwrap();

        assert!(find(&core, "").await.is_err());
    }
}
