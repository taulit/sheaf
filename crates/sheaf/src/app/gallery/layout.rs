//! How the notes are arranged: in groups (Pinned, Today, …) and, in the grid, in rows
//! of as many cards as fit. The virtual list draws one [`Row`] per item; arrow keys
//! move through the same rows. Pure.

use std::ops::Range;

use gpui::Pixels;
use jiff::ZonedDateTime;
use sheaf_core::{NoteMeta, Sort};

use super::dates::Group;
use crate::app::theme::size;

/// A run of consecutive notes under one heading; search results have none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Section {
    pub group: Option<Group>,
    pub len: usize,
}

/// Puts pinned notes first and cuts the rest into groups by the date `sort` orders
/// by. `notes` come in the order of `sort`; the returned notes are in screen order.
pub fn group(
    notes: Vec<NoteMeta>,
    sort: Sort,
    now: &ZonedDateTime,
) -> (Vec<NoteMeta>, Vec<Section>) {
    let (pinned, rest): (Vec<_>, Vec<_>) = notes.into_iter().partition(|note| note.pinned);
    let mut sections = Vec::new();
    if !pinned.is_empty() {
        sections.push(Section {
            group: Some(Group::Pinned),
            len: pinned.len(),
        });
    }
    for note in &rest {
        let group = match sort {
            Sort::Modified => Group::of(note.modified, now),
            Sort::Created => Group::of(note.created, now),
            Sort::Title => Group::Notes,
        };
        match sections.last_mut() {
            Some(last) if last.group == Some(group) => last.len += 1,
            _ => sections.push(Section {
                group: Some(group),
                len: 1,
            }),
        }
    }
    let mut ordered = pinned;
    ordered.extend(rest);
    (ordered, sections)
}

