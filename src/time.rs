use std::collections::BTreeMap;
use std::fmt;

use chrono::{Duration, IsoWeek, Local, NaiveDateTime, TimeZone};

use db::DatabaseError;

/// A newtype for displaying durations in our desired format, so this data can be easily rendered
/// in Askama templates.
#[derive(Clone, Copy, Debug)]
pub struct Elapsed(pub Duration);
impl fmt::Display for Elapsed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const MINUTES_IN_HOUR: i64 = 60;
        let t = self.0.num_minutes();
        let h = t / MINUTES_IN_HOUR;
        let m = t % MINUTES_IN_HOUR;
        write!(f, "{:.2}h{:.2}m", h, m)
    }
}
impl ::std::ops::Add for Elapsed {
    type Output = Elapsed;
    fn add(self, other: Elapsed) -> Elapsed {
        Elapsed(self.0 + other.0)
    }
}
impl ::std::ops::AddAssign for Elapsed {
    fn add_assign(&mut self, other: Elapsed) {
        self.0 = self.0 + other.0;
    }
}
impl<'a> ::std::ops::AddAssign<&'a Elapsed> for Elapsed {
    fn add_assign(&mut self, other: &'a Elapsed) {
        self.0 = self.0 + other.0;
    }
}

/// A newtype for displaying weeks in our desired format, so this data can be easily rendered in
/// Askama templates.
pub struct Week(pub IsoWeek);
impl fmt::Display for Week {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// Convert a NaiveDateTime in UTC to a NaiveDateTime in the local time zone.
/// This is less than ideal.  See the comments in the Event struct.
pub fn to_utc(local_datetime: &NaiveDateTime) -> Result<NaiveDateTime, DatabaseError> {
    use chrono::offset::LocalResult;
    match Local.from_local_datetime(local_datetime) {
        LocalResult::None => Err(DatabaseError::BadTime),
        LocalResult::Single(t) => Ok(t),
        LocalResult::Ambiguous(_, _) => Err(DatabaseError::BadTime),
    }.map(|t| t.naive_utc())
}

/// Convert a NaiveDateTime in the local time zone to a NaiveDateTime in UTC.
/// This is less than ideal.  See the comments in the Event struct.
pub fn to_local(utc_datetime: &NaiveDateTime) -> NaiveDateTime {
    Local.from_utc_datetime(utc_datetime).naive_local()
}

/// Represent an amount of work time in both gross and net forms.
#[derive(Clone, Copy, Debug)]
pub struct WorkTime {
    pub gross: Elapsed,
    pub net: Elapsed,
}
impl WorkTime {
    pub fn new() -> WorkTime {
        WorkTime {
            gross: Elapsed(Duration::zero()),
            net: Elapsed(Duration::zero()),
        }
    }
    pub fn from_duration(gross: Duration, overhead: Duration) -> WorkTime {
        let net = if overhead > gross {
            Duration::zero()
        } else {
            gross - overhead
        };
        WorkTime {
            gross: Elapsed(gross),
            net: Elapsed(net),
        }
    }
    pub fn flatten_map<T>(map: BTreeMap<T, WorkTime>) -> Vec<(T, WorkTime)> {
        let mut elements: Vec<(T, WorkTime)> = Vec::with_capacity(map.len());
        for (t, worktime) in map {
            elements.push((t, worktime));
        }
        elements
    }
}
impl ::std::ops::AddAssign for WorkTime {
    fn add_assign(&mut self, other: WorkTime) {
        self.gross = self.gross + other.gross;
        self.net = self.net + other.net;
    }
}
impl<'a> ::std::ops::AddAssign<&'a WorkTime> for WorkTime {
    fn add_assign(&mut self, other: &'a WorkTime) {
        self.gross = self.gross + other.gross;
        self.net = self.net + other.net;
    }
}

/// Represent a specific work session.
#[derive(Debug)]
pub struct Interval {
    pub start: NaiveDateTime,
    pub work_time: WorkTime,
}
impl Interval {
    pub fn new(start: &NaiveDateTime, end: &NaiveDateTime, overhead: Duration) -> Interval {
        Interval {
            start: start.clone(),
            work_time: WorkTime::from_duration(*end - *start, overhead),
        }
    }
}
