use super::*;
use crate::jurisdiction;

const MIN: i64 = MINUTE_MS;
const HOUR: i64 = 60 * MIN;
const VIENNA: Tz = chrono_tz::Europe::Vienna;

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

/// Local time in Vienna as unix ms.
fn at(day: &str, hhmm: &str) -> i64 {
    let local = date(day).and_time(parse_hhmm(hhmm).unwrap());
    VIENNA
        .from_local_datetime(&local)
        .single()
        .unwrap()
        .timestamp_millis()
}

fn schedule(target_minutes: u32, model: ScheduleModel) -> WorkSchedule {
    let day = |t| DayPlan {
        target_minutes: t,
        core_start: None,
        core_end: None,
    };
    WorkSchedule {
        id: "s".into(),
        valid_from: date("2020-01-01"),
        model,
        days: vec![
            day(target_minutes),
            day(target_minutes),
            day(target_minutes),
            day(target_minutes),
            day(target_minutes),
            day(0),
            day(0),
        ],
    }
}

struct Fixture {
    profile: Profile,
    schedules: Vec<WorkSchedule>,
    intervals: Vec<(i64, i64)>,
    holidays: HashMap<NaiveDate, String>,
    absences: HashMap<NaiveDate, Vec<AbsenceOnDay>>,
    adjustments: HashMap<NaiveDate, i64>,
    deduct: bool,
}

impl Fixture {
    fn new(country: &str) -> Self {
        Self {
            profile: jurisdiction::load(country, None).unwrap(),
            schedules: vec![schedule(8 * 60, ScheduleModel::Flextime)],
            intervals: vec![],
            holidays: HashMap::new(),
            absences: HashMap::new(),
            adjustments: HashMap::new(),
            deduct: true,
        }
    }

    fn work(&mut self, day: &str, from: &str, to: &str) -> &mut Self {
        self.intervals.push((at(day, from), at(day, to)));
        self
    }

    fn inputs(&self) -> Inputs<'_> {
        Inputs {
            tz: VIENNA,
            profile: &self.profile,
            schedules: &self.schedules,
            intervals: &self.intervals,
            holidays: &self.holidays,
            absences: &self.absences,
            adjustments: &self.adjustments,
            deduct_breaks: self.deduct,
        }
    }

    fn day(&self, day: &str) -> WorkDay {
        compute_day(date(day), &self.inputs(), None)
    }
}

#[test]
fn dst_days_have_23_and_25_hours() {
    let (s, e) = day_bounds(date("2026-03-29"), VIENNA);
    assert_eq!(e - s, 23 * HOUR);
    let (s, e) = day_bounds(date("2026-10-25"), VIENNA);
    assert_eq!(e - s, 25 * HOUR);
    let (s, e) = day_bounds(date("2026-06-01"), VIENNA);
    assert_eq!(e - s, 24 * HOUR);
}

#[test]
fn entries_crossing_midnight_are_split_between_days() {
    let mut f = Fixture::new("AT");
    f.intervals
        .push((at("2026-06-01", "22:00"), at("2026-06-02", "02:00")));
    assert_eq!(f.day("2026-06-01").worked_ms, 2 * HOUR);
    assert_eq!(f.day("2026-06-02").worked_ms, 2 * HOUR);
}

#[test]
fn overlapping_entries_count_once() {
    let mut f = Fixture::new("AT");
    f.work("2026-06-01", "08:00", "12:00")
        .work("2026-06-01", "11:00", "13:00");
    assert_eq!(f.day("2026-06-01").worked_ms, 5 * HOUR);
}

#[test]
fn missing_break_is_deducted_in_austria() {
    let mut f = Fixture::new("AT");
    f.work("2026-06-01", "08:00", "15:00"); // 7 h, no break
    let d = f.day("2026-06-01");
    assert_eq!(d.break_required_ms, 30 * MIN);
    assert_eq!(d.break_deducted_ms, 30 * MIN);
    assert_eq!(d.net_ms, 6 * HOUR + 30 * MIN);
    assert_eq!(d.delta_ms, -90 * MIN);
}

#[test]
fn gaps_count_as_break_when_long_enough() {
    let mut f = Fixture::new("AT");
    // Three 10-minute breaks are allowed in Austria.
    f.work("2026-06-01", "08:00", "10:00")
        .work("2026-06-01", "10:10", "12:00")
        .work("2026-06-01", "12:10", "14:00")
        .work("2026-06-01", "14:10", "16:00");
    let d = f.day("2026-06-01");
    assert_eq!(d.break_taken_ms, 30 * MIN);
    assert_eq!(d.break_deducted_ms, 0);

    // Five-minute gaps do not count.
    let mut f = Fixture::new("AT");
    f.work("2026-06-01", "08:00", "12:00")
        .work("2026-06-01", "12:05", "16:00");
    let d = f.day("2026-06-01");
    assert_eq!(d.break_taken_ms, 0);
    assert_eq!(d.break_deducted_ms, 30 * MIN);
}

#[test]
fn germany_requires_45_minutes_after_9_hours() {
    let mut f = Fixture::new("DE");
    f.work("2026-06-01", "07:00", "16:30"); // 9.5 h
    let d = f.day("2026-06-01");
    assert_eq!(d.break_deducted_ms, 45 * MIN);
}

#[test]
fn without_deduction_a_warning_is_shown() {
    let mut f = Fixture::new("AT");
    f.deduct = false;
    f.work("2026-06-01", "08:00", "15:00");
    let d = f.day("2026-06-01");
    assert_eq!(d.net_ms, 7 * HOUR);
    assert!(d.warnings.contains(&DayWarning::BreakTooShort {
        required_minutes: 30,
        taken_minutes: 0
    }));
}