/// How many cards of at least [`size::CARD_MIN_WIDTH`] fit into `width`, the width
/// inside the gallery's side padding: CSS `repeat(auto-fill, minmax(200px, 1fr))`.
pub fn columns(width: Pixels) -> usize {
    let gap = f32::from(size::GALLERY_GAP);
    let fits = (f32::from(width) + gap) / (f32::from(size::CARD_MIN_WIDTH) + gap);
    (fits.floor() as usize).max(1)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Header(Group),
    /// Indices into the notes, in screen order.
    Notes(Range<usize>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    rows: Vec<Row>,
    /// The row of each note.
    row_of: Vec<usize>,
}

impl Layout {
    pub fn new(sections: &[Section], columns: usize) -> Self {
        let columns = columns.max(1);
        let mut layout = Layout::default();
        let mut start = 0;
        for section in sections {
            if let Some(group) = section.group {
                layout.rows.push(Row::Header(group));
            }
            let end = start + section.len;
            for row_start in (start..end).step_by(columns) {
                let row_end = (row_start + columns).min(end);
                let row = layout.rows.len();
                layout.row_of.extend((row_start..row_end).map(|_| row));
                layout.rows.push(Row::Notes(row_start..row_end));
            }
            start = end;
        }
        layout
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn row_of(&self, note: usize) -> Option<usize> {
        self.row_of.get(note).copied()
    }

    /// The note in the next row of notes below (`down`) or above, in the same column
    /// or the last one of a shorter row; `None` at the edge.
    pub fn vertical(&self, note: usize, down: bool) -> Option<usize> {
        let row = self.row_of(note)?;
        let Row::Notes(here) = &self.rows[row] else {
            return None;
        };
        let column = note - here.start;
        let mut others: Box<dyn Iterator<Item = &Row>> = if down {
            Box::new(self.rows[row + 1..].iter())
        } else {
            Box::new(self.rows[..row].iter().rev())
        };
        others.find_map(|row| match row {
            Row::Notes(range) => Some((range.start + column).min(range.end - 1)),
            Row::Header(_) => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;
    use jiff::Timestamp;
    use jiff::tz::{TimeZone, offset};
    use sheaf_core::NoteId;

    fn now() -> ZonedDateTime {
        let time: Timestamp = "2026-11-16T13:24:00+01:00".parse().expect("a timestamp");
        time.to_zoned_datetime(TimeZone::fixed(offset(1)))
    }

    fn note(title: &str, modified: &str, pinned: bool) -> NoteMeta {
        let modified: Timestamp = modified.parse().expect("a timestamp");
        NoteMeta {
            id: NoteId::provisional(title),
            rel_path: format!("{title}.md"),
            folder: String::new(),
            title: title.to_owned(),
            created: "2020-01-01T00:00:00Z".parse().expect("a timestamp"),
            modified,
            pinned,
            tags: Vec::new(),
            tasks_open: 0,
            tasks_total: 0,
            has_image: false,
            has_table: false,
            has_attachment: false,
            conflict: false,
        }
    }

    fn notes() -> Vec<NoteMeta> {
        vec![
            note("a", "2026-11-16T12:00:00+01:00", false),
            note("b", "2026-11-16T11:00:00+01:00", true),
            note("c", "2026-11-15T11:00:00+01:00", false),
            note("d", "2026-11-12T11:00:00+01:00", false),
            note("e", "2026-11-11T11:00:00+01:00", false),
            note("f", "2026-10-01T11:00:00+02:00", true),
            note("g", "2026-09-01T11:00:00+02:00", false),
        ]
    }

    #[test]
    fn pinned_notes_come_first_then_date_groups() {
        let (ordered, sections) = group(notes(), Sort::Modified, &now());
        let titles: Vec<&str> = ordered.iter().map(|note| note.title.as_str()).collect();
        assert_eq!(titles, ["b", "f", "a", "c", "d", "e", "g"]);
        let groups: Vec<(Option<Group>, usize)> = sections
            .iter()
            .map(|section| (section.group, section.len))
            .collect();
        assert_eq!(
            groups,
            [
                (Some(Group::Pinned), 2),
                (Some(Group::Today), 1),
                (Some(Group::Yesterday), 1),
                (Some(Group::Previous7Days), 2),
                (
                    Some(Group::Month {
                        year: 2026,
                        month: 9
                    }),
                    1
                ),
            ]
        );
    }

    #[test]
    fn title_order_has_no_date_groups() {
        let (_, sections) = group(notes(), Sort::Title, &now());
        let groups: Vec<_> = sections.iter().map(|section| section.group).collect();
        assert_eq!(groups, [Some(Group::Pinned), Some(Group::Notes)]);
        let (_, none_pinned) = group(
            vec![note("x", "2026-11-16T12:00:00Z", false)],
            Sort::Title,
            &now(),
        );
        assert_eq!(none_pinned.len(), 1);
        assert_eq!(group(Vec::new(), Sort::Modified, &now()).1, []);
    }

    #[test]
    fn columns_fill_like_css_auto_fill() {
        assert_eq!(columns(px(100.)), 1);
        assert_eq!(columns(px(415.)), 1);
        assert_eq!(columns(px(416.)), 2);
        assert_eq!(columns(px(1052.)), 4);
        assert_eq!(columns(px(1112.)), 5);
    }

    #[test]
    fn rows_break_at_groups_and_at_the_column_count() {
        let sections = [
            Section {
                group: Some(Group::Pinned),
                len: 2,
            },
            Section {
                group: Some(Group::Today),
                len: 5,
            },
        ];
        let layout = Layout::new(&sections, 3);
        assert_eq!(
            layout.rows(),
            [
                Row::Header(Group::Pinned),
                Row::Notes(0..2),
                Row::Header(Group::Today),
                Row::Notes(2..5),
                Row::Notes(5..7),
            ]
        );
        assert_eq!(layout.row_of(4), Some(3));
        assert_eq!(layout.row_of(7), None);

        // Down keeps the column, or takes the last card of a shorter row.
        assert_eq!(layout.vertical(1, true), Some(3));
        assert_eq!(layout.vertical(4, true), Some(6));
        assert_eq!(layout.vertical(6, true), None);
        assert_eq!(layout.vertical(6, false), Some(3));
        assert_eq!(layout.vertical(2, false), Some(0));
        assert_eq!(layout.vertical(0, false), None);

        let results = Layout::new(
            &[Section {
                group: None,
                len: 3,
            }],
            1,
        );
        assert_eq!(
            results.rows(),
            [Row::Notes(0..1), Row::Notes(1..2), Row::Notes(2..3)]
        );
    }
}
