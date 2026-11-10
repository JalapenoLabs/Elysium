// Copyright © 2026 Jalapeno Labs

//! The Transcription doc's Markdown: reading its entries, and teaching it. Pure; the page it is read
//! from and written to is `super`'s.
//!
//! # The shape it reads and writes
//!
//! The doc is a page like any other, which people edit freely, so Elysium reads and writes only two
//! parts of it and leaves every other line as it found it:
//!
//! - **The entries**, a table whose header starts `| Heard | Is |`: one row per misheard word or
//!   phrase, `| Heard | Is | Sources | Speakers | Meetings |`. Sources and speakers are lists
//!   separated by commas; meetings are links to each meeting's page, newest first.
//! - **What it taught itself**, the bullets under the heading `## What it taught itself`, newest
//!   first: one for each entry Elysium added or heard again, with when, where, and why.
//!
//! Reading is lenient: a row that does not read as an entry is passed over, and a doc whose table or
//! heading a person removed gets them back, at its end, the next time it is taught. Teaching edits
//! lines in place, so a person's own rows, notes, and order stay as they wrote them.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::ops::Range;

use chrono::{Datelike, NaiveDate};
use uuid::Uuid;

/// The heading the self-taught bullets sit under.
const TAUGHT_HEADING: &str = "## What it taught itself";

/// What the doc says above its table, when teaching has to put the table back.
const ENTRIES_HEADING: &str = "## Entries";

/// The table's header and its divider, as the migration seeds them.
const TABLE_HEADER: &str = "| Heard | Is | Sources | Speakers | Meetings |";
const TABLE_DIVIDER: &str = "| --- | --- | --- | --- | --- |";

/// The most meetings one entry links, newest first: enough to show it is heard often and to open a
/// few, few enough that a name heard in every meeting keeps a readable row.
const MEETINGS_SHOWN: usize = 10;

/// The most bullets the self-taught list keeps, newest first. Every change is also a version of the
/// page, so what falls off the end is in its history.
const TAUGHT_SHOWN: usize = 100;

/// Where a meeting's page is, which every link in the Meetings column names.
pub const MEETING_PAGE_PATH: &str = "/settings/meetings/";

/// One row of the entries table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What transcription heard, such as `Sheriff`.
    pub heard: String,
    /// What it is, such as `Sharath`.
    pub written: String,
    /// The transcripts that heard it so: a source, such as `Teams`, or `Whisper`.
    pub sources: Vec<String>,
    /// Who was speaking when it was heard so.
    pub speakers: Vec<String>,
    /// The meetings it was heard in, newest first.
    pub meetings: Vec<MeetingLink>,
}

/// A meeting an entry was heard in, as its Meetings cell links it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingLink {
    pub id: Uuid,
    pub title: String,
}

/// One thing the doc learns: what was heard, what it is, where, and why it is believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesson {
    pub heard: String,
    pub written: String,
    /// The transcript that heard it so, such as `Teams` or `Whisper`.
    pub source: Option<String>,
    /// Who was speaking.
    pub speaker: Option<String>,
    pub meeting: MeetingLink,
    /// A sentence saying who taught it and why, such as "Ada Lovelace fixed it on the meeting's
    /// page."
    pub why: String,
}

/// Every entry of `body`'s table, in its order. A row that does not read as one is passed over.
pub fn entries(body: &str) -> Vec<Entry> {
    let lines: Vec<&str> = body.lines().collect();
    let Some(rows) = table_rows(&lines) else {
        return Vec::new();
    };
    lines[rows]
        .iter()
        .filter_map(|line| parse_row(line))
        .collect()
}

/// `body` taught `lessons`, on `day`: each one's entry added, or strengthened with its source,
/// speaker, and meeting, and a bullet saying so at the top of what it taught itself. A lesson that
/// adds nothing to its entry changes nothing, and `body` comes back as it was when none does.
pub fn teach(body: &str, lessons: &[Lesson], day: NaiveDate) -> String {
    let mut lines: Vec<String> = body.lines().map(ToOwned::to_owned).collect();
    let mut taught: Vec<String> = Vec::new();
    for lesson in lessons {
        let heard_key = phrase_key(&lesson.heard);
        let written_key = phrase_key(&lesson.written);
        if heard_key.is_empty() || written_key.is_empty() || heard_key == written_key {
            continue;
        }
        let rows = ensure_table(&mut lines);
        let existing = rows.clone().find_map(|index| {
            parse_row(&lines[index])
                .filter(|entry| {
                    phrase_key(&entry.heard) == heard_key
                        && phrase_key(&entry.written) == written_key
                })
                .map(|entry| (index, entry))
        });
        if let Some((index, mut entry)) = existing {
            if strengthen(&mut entry, lesson) {
                lines[index] = render_row(&entry);
                taught.push(bullet(lesson, day, "heard again"));
            }
            continue;
        }
        let mut entry = Entry {
            heard: lesson.heard.trim().to_owned(),
            written: lesson.written.trim().to_owned(),
            sources: Vec::new(),
            speakers: Vec::new(),
            meetings: Vec::new(),
        };
        strengthen(&mut entry, lesson);
        let at = sorted_place(&lines, rows, &written_key);
        lines.insert(at, render_row(&entry));
        taught.push(bullet(lesson, day, "added"));
    }
    if taught.is_empty() {
        return body.to_owned();
    }
    record_taught(&mut lines, taught);

    let mut written = lines.join("\n");
    if body.ends_with('\n') {
        written.push('\n');
    }
    written
}

