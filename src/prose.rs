//! Prose: a Markdown text read as sentences, each with the line it starts on,
//! in English, Chinese or Japanese; whether one has any of a list of words;
//! and a sentence's hash, alike whatever Markdown marks it carries.

/// A Markdown text's sentences, each with the line it starts on: paragraphs,
/// list items, table cells and quotes split at `.`, `?` or `!` before a space,
/// at `。`, `！` or `？`, and at a hard line break; code blocks and headings
/// are none.
pub fn split(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut block: Vec<(usize, &str)> = Vec::new();
    let mut fenced = false;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim().trim_start_matches('>').trim();
        let item = ["-", "*", "+"]
            .iter()
            .any(|m| t == *m || t.starts_with(&format!("{m} ")))
            || t.split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if t.starts_with("```") || fenced || t.is_empty() || t.starts_with(['#', '|']) || item {
            sentences_of(&block, &mut out);
            block.clear();
        }
        if t.starts_with("```") {
            fenced = !fenced;
        } else if t.starts_with('|') {
            for cell in t.split('|') {
                sentences_of(&[(i + 1, cell.trim())], &mut out);
            }
        } else if !fenced && !t.is_empty() && !t.starts_with('#') {
            let t = match item {
                true => t.trim_start_matches(|c: char| "-*+.".contains(c) || c.is_ascii_digit()),
                false => t,
            };
            block.push((i + 1, t.trim()));
            // A hard line break ends the block, as one ends each field of a
            // `> **Key**: value  ` list.
            if line.ends_with("  ") || t.ends_with('\\') {
                sentences_of(&block, &mut out);
                block.clear();
            }
        }
    }
    sentences_of(&block, &mut out);
    out
}

fn sentences_of(block: &[(usize, &str)], out: &mut Vec<(usize, String)>) {
    let mut s = String::new();
    let mut start = 0;
    let push = |s: &mut String, start: &mut usize, line: usize, piece: &str| {
        if s.is_empty() {
            *start = line;
        } else {
            s.push(' ');
        }
        s.push_str(piece);
    };
    for (line, text) in block {
        for word in text.split_whitespace() {
            // A sentence in Chinese or Japanese ends at its full-width mark,
            // with no space after it: the word is cut there.
            let mut rest = word;
            while let Some(end) = cjk_end(rest) {
                let (piece, after) = rest.split_at(end);
                push(&mut s, &mut start, *line, piece);
                out.push((start, std::mem::take(&mut s)));
                rest = after;
            }
            if rest.is_empty() {
                continue;
            }
            push(&mut s, &mut start, *line, rest);
            let body = rest.trim_end_matches(['.', '?', '!', ')', '"', '*', '`']);
            if body.len() < rest.len()
                && rest[body.len()..].starts_with(['.', '?', '!'])
                && !body.contains('.')
            {
                out.push((start, std::mem::take(&mut s)));
            }
        }
    }
    if !s.is_empty() {
        out.push((start, s));
    }
}

/// Where the first sentence in `word` ends at a full-width `。`, `！`, `？`
/// (or `．`), with the marks, closing quotes and brackets right after it.
fn cjk_end(word: &str) -> Option<usize> {
    let (i, c) = word.char_indices().find(|(_, c)| "。！？．".contains(*c))?;
    let tail = &word[i + c.len_utf8()..];
    let closing = tail
        .find(|c: char| !"。！？．」』）】”’\"*`)".contains(c))
        .unwrap_or(tail.len());
    Some(i + c.len_utf8() + closing)
}

/// Whether `text` has any of `words` (`, `-separated): an English one as a
/// whole word, in any case; one with any other character (Chinese, Japanese,
/// `100%`) anywhere in the text, as those scripts put no space between words.
pub fn has_any(text: &str, words: &str) -> bool {
    let words_of = |s: &str| {
        let words: Vec<String> = s
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect();
        format!(" {} ", words.join(" "))
    };
    let whole = words_of(text);
    words.split(", ").map(str::trim).any(|w| {
        if w.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ') {
            whole.contains(&words_of(w))
        } else {
            text.contains(w)
        }
    })
}

/// A sentence as compared: its words, without Markdown's `*` and backticks.
pub fn plain(s: &str) -> String {
    s.replace(['*', '`'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A sentence's mark key: the hash of its words as compared ([`plain`]).
pub fn hash(sentence: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(plain(sentence));
    digest[..6].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_finds_sentences_with_their_lines_but_not_code_or_headings() {
        assert_eq!(
            split("**Bold** is no item.\n"),
            vec![(1, "**Bold** is no item.".into())]
        );
        let text = "# Never a heading\n\nOne line. And it\nwraps, e.g. here! Done\n- an item\n```\nnever code\n```\n| never | a cell. Two |\n";
        assert_eq!(
            split(text),
            vec![
                (3, "One line.".into()),
                (3, "And it wraps, e.g. here!".into()),
                (4, "Done".into()),
                (5, "an item".into()),
                (9, "never".into()),
                (9, "a cell.".into()),
                (9, "Two".into()),
            ]
        );
    }

    #[test]
    fn split_cuts_chinese_and_japanese_at_full_width_marks() {
        let quote = "> **状态**：草案  \n> **原则**：100% 本地。\n";
        assert_eq!(
            split(quote),
            vec![
                (1, "**状态**：草案".into()),
                (2, "**原则**：100% 本地。".into())
            ]
        );
        let text = "消息绝不丢失。重试最多三次！真的？「只此一次。」然后\n- 一度だけ実行する。決して止まらない\n";
        assert_eq!(
            split(text),
            vec![
                (1, "消息绝不丢失。".into()),
                (1, "重试最多三次！".into()),
                (1, "真的？".into()),
                (1, "「只此一次。」".into()),
                (1, "然后".into()),
                (2, "一度だけ実行する。".into()),
                (2, "決して止まらない".into()),
            ]
        );
    }
}
