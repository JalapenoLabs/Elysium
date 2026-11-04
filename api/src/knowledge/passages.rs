// Copyright © 2026 Jalapeno Labs

//! Cuts a source's text into passages: the pieces the index matches words in, the embedding
//! model embeds, and a search cites.
//!
//! Text is cut at paragraph breaks and packed back together up to
//! [`PASSAGE_TARGET_CHARACTERS`], so a passage is a few paragraphs about one thing rather than a
//! fixed run of characters cut mid-sentence. A paragraph longer than that on its own is cut at
//! the last whitespace that fits. The cut depends on the text alone, so indexing the same text
//! again gives the same passages, and their embeddings are kept.

/// The size passages are packed to, in characters: about 400 tokens of English.
///
/// Small enough that a passage is about one thing, which is what a nearest-neighbour search and
/// a citation both want, and far inside the embedding model's 8,192-token context and the
/// 20,000 characters `knowledge_passages.content` allows. Changing it re-cuts every document the
/// next time it is indexed, and every passage whose text moved is embedded again.
pub const PASSAGE_TARGET_CHARACTERS: usize = 1_600;

/// The passages of `text`, in order. Blank text has none.
pub fn split(text: &str) -> Vec<String> {
    let normalized = text.replace("\r\n", "\n");
    let mut passages = Vec::new();
    let mut current = String::new();
    let mut current_length = 0;

    let pieces = normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .flat_map(cut_paragraph);
    for piece in pieces {
        let piece_length = piece.chars().count();
        // Two characters for the blank line that joins it to what came before.
        if current_length > 0 && current_length + 2 + piece_length > PASSAGE_TARGET_CHARACTERS {
            passages.push(std::mem::take(&mut current));
            current_length = 0;
        }
        if current_length > 0 {
            current.push_str("\n\n");
            current_length += 2;
        }
        current.push_str(piece);
        current_length += piece_length;
    }
    if current_length > 0 {
        passages.push(current);
    }
    passages
}

/// A paragraph in pieces no longer than [`PASSAGE_TARGET_CHARACTERS`], each cut at the last
/// whitespace that fits, or mid-word when a run of that many characters has none.
fn cut_paragraph(paragraph: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut rest = paragraph;
    loop {
        // The byte where the first PASSAGE_TARGET_CHARACTERS characters end, if the rest is
        // longer than that.
        let Some((limit, _)) = rest.char_indices().nth(PASSAGE_TARGET_CHARACTERS) else {
            pieces.push(rest);
            return pieces;
        };
        let window = &rest[..limit];
        let cut = window
            .rfind(char::is_whitespace)
            .filter(|&position| position > 0)
            .unwrap_or(limit);
        pieces.push(rest[..cut].trim_end());
        rest = rest[cut..].trim_start();
        if rest.is_empty() {
            return pieces;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lengths(passages: &[String]) -> Vec<usize> {
        passages
            .iter()
            .map(|passage| passage.chars().count())
            .collect()
    }

    #[test]
    fn blank_text_has_no_passages() {
        assert!(split("").is_empty());
        assert!(split(" \n\n \r\n\r\n ").is_empty());
    }

    #[test]
    fn short_paragraphs_share_one_passage_joined_by_a_blank_line() {
        let passages = split("Use COLLATE \"C\".\r\n\r\n\n\nIt never reorders.\n\nSee WAY-12.");
        assert_eq!(
            passages,
            ["Use COLLATE \"C\".\n\nIt never reorders.\n\nSee WAY-12."]
        );
    }

    #[test]
    fn paragraphs_are_packed_up_to_the_target_and_never_split_when_they_fit() {
        let paragraph = "word ".repeat(150).trim_end().to_owned(); // 749 characters
        let text = [paragraph.as_str(); 5].join("\n\n");

        let passages = split(&text);

        assert_eq!(lengths(&passages), [749 * 2 + 2, 749 * 2 + 2, 749]);
        assert!(passages.iter().all(|passage| passage.starts_with("word")));
    }

    #[test]
    fn a_long_paragraph_is_cut_at_whitespace_within_the_target() {
        let paragraph = "abcdefghi ".repeat(400); // 4,000 characters, a space every ten
        let passages = split(&paragraph);

        assert!(passages.len() >= 3, "cut into {} passages", passages.len());
        for passage in &passages {
            let length = passage.chars().count();
            assert!(length <= PASSAGE_TARGET_CHARACTERS, "{length} characters");
            assert!(
                passage.starts_with('a') && passage.ends_with('i'),
                "cut between words"
            );
        }
        let rejoined: Vec<&str> = passages
            .iter()
            .flat_map(|passage| passage.split(' '))
            .collect();
        assert_eq!(rejoined.len(), 400, "no word is lost or split");
    }

    #[test]
    fn a_run_without_whitespace_is_cut_mid_word_on_character_boundaries() {
        let run = "é".repeat(PASSAGE_TARGET_CHARACTERS * 2 + 5);
        let passages = split(&run);

        assert_eq!(
            lengths(&passages),
            [PASSAGE_TARGET_CHARACTERS, PASSAGE_TARGET_CHARACTERS, 5]
        );
    }

    #[test]
    fn the_same_text_is_always_cut_the_same_way() {
        let text = format!(
            "{}\n\nA closing line.",
            "Plenty of words here. ".repeat(200)
        );
        assert_eq!(split(&text), split(&text));
    }
}
