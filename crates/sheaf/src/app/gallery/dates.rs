//! When a note was last touched, in words: the groups of the gallery (Today,
//! Yesterday, Previous 7 Days, …, months) and the date under a card. Pure; `now` is
//! passed in so that tests and screenshots are stable.

use jiff::civil::{Date, Time};
use jiff::{Timestamp, ZonedDateTime};
use sheaf_core::i18n::tr;

/// A heading of the gallery. The order of the variants is the order on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Pinned,
    Today,
    Yesterday,
    Previous7Days,
    Previous30Days,
    /// Older notes, by month; later months first.
    Month {
        year: i16,
        month: i8,
    },
    /// Everything that is not pinned when there are no date groups (title order).
    Notes,
}

impl Group {
    /// The group of a note that is not pinned, by the date the sort order uses.
    pub fn of(date: Timestamp, now: &ZonedDateTime) -> Group {
        let day = date.to_zoned_datetime(now.time_zone().clone()).date();
        match days_between(day, now.date()) {
            ..=0 => Group::Today,
            1 => Group::Yesterday,
            2..=7 => Group::Previous7Days,
            8..=30 => Group::Previous30Days,
            _ => Group::Month {
                year: day.year(),
                month: day.month(),
            },
        }
    }

    pub fn label(self) -> String {
        match self {
            Group::Pinned => tr("Pinned"),
            Group::Today => tr("Today"),
            Group::Yesterday => tr("Yesterday"),
            Group::Previous7Days => tr("Previous 7 Days"),
            Group::Previous30Days => tr("Previous 30 Days"),
            Group::Month { year, month } => {
                // TRANSLATORS: a heading of the gallery, such as "October 2026".
                tr("{month} {year}")
                    .replace("{month}", &month_name(month))
                    .replace("{year}", &year.to_string())
            }
            Group::Notes => tr("Notes"),
        }
    }
}

/// The date under a card or in a list row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateLabel {
    /// Today: the time.
    Time(Time),
    Yesterday,
    /// Within the last week: the day of the week.
    Weekday(jiff::civil::Weekday),
    /// Earlier: the date, with the year if it is not this year's.
    Date {
        date: Date,
        with_year: bool,
    },
}

impl DateLabel {
    pub fn of(date: Timestamp, now: &ZonedDateTime) -> DateLabel {
        let zoned = date.to_zoned_datetime(now.time_zone().clone());
        let day = zoned.date();
        match days_between(day, now.date()) {
            ..=0 => DateLabel::Time(zoned.time()),
            1 => DateLabel::Yesterday,
            2..=6 => DateLabel::Weekday(day.weekday()),
            _ => DateLabel::Date {
                date: day,
                with_year: day.year() != now.year(),
            },
        }
    }

    pub fn format(self) -> String {
        match self {
            // TRANSLATORS: strftime format of a time of day today, as on a card.
            DateLabel::Time(time) => time.strftime(&tr("%H:%M")).to_string(),
            DateLabel::Yesterday => tr("Yesterday"),
            DateLabel::Weekday(weekday) => weekday_name(weekday),
            DateLabel::Date {
                date,
                with_year: false,
            } => {
                // TRANSLATORS: strftime format of a date in this year, as on a card.
                // Use numbers only (%m, %d) if month names would stay English.
                date.strftime(&tr("%b %-d")).to_string()
            }
            DateLabel::Date {
                date,
                with_year: true,
            } => {
                // TRANSLATORS: strftime format of a date in another year, as on a card.
                date.strftime(&tr("%b %-d, %Y")).to_string()
            }
        }
    }
}

/// Whole calendar days from `from` to `to`; negative if `from` is later.
fn days_between(from: Date, to: Date) -> i32 {
    to.since(from).map_or(0, |span| span.get_days())
}

fn month_name(month: i8) -> String {
    match month {
        1 => tr("January"),
        2 => tr("February"),
        3 => tr("March"),
        4 => tr("April"),
        5 => tr("May"),
        6 => tr("June"),
        7 => tr("July"),
        8 => tr("August"),
        9 => tr("September"),
        10 => tr("October"),
        11 => tr("November"),
        _ => tr("December"),
    }
}

