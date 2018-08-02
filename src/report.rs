use std::collections::BTreeMap;
use std::fmt;

use chrono::{Datelike, Duration, IsoWeek, Local, NaiveDate, Weekday};
use diesel::prelude::*;

use db::{self, DatabaseError};
use models;
use models::*;
use schema;
use time::*;

/// A summary report contains information about work activity in recent days and weeks, and is used
/// to populate the dashboard.
pub struct SummaryReport {
    pub next_direction: PunchDirection,
    pub days: Vec<(NaiveDate, WorkTime)>,
    pub weeks: Vec<(Week, WorkTime)>,
    pub recent_events: Vec<Event>,
}

impl fmt::Display for SummaryReport {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Summary report:")?;
        writeln!(f, "\tNext expected direction: {:?}", self.next_direction)?;
        writeln!(f, "\tDays:")?;
        for day in &self.days {
            writeln!(f, "\t\t{}: {} {}", day.0, day.1.gross, day.1.net)?;
        }
        writeln!(f, "\tWeeks:")?;
        for week in &self.weeks {
            writeln!(f, "\t\t{}: {} {}", week.0, week.1.gross, week.1.net)?;
        }
        writeln!(f, "\tRecent events:")?;
        for event in &self.recent_events {
            writeln!(f, "\t\t{:?}", event)?;
        }
        Ok(())
    }
}

/// Generate a summary report.
pub fn summary_report(
    connection: &SqliteConnection,
    project_id: i64,
) -> Result<SummaryReport, DatabaseError> {
    const MAX_REPORT_EVENTS: usize = 10;
    const START_WEEKS_IN_PAST: i64 = 5;

    use self::schema::events::dsl as events_dsl;
    use self::schema::projects::dsl as projects_dsl;

    // Load the project.
    let project = projects_dsl::projects
        .filter(projects_dsl::id.eq(project_id))
        .first::<models::Project>(connection)
        .optional()?
        .ok_or(DatabaseError::BadProject)?;

    // Determine the Monday at or before 5 weeks ago
    let today = Local::now().naive_local().date();
    let mut start_day = today - Duration::weeks(START_WEEKS_IN_PAST);
    while start_day.weekday() != Weekday::Mon {
        start_day -= Duration::days(1);
    }
    let start_utc = to_utc(&start_day.and_hms(0, 0, 0))?;

    let events = events_dsl::events
        .filter(events_dsl::project_id.eq(project_id))
        .filter(
            events_dsl::event_type
                .eq(models::EventType::In)
                .or(events_dsl::event_type.eq(models::EventType::Out)),
        )
        .filter(events_dsl::clock.ge(start_utc))
        .order(events_dsl::clock)
        .load::<models::Event>(connection)?;

    // Step through events and formulate in-out intervals
    let mut expected_type = EventType::In;
    let mut last_in: Option<&Event> = None;
    let mut intervals: Vec<Interval> = Vec::with_capacity(events.len() / 2);
    let mut lead_in: bool = true;
    let overhead = Duration::minutes(project.overhead as i64);
    for event in &events {
        // Trim any leading "out" events without a warning since we can't create a valid interval
        // without the corresponding "in" event.  This can happen since we picked an arbitrary
        // point in time to start.  This is somewhat redundant with the expected_type check below,
        // except it generates a warning.
        if lead_in && event.event_type == EventType::Out {
            continue;
        }
        // We already made this restriction in the database query, but we'll eventually need to be
        // able to do something with Note events...
        if event.event_type != EventType::In && event.event_type != EventType::Out {
            continue;
        }
        if event.event_type != expected_type {
            warn!("Unexpected event: {:?}", event);
            continue;
        }
        lead_in = false;
        match event.event_type {
            EventType::In => {
                last_in = Some(event);
                expected_type = EventType::Out;
            }
            EventType::Out => {
                let interval = match last_in.take() {
                    Some(e) => {
                        Interval::new(&to_local(&e.clock), &to_local(&event.clock), overhead)
                    }
                    None => unreachable!(),
                };
                intervals.push(interval);
                expected_type = EventType::In;
            }
            _ => {}
        }
    }

    // Is there a work session in progress? If so, then account for its time to the present.
    if let Some(event) = last_in {
        let interval = Interval::new(
            &to_local(&event.clock),
            &Local::now().naive_local(),
            overhead,
        );
        intervals.push(interval);
    }

    // Allocate work time to days and weeks
    let mut day_map = BTreeMap::<NaiveDate, WorkTime>::new();
    let mut week_map = BTreeMap::<IsoWeek, WorkTime>::new();
    for interval in &intervals {
        // Allocate to days
        let day = interval.start.date();
        let mut entry = day_map.entry(day).or_insert(WorkTime::new());
        *entry += interval.work_time;

        // Allocate to weeks
        let week = day.iso_week();
        let mut entry = week_map.entry(week).or_insert(WorkTime::new());
        *entry += interval.work_time;
    }

    // Fill in empty days with zero values
    let mut day = start_day;
    while day <= today {
        day_map.entry(day).or_insert(WorkTime::new());
        day = day.succ();
    }

    // Fill in empty weeks with zero values
    let mut week = start_day.iso_week();
    while week <= today.iso_week() {
        week_map.entry(week).or_insert(WorkTime::new());
        week = (NaiveDate::from_isoywd(week.year(), week.week(), Weekday::Mon)
            + Duration::weeks(1))
            .iso_week();
    }

    // Flatten to vectors
    let mut days = WorkTime::flatten_map(day_map);
    let mut weeks = WorkTime::flatten_map(week_map);

    // Keep only the days from this week
    let keep_days = (today.weekday().num_days_from_monday() + 1) as usize;
    if days.len() > keep_days {
        let split_point = days.len() - keep_days;
        days = days.split_off(split_point);
    }

    // Keep only the last 10 events
    let mut recent_events = if events.len() > MAX_REPORT_EVENTS {
        let split_point = events.len() - MAX_REPORT_EVENTS;
        events[split_point..].to_vec()
    } else {
        events.clone()
    };

    // Reverse date order
    days.reverse();
    weeks.reverse();
    recent_events.reverse();

    Ok(SummaryReport {
        next_direction: db::next_expected_punch_direction(connection, project_id)?,
        days,
        weeks: weeks.iter().map(|(w, t)| (Week(*w), t.clone())).collect(),
        recent_events,
    })
}
