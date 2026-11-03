// Copyright © 2026 Jalapeno Labs

//! Confluence's storage format, rendered as plain text, with the people it mentions.
//!
//! A page or comment body arrives as storage format: XHTML with Confluence's own `ac:` and
//! `ri:` elements. The timeline keeps plain text, for search and for summaries, so this walks
//! the markup once and keeps what a reader would read:
//!
//! - Blocks (`p`, headings, `div`, `blockquote`, table rows, tasks, macros) start new lines,
//!   and a list item starts with `- `. `br` is a line break.
//! - Table cells in a row are separated by ` | `.
//! - Text keeps its words; runs of whitespace collapse to one space, and at most one blank line
//!   is kept between blocks. Preformatted text (`pre`, a macro's `ac:plain-text-body`) keeps its
//!   lines as written.
//! - A mention (`ri:user`) reads as `@user`, and its account id is collected in
//!   [`Rendered::mentions`]. A link to a page (`ri:page`) reads as that page's title, and a
//!   link's own text (`ac:plain-text-link-body`) as written.
//! - Macro parameters (`ac:parameter`), placeholders, images, and emoticons carry no text a
//!   reader reads, and are dropped.
//! - Character references are decoded: numeric ones, and the named ones storage format uses.
//!   An unknown name is kept as written.
//!
//! This is a reader's rendering, not a parser: markup that is not well formed still renders
//! whatever text it holds.

/// A body as plain text, and the Atlassian account ids it mentions, each once, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub mentions: Vec<String>,
}

/// Elements whose content is not read: macro parameters and placeholder hints.
const SILENT_ELEMENTS: &[&str] = &["ac:parameter", "ac:placeholder"];

/// Elements that start and end a block of their own.
const BLOCK_ELEMENTS: &[&str] = &[
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "table",
    "tr",
    "ul",
    "ol",
    "li",
    "pre",
    "hr",
    "ac:task",
    "ac:task-list",
    "ac:structured-macro",
    "ac:layout-section",
    "ac:layout-cell",
];

/// Elements whose text keeps its whitespace and lines.
const PREFORMATTED_ELEMENTS: &[&str] = &["pre", "ac:plain-text-body"];

/// Renders a storage-format body as plain text. See the module docs for what is kept.
pub fn render(storage: &str) -> Rendered {
    let mut writer = Writer::default();
    let mut silent_depth = 0usize;
    let mut preformatted_depth = 0usize;
    let mut cells_in_row = 0usize;
    let mut rest = storage;

    while let Some(start) = rest.find('<') {
        let text = &rest[..start];
        if silent_depth == 0 {
            writer.text(&decode(text), preformatted_depth > 0);
        }
        rest = &rest[start..];

        if let Some(after) = rest.strip_prefix("<![CDATA[") {
            let end = after.find("]]>").unwrap_or(after.len());
            if silent_depth == 0 {
                writer.text(&after[..end], preformatted_depth > 0);
            }
            rest = after.get(end + 3..).unwrap_or("");
            continue;
        }
        if let Some(after) = rest.strip_prefix("<!--") {
            let end = after.find("-->").map_or(after.len(), |end| end + 3);
            rest = &after[end..];
            continue;
        }

        let Some(tag_end) = tag_end(rest) else {
            // An unclosed tag at the end: nothing after it is text a reader would see.
            rest = "";
            break;
        };
        let tag = Tag::parse(&rest[1..tag_end]);
        rest = &rest[tag_end + 1..];

        let name = tag.name.as_str();
        if SILENT_ELEMENTS.contains(&name) {
            if tag.is_closing {
                silent_depth = silent_depth.saturating_sub(1);
            } else if !tag.is_self_closing {
                silent_depth += 1;
            }
            continue;
        }
        if silent_depth > 0 {
            continue;
        }

        if PREFORMATTED_ELEMENTS.contains(&name) {
            writer.start_line();
            if tag.is_closing {
                preformatted_depth = preformatted_depth.saturating_sub(1);
            } else if !tag.is_self_closing {
                preformatted_depth += 1;
            }
            continue;
        }

        match name {
            "br" => writer.line_break(),
            "ri:user" if !tag.is_closing => {
                if let Some(account_id) = tag.attribute("ri:account-id") {
                    writer.mention(account_id);
                }
                writer.word("@user");
            }
            "ri:page" if !tag.is_closing => {
                if let Some(title) = tag.attribute("ri:content-title") {
                    writer.word(&decode(title));
                }
            }
            "tr" => {
                cells_in_row = 0;
                writer.block();
            }
            "td" | "th" if !tag.is_closing => {
                if cells_in_row > 0 {
                    writer.cell_separator();
                }
                cells_in_row += 1;
            }
            "li" if !tag.is_closing => {
                writer.block();
                writer.word("-");
            }
            other if BLOCK_ELEMENTS.contains(&other) => writer.block(),
            _ => {}
        }
    }
    if silent_depth == 0 {
        writer.text(&decode(rest), preformatted_depth > 0);
    }
    writer.finish()
}

