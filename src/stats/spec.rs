//! The serde enums a query is written in (CLI flags parse into them, `--json`
//! echoes them, an AI tool sends them). Each builds its engine object.

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::stats::{
    dims::{
        Dim,
        calendar::{Calendar, Unit},
        hour::Hour,
        node::Node,
        weekday::Weekday,
    },
    measures::{Measure, minutes::MinutesMeasure, sessions::SessionsMeasure},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "dim", rename_all = "snake_case")]
pub enum DimSpec {
    Calendar { unit: Unit, every: u32 },
    Hour,
    Weekday,
    Node { level: Level },
}

impl DimSpec {
    pub fn build<Tz: TimeZone + 'static>(&self) -> Box<dyn Dim<Tz>> {
        match *self {
            DimSpec::Calendar { unit, every } => Box::new(Calendar { unit, every }),
            DimSpec::Hour => Box::new(Hour),
            DimSpec::Weekday => Box::new(Weekday),
            DimSpec::Node { level } => Box::new(Node::new(level)),
        }
    }

    /// The name of the dimension it builds (`Dim::name`), without a zone.
    pub fn name(&self) -> &'static str {
        self.build::<Utc>().name()
    }
}

/// What a node dimension groups by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Task,
    Project,
    Workspace,
    Child,
}

impl Level {
    /// Name in JSON keys and CLI flags.
    pub fn name(self) -> &'static str {
        match self {
            Level::Task => "task",
            Level::Project => "project",
            Level::Workspace => "workspace",
            Level::Child => "child",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasureSpec {
    Minutes,
    Sessions,
}

impl MeasureSpec {
    pub fn build(&self) -> Box<dyn Measure> {
        match self {
            MeasureSpec::Minutes => Box::new(MinutesMeasure),
            MeasureSpec::Sessions => Box::new(SessionsMeasure),
        }
    }
}
