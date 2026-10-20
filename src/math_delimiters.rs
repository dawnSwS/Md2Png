//! Normalize common LaTeX delimiters only outside protected Markdown spans.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

pub(crate) fn normalize(md: &str, options: Options) -> String {
    let parser = Parser::new_ext(md, options);
    let mut protected: Vec<Range<usize>> = parser
        .reference_definitions()
        .iter()
        .map(|(_, definition)| definition.span.clone())
        .collect();
    let mut link: Option<(Range<usize>, Option<Range<usize>>)> = None;
    for (event, range) in parser.into_offset_iter() {
        match &event {
            Event::Start(Tag::Link { .. }) => link = Some((range.clone(), None)),
            Event::End(TagEnd::Link) => {
                if let Some((whole, body)) = link.take() {
                    if let Some(body) = body {
                        protected.push(whole.start..body.start);
                        protected.push(body.end..whole.end);
                    } else {
                        protected.push(whole);
                    }
                }
            }
            Event::Start(_) | Event::End(_) => {}
            _ => {
                if let Some((_, body)) = &mut link {
                    if let Some(body) = body {
                        body.end = range.end;
                    } else {
                        *body = Some(range.clone());
                    }
                }
            }
        }
        if matches!(
            event,
            Event::Code(_)
                | Event::Html(_)
                | Event::InlineHtml(_)
                | Event::InlineMath(_)
                | Event::DisplayMath(_)
                | Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Image { .. })
        ) {
            protected.push(range);
        }
    }
    protected.sort_by_key(|range| range.start);
    let mut output = String::with_capacity(md.len());
    let mut cursor = 0;
    for range in protected {
        if range.end <= cursor {
            continue;
        }
        if let Some(part) = md.get(cursor..range.start) {
            normalize_text(part, &mut output);
        }
        if let Some(part) = md.get(cursor.max(range.start)..range.end) {
            output.push_str(part);
        }
        cursor = range.end;
    }
    if let Some(part) = md.get(cursor..) {
        normalize_text(part, &mut output);
    }
    output
}

fn normalize_text(value: &str, output: &mut String) {
    let mut chars = value.char_indices().peekable();
    let mut missing_paren = false;
    let mut missing_bracket = false;
    while let Some((_, ch)) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        let Some((position, next)) = chars.next() else {
            output.push(ch);
            break;
        };
        let closing = match next {
            '(' => ')',
            '[' => ']',
            _ => {
                output.push(ch);
                output.push(next);
                continue;
            }
        };
        let missing = if closing == ')' {
            &mut missing_paren
        } else {
            &mut missing_bracket
        };
        if *missing {
            output.push(ch);
            output.push(next);
            continue;
        }
        let start = position + next.len_utf8();
        let Some(rest) = value.get(start..) else {
            continue;
        };
        if let Some(end) = closing_offset(rest, closing) {
            let delimiter = if closing == ')' { "$" } else { "$$" };
            output.push_str(delimiter);
            if let Some(body) = rest.get(..end) {
                output.push_str(body);
            }
            output.push_str(delimiter);
            let consumed = start + end + 2;
            while chars.peek().is_some_and(|(index, _)| *index < consumed) {
                chars.next();
            }
        } else {
            *missing = true;
            output.push(ch);
            output.push(next);
        }
    }
}

fn closing_offset(value: &str, closing: char) -> Option<usize> {
    let mut chars = value.char_indices();
    while let Some((offset, ch)) = chars.next() {
        if ch == '\\' {
            if let Some((_, next)) = chars.next() {
                if next == closing {
                    return Some(offset);
                }
            }
        }
    }
    None
}