/// A phrase as entries are matched: its words lowercase, without the punctuation around them, so
/// "Sheriff," and "sheriff" are one entry.
pub fn phrase_key(phrase: &str) -> String {
    phrase
        .split_whitespace()
        .map(|word| {
            word.trim_matches(|character: char| !character.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The lines of the entries table's rows: the first table in `lines` whose header's first two cells
/// are Heard and Is.
fn table_rows<Line: AsRef<str>>(lines: &[Line]) -> Option<Range<usize>> {
    let header = lines.iter().position(|line| {
        let cells = cells_of(line.as_ref());
        cells.len() >= 2
            && cells[0].eq_ignore_ascii_case("heard")
            && cells[1].eq_ignore_ascii_case("is")
    })?;
    // The row after the header divides it from the rows, as GitHub's tables have it; a table a
    // person left without one starts its rows straight after the header.
    let divided = lines.get(header + 1).is_some_and(|line| {
        let cells = cells_of(line.as_ref());
        !cells.is_empty()
            && cells.iter().all(|cell| {
                !cell.is_empty() && cell.chars().all(|character| matches!(character, '-' | ':'))
            })
    });
    let first_row = (header + if divided { 2 } else { 1 }).min(lines.len());
    let end = lines[first_row..]
        .iter()
        .position(|line| !line.as_ref().trim_start().starts_with('|'))
        .map_or(lines.len(), |offset| first_row + offset);
    Some(first_row..end)
}

/// The rows of `lines`' entries table, putting the table back at their end when a person removed
/// it.
fn ensure_table(lines: &mut Vec<String>) -> Range<usize> {
    if let Some(rows) = table_rows(lines) {
        return rows;
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push(ENTRIES_HEADING.to_owned());
    lines.push(String::new());
    lines.push(TABLE_HEADER.to_owned());
    lines.push(TABLE_DIVIDER.to_owned());
    lines.len()..lines.len()
}

/// Where a new entry for `written_key` goes among the table's rows: before the first row that
/// reads after it in alphabetical order, so a table kept in order stays so, or after the last.
fn sorted_place(lines: &[String], rows: Range<usize>, written_key: &str) -> usize {
    rows.clone()
        .find(|index| {
            parse_row(&lines[*index])
                .is_some_and(|entry| phrase_key(&entry.written).as_str() > written_key)
        })
        .unwrap_or(rows.end)
}

/// Adds what `entry` lacks of `lesson`: its source, its speaker, and its meeting, first. Answers
/// whether it added anything.
fn strengthen(entry: &mut Entry, lesson: &Lesson) -> bool {
    let mut changed = false;
    for (list, value) in [
        (&mut entry.sources, &lesson.source),
        (&mut entry.speakers, &lesson.speaker),
    ] {
        let Some(value) = value
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if !list.iter().any(|known| known.eq_ignore_ascii_case(value)) {
            list.push(value.to_owned());
            changed = true;
        }
    }
    if !entry
        .meetings
        .iter()
        .any(|meeting| meeting.id == lesson.meeting.id)
    {
        entry.meetings.insert(0, lesson.meeting.clone());
        entry.meetings.truncate(MEETINGS_SHOWN);
        changed = true;
    }
    changed
}

/// Puts `taught`, newest first, at the top of the doc's self-taught bullets, putting the heading
/// back at the doc's end when a person removed it, and keeps the newest [`TAUGHT_SHOWN`].
fn record_taught(lines: &mut Vec<String>, taught: Vec<String>) {
    let found = lines
        .iter()
        .position(|line| line.trim().eq_ignore_ascii_case(TAUGHT_HEADING));
    let heading = found.unwrap_or_else(|| {
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        lines.push(String::new());
        lines.push(TAUGHT_HEADING.to_owned());
        lines.len() - 1
    });
    let section_end = lines[heading + 1..]
        .iter()
        .position(|line| line.starts_with("## "))
        .map_or(lines.len(), |offset| heading + 1 + offset);
    let first_bullet = lines[heading + 1..section_end]
        .iter()
        .position(|line| line.starts_with("- "))
        .map(|offset| heading + 1 + offset);
    let at = first_bullet.unwrap_or_else(|| {
        // The heading's own paragraph stays above the first bullet, a blank line between.
        let mut at = section_end;
        while at > heading + 1 && lines[at - 1].trim().is_empty() {
            at -= 1;
        }
        lines.insert(at, String::new());
        at + 1
    });
    let count = taught.len();
    lines.splice(at..at, taught);

    let mut kept = 0;
    let mut index = at;
    while index < lines.len() && !lines[index].starts_with("## ") {
        if lines[index].starts_with("- ") {
            kept += 1;
            if kept > TAUGHT_SHOWN.max(count) {
                lines.remove(index);
                continue;
            }
        }
        index += 1;
    }
}

/// The bullet saying `lesson` was taught on `day`, `how` saying whether it was added or heard
/// again.
fn bullet(lesson: &Lesson, day: NaiveDate, how: &str) -> String {
    let mut text = format!(
        "- {}, {how}: \"{}\" is {}",
        long_date(day),
        inline(&lesson.heard),
        inline(&lesson.written)
    );
    if let Some(source) = lesson.source.as_deref().filter(|source| !source.is_empty()) {
        let _ = write!(text, ", heard by {}", inline(source));
    }
    if let Some(speaker) = lesson
        .speaker
        .as_deref()
        .filter(|speaker| !speaker.is_empty())
    {
        let _ = write!(text, " when {} spoke", inline(speaker));
    }
    let _ = write!(text, ", in {}.", link(&lesson.meeting));
    let why = inline(&lesson.why);
    if !why.is_empty() {
        text.push(' ');
        text.push_str(&why);
    }
    text
}

/// `day` as the web app writes dates, "November 10th 2026", since people read the page as it is.
fn long_date(day: NaiveDate) -> String {
    let number = day.day();
    let suffix = if (11..=13).contains(&number) {
        "th"
    } else {
        match number % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    };
    format!("{} {number}{suffix} {}", day.format("%B"), day.year())
}

/// The cells of a table row, unescaped and trimmed; none for a line that is not a row.
fn cells_of(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return Vec::new();
    }
    let inner = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut characters = inner.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' if characters.peek() == Some(&'|') => {
                cell.push('|');
                characters.next();
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_owned()),
            other => cell.push(other),
        }
    }
    cells.push(cell.trim().to_owned());
    cells
}

/// `line` as an entry, when it is a row with a Heard and an Is.
fn parse_row(line: &str) -> Option<Entry> {
    let cells = cells_of(line);
    let heard = cells.first().filter(|heard| !heard.is_empty())?;
    let written = cells.get(1).filter(|written| !written.is_empty())?;
    // The divider under the header reads as a row of dashes.
    if heard
        .chars()
        .all(|character| matches!(character, '-' | ':'))
    {
        return None;
    }
    let list = |index: usize| -> Vec<String> {
        cells
            .get(index)
            .map(|cell| {
                cell.split(',')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(Entry {
        heard: heard.clone(),
        written: written.clone(),
        sources: list(2),
        speakers: list(3),
        meetings: cells.get(4).map(|cell| links_in(cell)).unwrap_or_default(),
    })
}

/// Every link to a meeting's page in `cell`, in order, each meeting once.
fn links_in(cell: &str) -> Vec<MeetingLink> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();
    let mut rest = cell;
    while let Some(open) = rest.find('[') {
        let after_open = &rest[open + 1..];
        let Some(close) = find_unescaped(after_open, ']') else {
            break;
        };
        let title = after_open[..close].replace("\\]", "]").replace("\\[", "[");
        let after_title = &after_open[close + 1..];
        let Some(target) = after_title.strip_prefix('(') else {
            rest = after_title;
            continue;
        };
        let Some(end) = target.find(')') else {
            break;
        };
        let id = target[..end]
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .and_then(|last| last.parse::<Uuid>().ok());
        if let Some(id) = id
            && seen.insert(id)
        {
            links.push(MeetingLink { id, title });
        }
        rest = &target[end + 1..];
    }
    links
}

/// Where `wanted` first appears in `text` without a backslash before it.
fn find_unescaped(text: &str, wanted: char) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if character == wanted && !escaped {
            return Some(index);
        }
        escaped = character == '\\' && !escaped;
    }
    None
}

/// `entry` as a row of the table.
fn render_row(entry: &Entry) -> String {
    let list = |items: &[String]| {
        items
            .iter()
            .map(|item| cell(item))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let meetings = entry
        .meetings
        .iter()
        .map(link)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "| {} | {} | {} | {} | {} |",
        cell(&entry.heard),
        cell(&entry.written),
        list(&entry.sources),
        list(&entry.speakers),
        meetings,
    )
}

/// A link to `meeting`'s page, its title escaped so it stays one link in one cell.
fn link(meeting: &MeetingLink) -> String {
    let title = inline(&meeting.title)
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('|', "\\|");
    let title = if title.is_empty() {
        "A meeting".to_owned()
    } else {
        title
    };
    format!("[{title}]({MEETING_PAGE_PATH}{})", meeting.id)
}

/// `text` on one line, its spacing collapsed.
fn inline(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` as a table cell: on one line, a pipe escaped so it never ends the cell.
fn cell(text: &str) -> String {
    inline(text).replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The doc as its migration seeds it.
    const SEEDED: &str = "How speech is misheard.\n\n## Entries\n\nWhat was heard.\n\n\
        | Heard | Is | Sources | Speakers | Meetings |\n| --- | --- | --- | --- | --- |\n\n\
        ## What it taught itself\n\nNewest first, for a person to review.";

    fn meeting(title: &str) -> MeetingLink {
        MeetingLink {
            id: Uuid::now_v7(),
            title: title.to_owned(),
        }
    }

    fn lesson(heard: &str, written: &str, source: &str, speaker: &str, at: &MeetingLink) -> Lesson {
        Lesson {
            heard: heard.to_owned(),
            written: written.to_owned(),
            source: Some(source.to_owned()),
            speaker: Some(speaker.to_owned()),
            meeting: at.clone(),
            why: "Ada Lovelace fixed it on the meeting's page.".to_owned(),
        }
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 10).expect("a date")
    }

    #[test]
    fn a_lesson_adds_its_entry_and_says_so_first_among_what_it_taught_itself() {
        let sync = meeting("Weekly sync | planning");
        let taught = teach(
            SEEDED,
            &[lesson("Sheriff", "Sharath", "Teams", "Ada Lovelace", &sync)],
            day(),
        );

        assert_eq!(
            entries(&taught),
            vec![Entry {
                heard: "Sheriff".to_owned(),
                written: "Sharath".to_owned(),
                sources: vec!["Teams".to_owned()],
                speakers: vec!["Ada Lovelace".to_owned()],
                meetings: vec![sync.clone()],
            }]
        );
        assert!(
            taught.contains(&format!(
                "- November 10th 2026, added: \"Sheriff\" is Sharath, heard by Teams when Ada \
                 Lovelace spoke, in [Weekly sync \\| planning](/settings/meetings/{}). Ada \
                 Lovelace fixed it on the meeting's page.",
                sync.id
            )),
            "{taught}"
        );
        assert!(
            taught.starts_with("How speech is misheard.\n\n## Entries\n\nWhat was heard.\n"),
            "everything around the table stays as it was"
        );
        let paragraph = taught
            .find("Newest first")
            .expect("the heading's paragraph");
        let first_bullet = taught.find("- November").expect("a bullet");
        assert!(paragraph < first_bullet);
    }

    #[test]
    fn hearing_an_entry_again_strengthens_it_and_the_newest_comes_first() {
        let first = meeting("Standup");
        let second = meeting("Launch review");
        let once = teach(
            SEEDED,
            &[lesson("Sheriff", "Sharath", "Teams", "Ada", &first)],
            day(),
        );
        // The same mishearing, in another case, by another source and speaker.
        let twice = teach(
            &once,
            &[lesson("sheriff,", "Sharath", "Whisper", "Grace", &second)],
            day(),
        );

        let read = entries(&twice);
        assert_eq!(read.len(), 1, "{twice}");
        assert_eq!(read[0].sources, vec!["Teams", "Whisper"]);
        assert_eq!(read[0].speakers, vec!["Ada", "Grace"]);
        assert_eq!(read[0].meetings, vec![second.clone(), first]);
        let again = twice.find("heard again").expect("the second bullet");
        let added = twice.find("added").expect("the first bullet");
        assert!(again < added, "newest first");

        // Taught what it already holds, it changes nothing, and says nothing.
        let thrice = teach(
            &twice,
            &[lesson("Sheriff", "Sharath", "Teams", "Ada", &second)],
            day(),
        );
        assert_eq!(thrice, twice);
    }

    #[test]
    fn a_new_entry_keeps_the_table_in_order_and_every_line_a_person_wrote() {
        let body = "Notes of ours.\n\n| Heard | Is | Sources | Speakers | Meetings |\n\
            |---|---|---|---|---|\n\
            | Ely see um | Elysium | Fireflies | | |\n\
            | Moore's lab | Mooreslab | | | |\n\
            | not a row\n\nMore notes.";
        let taught = teach(
            body,
            &[lesson(
                "Fire flies",
                "Fireflies",
                "Whisper",
                "Ada",
                &meeting("Kickoff"),
            )],
            day(),
        );

        let written: Vec<String> = entries(&taught)
            .into_iter()
            .map(|entry| entry.written)
            .collect();
        assert_eq!(written, vec!["Elysium", "Fireflies", "Mooreslab"]);
        assert!(taught.starts_with("Notes of ours.\n"));
        assert!(taught.contains("| not a row\n\nMore notes.\n"));
        let heading = taught
            .find(TAUGHT_HEADING)
            .expect("the heading a person removed comes back");
        assert!(taught[heading..].contains("\n\n- November 10th 2026, added: \"Fire flies\""));
    }

    #[test]
    fn a_doc_without_its_table_or_heading_gets_them_back_at_its_end() {
        let taught = teach(
            "Only our notes.\n",
            &[lesson(
                "Sheriff",
                "Sharath",
                "Teams",
                "Ada",
                &meeting("Standup"),
            )],
            day(),
        );
        assert!(taught.starts_with("Only our notes.\n\n## Entries\n\n| Heard | Is |"));
        assert_eq!(entries(&taught).len(), 1);
        let heading = taught.find(TAUGHT_HEADING).expect("the heading");
        assert!(taught[heading..].contains("\n- November 10th 2026, added"));
        assert!(taught.ends_with('\n'), "a trailing newline stays");
    }

    #[test]
    fn a_lesson_that_teaches_nothing_changes_nothing() {
        let at = meeting("Standup");
        assert_eq!(
            teach(
                SEEDED,
                &[
                    lesson("Sharath", "sharath.", "Teams", "Ada", &at),
                    lesson("  ", "Sharath", "Teams", "Ada", &at),
                ],
                day(),
            ),
            SEEDED
        );
    }

    #[test]
    fn what_it_taught_itself_keeps_its_newest() {
        let mut body = SEEDED.to_owned();
        for number in 0..(TAUGHT_SHOWN + 5) {
            body = teach(
                &body,
                &[lesson(
                    &format!("heard {number}"),
                    &format!("written {number}"),
                    "Teams",
                    "Ada",
                    &meeting("Standup"),
                )],
                day(),
            );
        }
        let bullets: Vec<&str> = body.lines().filter(|line| line.starts_with("- ")).collect();
        assert_eq!(bullets.len(), TAUGHT_SHOWN);
        assert!(bullets[0].contains("heard 104"), "{}", bullets[0]);
        assert!(bullets[TAUGHT_SHOWN - 1].contains("heard 5"));
        assert_eq!(
            entries(&body).len(),
            TAUGHT_SHOWN + 5,
            "entries are never dropped"
        );
    }

    #[test]
    fn an_entry_links_its_newest_meetings() {
        let mut body = SEEDED.to_owned();
        let mut newest = Vec::new();
        for number in 0..(MEETINGS_SHOWN + 2) {
            let at = meeting(&format!("Meeting [{number}]"));
            newest.insert(0, at.clone());
            body = teach(
                &body,
                &[lesson("Sheriff", "Sharath", "Teams", "Ada", &at)],
                day(),
            );
        }
        newest.truncate(MEETINGS_SHOWN);
        assert_eq!(
            entries(&body)[0].meetings,
            newest,
            "titles keep their brackets"
        );
    }

    #[test]
    fn dates_read_as_the_web_app_writes_them() {
        let on = |day: u32| long_date(NaiveDate::from_ymd_opt(2026, 1, day).expect("a date"));
        assert_eq!(on(1), "January 1st 2026");
        assert_eq!(on(2), "January 2nd 2026");
        assert_eq!(on(3), "January 3rd 2026");
        assert_eq!(on(4), "January 4th 2026");
        assert_eq!(on(11), "January 11th 2026");
        assert_eq!(on(12), "January 12th 2026");
        assert_eq!(on(13), "January 13th 2026");
        assert_eq!(on(21), "January 21st 2026");
        assert_eq!(on(22), "January 22nd 2026");
        assert_eq!(on(23), "January 23rd 2026");
        assert_eq!(on(31), "January 31st 2026");
    }
}