#[test]
fn targets_respect_holidays_absences_and_models() {
    let mut f = Fixture::new("AT");
    f.holidays.insert(date("2026-06-04"), "Fronleichnam".into());
    f.absences.insert(
        date("2026-06-01"),
        vec![AbsenceOnDay {
            type_id: "vacation".into(),
            fraction: 0.5,
            reduces_target: true,
        }],
    );
    f.absences.insert(
        date("2026-06-02"),
        vec![AbsenceOnDay {
            type_id: "comp".into(),
            fraction: 1.0,
            reduces_target: false,
        }],
    );
    assert_eq!(f.day("2026-06-01").target_ms, 4 * HOUR);
    assert_eq!(
        f.day("2026-06-02").target_ms,
        8 * HOUR,
        "comp time keeps the target"
    );
    assert_eq!(
        f.day("2026-06-02").delta_ms,
        -8 * HOUR,
        "and is taken from the balance"
    );
    assert_eq!(f.day("2026-06-04").target_ms, 0);
    assert_eq!(f.day("2026-06-06").target_ms, 0, "Saturday");

    f.schedules = vec![schedule(8 * 60, ScheduleModel::Trust)];
    assert_eq!(f.day("2026-06-03").target_ms, 8 * HOUR);
    assert_eq!(f.day("2026-06-03").delta_ms, 0, "trust-based: no balance");
}

#[test]
fn schedules_apply_from_their_valid_date() {
    let mut f = Fixture::new("AT");
    let mut part_time = schedule(6 * 60, ScheduleModel::Flextime);
    part_time.valid_from = date("2026-06-03");
    f.schedules.push(part_time);
    assert_eq!(f.day("2026-06-02").target_ms, 8 * HOUR);
    assert_eq!(f.day("2026-06-03").target_ms, 6 * HOUR);
}

#[test]
fn legal_limit_warnings() {
    let mut f = Fixture::new("AT");
    f.work("2026-06-01", "06:00", "19:30"); // 13.5 h, net 13 h > 12 h
    f.work("2026-06-02", "05:00", "08:00"); // only 9.5 h rest
    let days = compute_days(date("2026-06-01"), date("2026-06-02"), &f.inputs(), None);
    assert!(days[0]
        .warnings
        .contains(&DayWarning::ExceedsDailyMax { max_minutes: 720 }));
    assert!(days[1].warnings.contains(&DayWarning::RestTooShort {
        rest_minutes: 570,
        min_minutes: 660
    }));
}

/// Reference month, computed by hand (see comments). October 2026,
/// Austria, 40 h week (8 h Mon–Fri). The month contains a public holiday
/// (Mon 26 Oct) and the end of DST (Sun 25 Oct).
#[test]
fn reference_month_balance() {
    let mut f = Fixture::new("AT");
    let profile_holidays: HashMap<NaiveDate, String> = f
        .profile
        .holidays(2026)
        .into_iter()
        .filter(|h| h.day_off)
        .map(|h| (h.date, h.id))
        .collect();
    f.holidays = profile_holidays;

    let start = date("2026-10-01");
    let end = date("2026-10-31");
    let special = ["2026-10-09", "2026-10-14", "2026-10-21"];
    let mut d = start;
    while d <= end {
        let s = d.to_string();
        let weekday = d.weekday().num_days_from_monday() < 5;
        if weekday && !f.holidays.contains_key(&d) && !special.contains(&s.as_str()) {
            // 18 normal days: 8:15 h with a 30 min lunch → +15 min each = +270 min
            f.work(&s, "08:00", "12:00").work(&s, "12:30", "16:45");
        }
        d += Duration::days(1);
    }
    // Fri 9 Oct: 7 h without break → 30 min deducted → 6:30 h, −90 min
    f.work("2026-10-09", "08:00", "15:00");
    // Wed 14 Oct: full vacation day → target 0, ±0
    f.absences.insert(
        date("2026-10-14"),
        vec![AbsenceOnDay {
            type_id: "vacation".into(),
            fraction: 1.0,
            reduces_target: true,
        }],
    );
    // Wed 21 Oct: half vacation, 4 h worked → target 4 h, ±0
    f.absences.insert(
        date("2026-10-21"),
        vec![AbsenceOnDay {
            type_id: "vacation".into(),
            fraction: 0.5,
            reduces_target: true,
        }],
    );
    f.work("2026-10-21", "12:30", "16:30");
    // Sat 3 Oct: 2 h → +120 min
    f.work("2026-10-03", "10:00", "12:00");
    // Manual correction on 31 Oct: −60 min
    f.adjustments.insert(date("2026-10-31"), -60);

    let days = compute_days(start, end, &f.inputs(), None);
    assert_eq!(days.len(), 31);
    let target: i64 = days.iter().map(|d| d.target_ms).sum();
    let net: i64 = days.iter().map(|d| d.net_ms).sum();
    assert_eq!(target, 156 * HOUR, "18 × 8 h + 4 h + 8 h");
    assert_eq!(net, 161 * HOUR, "18 × 8:15 h + 4 h + 6:30 h + 2 h");
    assert_eq!(balance_ms(&days), 240 * MIN, "+270 − 90 + 120 − 60");
    assert_eq!(days[25].holiday.as_deref(), Some("national_day"));
    assert!(days.iter().all(|d| d.warnings.is_empty()));
}
