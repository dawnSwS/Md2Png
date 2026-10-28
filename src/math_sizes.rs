//! Fixed-size LaTeX delimiters, independent of matching pairs or line breaks.

use std::{iter::Peekable, str::Chars};

// TeX's four conventional delimiter heights, in the current math font size.
const SAFE_LENGTHS: &[&str] = &["1.2em", "1.8em", "2.4em", "3em"];

pub(super) fn read(
    command: &str,
    chars: &mut Peekable<Chars<'_>>,
) -> Option<Result<String, String>> {
    let (base, class) = match command.chars().last()? {
        'l' => (command.strip_suffix('l')?, "opening"),
        'r' => (command.strip_suffix('r')?, "closing"),
        'm' => (command.strip_suffix('m')?, "relation"),
        _ => (command, "normal"),
    };
    let size = match base {
        "\\big" => "1.2em",
        "\\Big" => "1.8em",
        "\\bigg" => "2.4em",
        "\\Bigg" => "3em",
        _ => return None,
    };
    Some(read_delimiter(chars).map(|glyph| {
        if glyph.is_empty() {
            return format!("class(\"{class}\", \"\")");
        }
        // A quoted glyph avoids syntactic delimiter matching and the upstream
        // writer's floor/ceil rewrite, while class preserves TeX atom spacing.
        let glyph = crate::markdown::typst_string(glyph);
        format!("class(\"{class}\", stretch({glyph}, size: #{size}))")
    }))
}

pub(super) fn skip_ignored(chars: &mut Peekable<Chars<'_>>) {
    loop {
        while chars.next_if(|ch| ch.is_whitespace()).is_some() {}
        if chars.next_if_eq(&'%').is_none() {
            break;
        }
        for ch in chars.by_ref() {
            if ch == '\n' {
                break;
            }
        }
    }
}

fn read_delimiter(chars: &mut Peekable<Chars<'_>>) -> Result<&'static str, String> {
    skip_ignored(chars);
    let first = chars.next().ok_or("LaTeX 定尺寸命令后缺少括号")?;
    let mut delimiter = first.to_string();
    if first == '\\' {
        let next = chars.next().ok_or("LaTeX 括号转义未完成")?;
        delimiter.push(next);
        if next.is_ascii_alphabetic() {
            while let Some(ch) = chars.next_if(char::is_ascii_alphabetic) {
                delimiter.push(ch);
            }
        }
    }
    match delimiter.as_str() {
        "(" | "\\lparen" => Ok("("),
        ")" | "\\rparen" => Ok(")"),
        "[" | "\\lbrack" => Ok("["),
        "]" | "\\rbrack" => Ok("]"),
        "\\{" | "\\lbrace" => Ok("{"),
        "\\}" | "\\rbrace" => Ok("}"),
        "<" | "\\langle" => Ok("⟨"),
        ">" | "\\rangle" => Ok("⟩"),
        "|" | "\\vert" | "\\lvert" | "\\rvert" => Ok("|"),
        "\\|" | "\\Vert" | "\\lVert" | "\\rVert" => Ok("‖"),
        "\\lfloor" => Ok("⌊"),
        "\\rfloor" => Ok("⌋"),
        "\\lceil" => Ok("⌈"),
        "\\rceil" => Ok("⌉"),
        "\\lgroup" => Ok("⟮"),
        "\\rgroup" => Ok("⟯"),
        "\\lmoustache" => Ok("⎰"),
        "\\rmoustache" => Ok("⎱"),
        "\\uparrow" => Ok("↑"),
        "\\downarrow" => Ok("↓"),
        "\\updownarrow" => Ok("↕"),
        "\\Uparrow" => Ok("⇑"),
        "\\Downarrow" => Ok("⇓"),
        "\\Updownarrow" => Ok("⇕"),
        "/" => Ok("/"),
        "\\backslash" => Ok("\\"),
        "." => Ok(""),
        _ => Err(format!("LaTeX 定尺寸命令后不是支持的括号: {delimiter}")),
    }
}

pub(super) fn consume_length(chars: &mut Peekable<Chars<'_>>) -> bool {
    for length in SAFE_LENGTHS {
        let mut candidate = chars.clone();
        if length
            .chars()
            .all(|expected| candidate.next() == Some(expected))
            && matches!(candidate.peek(), None | Some(')' | ',' | ' ' | '\n'))
        {
            *chars = candidate;
            return true;
        }
    }
    false
}