/// Where the tag starting at `markup[0] == '<'` ends, skipping a `>` inside a quoted attribute.
fn tag_end(markup: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (index, character) in markup.char_indices().skip(1) {
        match quote {
            // Inside a quoted value only its closing quote means anything.
            Some(open) => {
                if character == open {
                    quote = None;
                }
            }
            None if matches!(character, '"' | '\'') => quote = Some(character),
            None if character == '>' => return Some(index),
            None => {}
        }
    }
    None
}

/// One tag's name, its attributes, and whether it closes or closes itself.
#[derive(Debug)]
struct Tag {
    /// Lowercased, so `<P>` reads as `p`.
    name: String,
    is_closing: bool,
    is_self_closing: bool,
    attributes: Vec<(String, String)>,
}

impl Tag {
    /// Parses what lies between `<` and `>`.
    fn parse(inside: &str) -> Self {
        let inside = inside.trim();
        let (is_closing, inside) = inside
            .strip_prefix('/')
            .map_or((false, inside), |rest| (true, rest.trim_start()));
        let (is_self_closing, inside) = inside
            .strip_suffix('/')
            .map_or((false, inside), |rest| (true, rest.trim_end()));
        let name_end = inside
            .find(|character: char| character.is_whitespace())
            .unwrap_or(inside.len());
        Self {
            name: inside[..name_end].to_ascii_lowercase(),
            is_closing,
            is_self_closing,
            attributes: attributes(&inside[name_end..]),
        }
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _value)| key.eq_ignore_ascii_case(name))
            .map(|(_key, value)| value.as_str())
    }
}

/// `name="value"` pairs, in single or double quotes. A bare name has an empty value.
fn attributes(mut rest: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return found;
        }
        let name_end = rest
            .find(|character: char| character == '=' || character.is_whitespace())
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_owned();
        rest = rest[name_end..].trim_start();
        let Some(after_equals) = rest.strip_prefix('=') else {
            found.push((name, String::new()));
            continue;
        };
        let after_equals = after_equals.trim_start();
        let mut characters = after_equals.chars();
        let value = if let Some(quote @ ('"' | '\'')) = characters.next() {
            let body = &after_equals[1..];
            let end = body.find(quote).unwrap_or(body.len());
            rest = body.get(end + 1..).unwrap_or("");
            body[..end].to_owned()
        } else {
            let end = after_equals
                .find(char::is_whitespace)
                .unwrap_or(after_equals.len());
            rest = &after_equals[end..];
            after_equals[..end].to_owned()
        };
        found.push((name, value));
    }
}

