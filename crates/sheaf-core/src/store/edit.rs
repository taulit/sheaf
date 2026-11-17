//! Text edits on a note body. Pure functions, so the rules are easy to test.

use crate::error::{Error, Result};
use crate::scan::Task;

use super::TaskRef;

/// `body` with `text` added as new lines at the end.
pub(super) fn append(body: &str, text: &str, eol: &str) -> String {
    if text.is_empty() {
        return body.to_owned();
    }
    let mut out = body.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&terminated(text, eol));
    out
}

/// `body` with `text` added below the title line.
///
/// A title that is followed by a blank line keeps it, and the new text is set off from
/// what follows by a blank line as well. A title directly followed by text gets the new
/// lines directly below.
pub(super) fn prepend(body: &str, text: &str, eol: &str) -> String {
    if text.is_empty() {
        return body.to_owned();
    }
    let text = terminated(text, eol);
    if body.trim().is_empty() {
        return text;
    }

    let mut lines = body.split_inclusive('\n').peekable();
    let mut head = String::new();
    for line in lines.by_ref() {
        head.push_str(line);
        if !line.trim().is_empty() {
            break;
        }
    }
    // The underline of a setext heading belongs to the title.
    if let Some(line) = lines.next_if(|line| is_setext_underline(line)) {
        head.push_str(line);
    }

    let mut spaced = false;
    while let Some(line) = lines.next_if(|line| line.trim().is_empty()) {
        head.push_str(line);
        spaced = true;
    }
    let rest: String = lines.collect();
    if !head.ends_with('\n') {
        head.push_str(eol);
    }
    let gap = if spaced && !rest.is_empty() { eol } else { "" };
    format!("{head}{text}{gap}{rest}")
}

/// The file text with the task's marker set to `checked`, or `None` if it already is.
pub(super) fn set_task(body: &str, task: &Task, checked: bool) -> Result<Option<String>> {
    let marker = body.get(task.marker.clone()).unwrap_or_default();
    if !matches!(marker, "[ ]" | "[x]" | "[X]") {
        return Err(Error::Invalid(format!(
            "task {} has no checkbox",
            task.number
        )));
    }
    if task.checked == checked {
        return Ok(None);
    }
    let inner = task.marker.start + 1;
    let mut out = body.to_owned();
    out.replace_range(inner..inner + 1, if checked { "x" } else { " " });
    Ok(Some(out))
}

/// The task meant by a number or by a piece of its text.
pub(super) fn pick_task<'a>(tasks: &'a [Task], wanted: &TaskRef) -> Result<&'a Task> {
    let text = match wanted {
        TaskRef::Number(number) => {
            return tasks
                .iter()
                .find(|task| task.number == *number)
                .ok_or_else(|| Error::NotFound(format!("task {number}")));
        }
        TaskRef::Text(text) => text,
    };
    let needle = text.trim().to_lowercase();
    let exact: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.text.to_lowercase() == needle)
        .collect();
    let matches = if exact.is_empty() {
        tasks
            .iter()
            .filter(|task| task.text.to_lowercase().contains(&needle))
            .collect()
    } else {
        exact
    };
    match matches.as_slice() {
        [] => Err(Error::NotFound(format!("task \"{text}\""))),
        [task] => Ok(task),
        _ => Err(Error::Ambiguous {
            query: text.clone(),
            candidates: Vec::new(),
        }),
    }
}

/// `text` with the line endings of the file and a final one.
fn terminated(text: &str, eol: &str) -> String {
    let mut text = text.replace("\r\n", "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if eol == "\n" {
        text
    } else {
        text.replace('\n', eol)
    }
}

fn is_setext_underline(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty() && (line.chars().all(|c| c == '=') || line.chars().all(|c| c == '-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::scan;

    #[test]
    fn append_adds_lines_at_the_end() {
        let cases = [
            // (body, text, eol, result)
            ("", "new", "\n", "new\n"),
            ("Title", "new", "\n", "Title\nnew\n"),
            ("Title\n", "new", "\n", "Title\nnew\n"),
            ("Title\n\n", "new\n", "\n", "Title\n\nnew\n"),
            (
                "Title\r\nBody",
                "a\nb",
                "\r\n",
                "Title\r\nBody\r\na\r\nb\r\n",
            ),
            ("Title\n", "a\r\nb\r\n", "\n", "Title\na\nb\n"),
            ("Title\n", "", "\n", "Title\n"),
            ("Title", "", "\n", "Title"),
        ];
        for (body, text, eol, result) in cases {
            assert_eq!(append(body, text, eol), result, "{body:?} + {text:?}");
        }
    }

    #[test]
    fn prepend_adds_lines_below_the_title() {
        let cases = [
            ("", "new", "\n", "new\n"),
            ("\n\n", "new", "\n", "new\n"),
            ("Title", "new", "\n", "Title\nnew\n"),
            ("Title\n", "new", "\n", "Title\nnew\n"),
            ("Title\n- a\n- b\n", "- c", "\n", "Title\n- c\n- a\n- b\n"),
            ("# Title\n\nText\n", "new", "\n", "# Title\n\nnew\n\nText\n"),
            ("# Title\n\n", "new", "\n", "# Title\n\nnew\n"),
            (
                "\n# Title\n\n\nText",
                "new",
                "\n",
                "\n# Title\n\n\nnew\n\nText",
            ),
            (
                "Title\n=====\nText\n",
                "new",
                "\n",
                "Title\n=====\nnew\nText\n",
            ),
            (
                "Title\r\n\r\nText\r\n",
                "a\nb",
                "\r\n",
                "Title\r\n\r\na\r\nb\r\n\r\nText\r\n",
            ),
            ("Title\n", "", "\n", "Title\n"),
        ];
        for (body, text, eol, result) in cases {
            assert_eq!(prepend(body, text, eol), result, "{body:?} + {text:?}");
        }
    }

    #[test]
    fn set_task_changes_one_byte() {
        let body = "- [ ] a\n- [X] b\n";
        let tasks = scan(body).tasks;
        assert_eq!(
            set_task(body, &tasks[0], true).unwrap().unwrap(),
            "- [x] a\n- [X] b\n"
        );
        assert_eq!(
            set_task(body, &tasks[1], false).unwrap().unwrap(),
            "- [ ] a\n- [ ] b\n"
        );
        assert_eq!(set_task(body, &tasks[0], false).unwrap(), None);
        assert_eq!(set_task(body, &tasks[1], true).unwrap(), None);
    }

    #[test]
    fn picks_tasks_by_number_and_text() {
        let tasks = scan("- [ ] Milk\n- [ ] Oat milk\n- [x] Bread\n").tasks;
        let pick = |wanted: TaskRef| pick_task(&tasks, &wanted).map(|task| task.number);
        assert_eq!(pick(TaskRef::Number(2)).unwrap(), 2);
        assert_eq!(pick(TaskRef::Text("bread".into())).unwrap(), 3);
        assert_eq!(pick(TaskRef::Text("OAT".into())).unwrap(), 2);
        // An exact match wins over items that merely contain the text.
        assert_eq!(pick(TaskRef::Text("milk".into())).unwrap(), 1);
        assert!(matches!(
            pick(TaskRef::Text("i".into())),
            Err(Error::Ambiguous { .. })
        ));
        assert!(matches!(
            pick(TaskRef::Text("eggs".into())),
            Err(Error::NotFound(_))
        ));
        assert!(matches!(pick(TaskRef::Number(0)), Err(Error::NotFound(_))));
        assert!(matches!(pick(TaskRef::Number(4)), Err(Error::NotFound(_))));
    }
}
