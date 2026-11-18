//! Dates as the note view writes them: "16 November 2026, 10:12".

use jiff::ZonedDateTime;
use sheaf_core::i18n::tr;

use crate::app::gallery::dates::month_name;

/// The day, the month by name, the year and the time of `time`, in the language of
/// the app.
pub fn long_date_time(time: &ZonedDateTime) -> String {
    // TRANSLATORS: a date with its time, such as "16 November 2026, 10:12". Keep the
    // words in braces; German: "{day}. {month} {year} um {time}".
    tr("{day} {month} {year}, {time}")
        .replace("{day}", &time.day().to_string())
        .replace("{month}", &month_name(time.month()))
        .replace("{year}", &time.year().to_string())
        .replace("{time}", &time.strftime("%H:%M").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_day_month_year_and_time() {
        let time: jiff::Timestamp = "2026-11-16T09:12:00Z".parse().unwrap_or_default();
        let zoned = time.to_zoned_datetime(jiff::tz::TimeZone::fixed(jiff::tz::offset(1)));
        assert_eq!(long_date_time(&zoned), "16 November 2026, 10:12");
    }
}