/// The named character references storage format writes, by name.
const NAMED_REFERENCES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", " "),
    ("rsquo", "\u{2019}"),
    ("lsquo", "\u{2018}"),
    ("rdquo", "\u{201d}"),
    ("ldquo", "\u{201c}"),
    ("hellip", "\u{2026}"),
    ("ndash", "\u{2013}"),
    ("mdash", "\u{2014}"),
    ("bull", "\u{2022}"),
    ("middot", "\u{b7}"),
    ("copy", "\u{a9}"),
    ("reg", "\u{ae}"),
    ("trade", "\u{2122}"),
    ("times", "\u{d7}"),
    ("rarr", "\u{2192}"),
    ("larr", "\u{2190}"),
];

/// `text` with its character references decoded. An unknown one is kept as written.
fn decode(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        decoded.push_str(&rest[..start]);
        rest = &rest[start..];
        let reference = rest[1..]
            .find(';')
            .filter(|end| *end <= 10)
            .map(|end| &rest[1..=end]);
        let Some(reference) = reference else {
            decoded.push('&');
            rest = &rest[1..];
            continue;
        };
        let replacement = match reference.strip_prefix('#') {
            Some(number) => number
                .strip_prefix(['x', 'X'])
                .map_or_else(
                    || number.parse::<u32>().ok(),
                    |hex| u32::from_str_radix(hex, 16).ok(),
                )
                .and_then(char::from_u32)
                .map(String::from),
            None => NAMED_REFERENCES
                .iter()
                .find(|(name, _value)| *name == reference)
                .map(|(_name, value)| (*value).to_owned()),
        };
        if let Some(replacement) = replacement {
            decoded.push_str(&replacement);
            rest = &rest[reference.len() + 2..];
        } else {
            decoded.push('&');
            rest = &rest[1..];
        }
    }
    decoded.push_str(rest);
    decoded
}

/// Builds the plain text: words joined by single spaces, blocks on lines of their own, and at
/// most one blank line in a row.
#[derive(Debug, Default)]
struct Writer {
    text: String,
    mentions: Vec<String>,
    /// A space is owed before the next word on this line.
    owes_space: bool,
}

impl Writer {
    fn text(&mut self, text: &str, is_preformatted: bool) {
        if is_preformatted {
            if text.is_empty() {
                return;
            }
            if self.owes_space {
                self.text.push(' ');
                self.owes_space = false;
            }
            self.text.push_str(text);
            return;
        }
        if text.starts_with(char::is_whitespace) {
            self.owes_space = !self.at_line_start();
        }
        for (index, word) in text.split_whitespace().enumerate() {
            if index > 0 {
                self.owes_space = true;
            }
            self.word(word);
        }
        if text.ends_with(char::is_whitespace) && !self.at_line_start() {
            self.owes_space = true;
        }
    }

    /// Writes one word, after a space when one is owed. The text after it decides whether the
    /// next is owed one, except after a list item's dash, which always is.
    fn word(&mut self, word: &str) {
        if self.owes_space && !self.at_line_start() {
            self.text.push(' ');
        }
        self.text.push_str(word);
        self.owes_space = word == "-";
    }

    /// Separates one table cell from the next on its row.
    fn cell_separator(&mut self) {
        self.trim_line_end();
        self.text.push_str(" |");
        self.owes_space = true;
    }

    fn mention(&mut self, account_id: &str) {
        if !self.mentions.iter().any(|known| known == account_id) {
            self.mentions.push(account_id.to_owned());
        }
    }

    fn line_break(&mut self) {
        self.trim_line_end();
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.owes_space = false;
    }

    /// Moves to the start of a line, unless already there.
    fn start_line(&mut self) {
        self.trim_line_end();
        if !self.at_line_start() {
            self.text.push('\n');
        }
        self.owes_space = false;
    }

    /// Starts a new block: a fresh line, with a blank line before it when text came before.
    fn block(&mut self) {
        self.trim_line_end();
        if self.text.is_empty() || self.text.ends_with("\n\n") {
            self.owes_space = false;
            return;
        }
        if !self.text.ends_with('\n') {
            self.text.push('\n');
        }
        self.text.push('\n');
        self.owes_space = false;
    }

    fn at_line_start(&self) -> bool {
        self.text.is_empty() || self.text.ends_with('\n')
    }

