// Copyright © 2026 Jalapeno Labs

//! Atlassian Document Format, the JSON tree Jira Cloud stores rich text as.
//!
//! Descriptions and comments on Jira Cloud are documents, not strings, so Elysium has to
//! translate in both directions.
//!
//! Going out, callers hand in plain text and [`from_plain_text`] wraps it: blank lines
//! separate paragraphs, and a single newline inside a paragraph becomes a hard break. That
//! covers what a form and an agent write. It is deliberately not a Markdown renderer, so
//! `**bold**` reaches Jira as those six characters.
//!
//! Coming back, [`RichText`] carries both halves: the document exactly as Jira stored it,
//! for a client that renders it properly, and [`to_plain_text`]'s rendering for everything
//! else. The rendering keeps paragraphs, headings, list items, and code blocks as lines and
//! flattens the rest, which is enough to read an issue. A document in a shape this does not
//! know renders as much text as it can find rather than failing.

use serde::Serialize;
use serde_json::{Value, json};

/// The ADF version every Jira Cloud document carries today.
const DOCUMENT_VERSION: u8 = 1;

/// Rich text as Elysium returns it: Jira's own document, and a plain rendering of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RichText {
    /// The document rendered as text, with paragraphs and list items on their own lines.
    pub text: String,
    /// The document exactly as Jira stored it.
    pub adf: Value,
}

impl RichText {
    /// Reads a document Jira sent, rendering its text alongside it.
    pub fn from_document(document: Value) -> Self {
        Self {
            text: to_plain_text(&document),
            adf: document,
        }
    }
}

/// Wraps plain text into the smallest document Jira accepts.
///
/// Blank lines separate paragraphs; a single newline inside one becomes a hard break. Empty
/// text becomes an empty document, which is how Jira spells "no description".
pub fn from_plain_text(text: &str) -> Value {
    let paragraphs: Vec<Value> = text
        .replace("\r\n", "\n")
        .split("\n\n")
        .map(str::trim_end)
        .filter(|paragraph| !paragraph.trim().is_empty())
        .map(paragraph_node)
        .collect();

    json!({ "type": "doc", "version": DOCUMENT_VERSION, "content": paragraphs })
}

/// One paragraph, with its single newlines as hard breaks.
fn paragraph_node(paragraph: &str) -> Value {
    let mut content = Vec::new();
    for (index, line) in paragraph.split('\n').enumerate() {
        if index > 0 {
            content.push(json!({ "type": "hardBreak" }));
        }
        if !line.is_empty() {
            content.push(json!({ "type": "text", "text": line }));
        }
    }

    json!({ "type": "paragraph", "content": content })
}

/// Renders a document as text, keeping its blocks on separate lines.
pub fn to_plain_text(document: &Value) -> String {
    let mut rendered = String::new();
    render(document, &mut rendered);

    // Blocks each end in their own separator, so nesting can leave runs of blank lines
    // behind. One blank line between blocks is what the text would have been written with.
    let mut text = String::with_capacity(rendered.len());
    let mut consecutive_newlines = 0_usize;
    for character in rendered.chars() {
        if character == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines > 2 {
                continue;
            }
        } else {
            consecutive_newlines = 0;
        }
        text.push(character);
    }
    text.trim().to_owned()
}

/// What follows a node of this type once its children are rendered.
///
/// Anything not named here is a mark, an inline node, or a wrapper whose children carry
/// their own separators, so it adds nothing of its own.
fn separator_after(node_type: &str) -> &'static str {
    match node_type {
        "paragraph" | "heading" | "codeBlock" | "blockquote" | "panel" | "rule" | "table"
        | "mediaSingle" | "mediaGroup" => "\n\n",
        "listItem" | "tableRow" => "\n",
        _ => "",
    }
}

/// Appends one node's text, then its children's.
fn render(node: &Value, out: &mut String) {
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or_default();
    match node_type {
        "text" => {
            out.push_str(node.get("text").and_then(Value::as_str).unwrap_or_default());
            return;
        }
        "hardBreak" => {
            out.push('\n');
            return;
        }
        // A mention or an emoji renders as the text Jira stores for clients that cannot
        // draw it, such as `@Alex Navarro` or `:smile:`.
        "mention" | "emoji" => {
            let label = node
                .pointer("/attrs/text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            out.push_str(label);
            return;
        }
        _ => {}
    }

    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            render(child, out);
        }
    }
    out.push_str(separator_after(node_type));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_becomes_paragraphs_and_hard_breaks() {
        let document = from_plain_text("First line\nstill first\n\nSecond paragraph");

        assert_eq!(
            document,
            json!({
                "type": "doc",
                "version": 1,
                "content": [
                    {
                        "type": "paragraph",
                        "content": [
                            { "type": "text", "text": "First line" },
                            { "type": "hardBreak" },
                            { "type": "text", "text": "still first" },
                        ],
                    },
                    {
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "Second paragraph" }],
                    },
                ],
            })
        );
    }

    #[test]
    fn empty_text_becomes_an_empty_document() {
        assert_eq!(
            from_plain_text("   \n\n  "),
            json!({ "type": "doc", "version": 1, "content": [] })
        );
    }

    #[test]
    fn markdown_is_not_interpreted() {
        let document = from_plain_text("**not bold**");
        assert_eq!(
            document.pointer("/content/0/content/0/text"),
            Some(&json!("**not bold**"))
        );
    }

    #[test]
    fn a_document_renders_its_blocks_on_their_own_lines() {
        let document = json!({
            "type": "doc",
            "version": 1,
            "content": [
                { "type": "heading", "attrs": { "level": 2 },
                  "content": [{ "type": "text", "text": "Heading" }] },
                { "type": "paragraph", "content": [
                    { "type": "text", "text": "Hello " },
                    { "type": "text", "text": "world", "marks": [{ "type": "strong" }] },
                ] },
                { "type": "bulletList", "content": [
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "one" }] },
                    ] },
                    { "type": "listItem", "content": [
                        { "type": "paragraph", "content": [{ "type": "text", "text": "two" }] },
                    ] },
                ] },
                { "type": "paragraph", "content": [
                    { "type": "mention", "attrs": { "id": "5b10", "text": "@Alex" } },
                    { "type": "text", "text": " look" },
                ] },
            ],
        });

        assert_eq!(
            to_plain_text(&document),
            "Heading\n\nHello world\n\none\n\ntwo\n\n@Alex look"
        );
    }

    #[test]
    fn text_survives_a_round_trip_and_odd_documents_render_what_they_can() {
        let original = "One paragraph.\n\nAnd another.";
        assert_eq!(to_plain_text(&from_plain_text(original)), original);

        assert_eq!(to_plain_text(&json!({})), "");
        assert_eq!(to_plain_text(&json!({ "type": "doc" })), "");
        assert_eq!(
            to_plain_text(&json!({ "type": "doc", "content": [
                { "type": "somethingNew", "content": [{ "type": "text", "text": "kept" }] },
            ] })),
            "kept",
            "an unknown node still gives up its text"
        );
    }

    #[test]
    fn rich_text_carries_the_document_and_its_rendering() {
        let document = json!({ "type": "doc", "version": 1, "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": "Read me" }] },
        ] });
        let rich = RichText::from_document(document.clone());

        assert_eq!(rich.text, "Read me");
        assert_eq!(rich.adf, document);
        assert_eq!(
            serde_json::to_value(&rich).expect("serializes"),
            json!({ "text": "Read me", "adf": document })
        );
    }
}