fn weekday_name(weekday: jiff::civil::Weekday) -> String {
    use jiff::civil::Weekday;
    match weekday {
        Weekday::Monday => tr("Monday"),
        Weekday::Tuesday => tr("Tuesday"),
        Weekday::Wednesday => tr("Wednesday"),
        Weekday::Thursday => tr("Thursday"),
        Weekday::Friday => tr("Friday"),
        Weekday::Saturday => tr("Saturday"),
        Weekday::Sunday => tr("Sunday"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::tz::{TimeZone, offset};

    /// Monday, 16 November 2026, 13:24 in Berlin.
    fn now() -> ZonedDateTime {
        let time: Timestamp = "2026-11-16T13:24:00+01:00".parse().expect("a timestamp");
        time.to_zoned_datetime(TimeZone::fixed(offset(1)))
    }

    fn at(text: &str) -> Timestamp {
        text.parse().expect("a timestamp")
    }

    #[test]
    fn groups_follow_calendar_days_not_hours() {
        let now = now();
        let cases = [
            ("2026-11-16T13:30:00+01:00", Group::Today),
            ("2026-11-16T00:00:00+01:00", Group::Today),
            ("2026-11-15T23:59:00+01:00", Group::Yesterday),
            // 23:30 UTC on the 15th is already the 16th in Berlin.
            ("2026-11-15T23:30:00Z", Group::Today),
            ("2026-11-15T00:01:00+01:00", Group::Yesterday),
            ("2026-11-14T23:59:00+01:00", Group::Previous7Days),
            ("2026-11-09T00:00:00+01:00", Group::Previous7Days),
            ("2026-11-08T23:59:00+01:00", Group::Previous30Days),
            ("2026-10-17T12:00:00+02:00", Group::Previous30Days),
            (
                "2026-10-16T12:00:00+02:00",
                Group::Month {
                    year: 2026,
                    month: 10,
                },
            ),
            (
                "2025-12-31T12:00:00+01:00",
                Group::Month {
                    year: 2025,
                    month: 12,
                },
            ),
            // A clock that ran ahead does not make a group of its own.
            ("2026-11-20T12:00:00+01:00", Group::Today),
        ];
        for (date, group) in cases {
            assert_eq!(Group::of(at(date), &now), group, "{date}");
        }
    }

    #[test]
    fn groups_sort_in_screen_order() {
        let mut groups = vec![
            Group::Month {
                year: 2026,
                month: 9,
            },
            Group::Today,
            Group::Pinned,
            Group::Previous30Days,
            Group::Yesterday,
            Group::Previous7Days,
        ];
        groups.sort();
        assert_eq!(groups[0], Group::Pinned);
        assert_eq!(groups[1], Group::Today);
        assert_eq!(groups[4], Group::Previous30Days);
    }

    #[test]
    fn group_labels() {
        assert_eq!(Group::Pinned.label(), "Pinned");
        assert_eq!(Group::Previous7Days.label(), "Previous 7 Days");
        let october = Group::Month {
            year: 2026,
            month: 10,
        };
        assert_eq!(october.label(), "October 2026");
    }

    #[test]
    fn card_dates_shorten_with_age() {
        let now = now();
        let label = |date: &str| DateLabel::of(at(date), &now).format();
        assert_eq!(label("2026-11-16T11:02:00+01:00"), "11:02");
        assert_eq!(label("2026-11-15T10:24:00+01:00"), "Yesterday");
        assert_eq!(label("2026-11-13T11:24:00+01:00"), "Friday");
        assert_eq!(label("2026-11-10T11:24:00+01:00"), "Tuesday");
        assert_eq!(label("2026-11-09T11:24:00+01:00"), "Nov 9");
        assert_eq!(label("2026-07-19T11:24:00+02:00"), "Jul 19");
        assert_eq!(label("2025-12-24T11:24:00+01:00"), "Dec 24, 2025");
    }
}