    fn trim_line_end(&mut self) {
        let trimmed = self.text.trim_end_matches([' ', '\t']).len();
        self.text.truncate(trimmed);
    }

    fn finish(self) -> Rendered {
        Rendered {
            text: self.text.trim().to_owned(),
            mentions: self.mentions,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_and_headings_become_blocks_separated_by_one_blank_line() {
        let rendered = render(
            "<h1>Launch plan</h1><p>We ship   on\n Friday.</p><p></p><p>Questions welcome.</p>",
        );
        assert_eq!(
            rendered.text,
            "Launch plan\n\nWe ship on Friday.\n\nQuestions welcome."
        );
    }

    #[test]
    fn list_items_start_with_a_dash_and_line_breaks_are_kept() {
        let rendered = render("<ul><li>Design</li><li>Build<br/>then test</li></ul>");
        assert_eq!(rendered.text, "- Design\n\n- Build\nthen test");
    }

    #[test]
    fn a_mention_reads_as_at_user_and_its_account_is_collected_once() {
        let storage = "<p>Hey <ac:link><ri:user ri:account-id=\"5b10a2844c20165700ede21g\" /></ac:link>, \
                       can you review? cc <ac:link><ri:user ri:account-id=\"5b10a2844c20165700ede21g\"/></ac:link></p>";
        let rendered = render(storage);
        assert_eq!(rendered.text, "Hey @user, can you review? cc @user");
        assert_eq!(
            rendered.mentions,
            vec!["5b10a2844c20165700ede21g".to_owned()]
        );
    }

    #[test]
    fn a_link_to_a_page_reads_as_its_title() {
        let storage =
            "<p>See <ac:link><ri:page ri:content-title=\"Q4 &amp; beyond\" /></ac:link> first.</p>";
        assert_eq!(render(storage).text, "See Q4 & beyond first.");
    }

    #[test]
    fn a_code_macro_keeps_its_lines_and_drops_its_parameters() {
        let storage = "<p>Run:</p><ac:structured-macro ac:name=\"code\">\
                       <ac:parameter ac:name=\"language\">bash</ac:parameter>\
                       <ac:plain-text-body><![CDATA[cargo test\n  --locked]]></ac:plain-text-body>\
                       </ac:structured-macro>";
        assert_eq!(render(storage).text, "Run:\n\ncargo test\n  --locked");
    }

    #[test]
    fn table_cells_are_separated_and_rows_are_blocks() {
        let storage = "<table><tbody><tr><th>Owner</th><th>Due</th></tr><tr><td>Ada</td><td>Friday</td></tr></tbody></table>";
        assert_eq!(render(storage).text, "Owner | Due\n\nAda | Friday");
    }

    #[test]
    fn character_references_are_decoded_and_unknown_ones_kept() {
        assert_eq!(
            render("<p>Tom&rsquo;s &#8220;plan&#x201D; &amp; &nbsp;more &bogus; A&B</p>").text,
            "Tom\u{2019}s \u{201c}plan\u{201d} & more &bogus; A&B"
        );
    }

    #[test]
    fn a_greater_than_inside_an_attribute_does_not_end_the_tag() {
        assert_eq!(render("<p title=\"a > b\">kept</p>").text, "kept");
    }

    #[test]
    fn comments_images_and_emoticons_carry_no_text() {
        let storage = "<p>Done<!-- hidden --> <ac:emoticon ac:name=\"tick\" /><ac:image><ri:attachment ri:filename=\"x.png\" /></ac:image></p>";
        assert_eq!(render(storage).text, "Done");
    }

    #[test]
    fn markup_that_is_not_well_formed_still_renders_its_text() {
        assert_eq!(render("<p>Half <b>bold").text, "Half bold");
        assert_eq!(
            render("plain text, no markup").text,
            "plain text, no markup"
        );
        assert_eq!(render("<p>cut off <a href=\"").text, "cut off");
    }
}
