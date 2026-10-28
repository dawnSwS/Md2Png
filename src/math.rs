//! Safe adapter around the pinned LaTeX converter.
//!
//! Version 0.4.1 mixes byte and character offsets in its tokenizer and emits
//! unescaped Typst strings. Keep its parser and mathematical conversion, but
//! supply UTF-8-safe tokens and escape text before the writer finalizes output.

use std::{iter::Peekable, str::Chars};
use tex2typst_rs::{
    definitions::{TexToken, TexTokenType, TypstTokenType},
    tex_parser::LatexParser,
    typst_writer::TypstWriter,
};

#[path = "math_sizes.rs"]
mod sizes;

// A single synthetic token cannot be produced by the character tokenizer.
pub(crate) const BOXED_MARKER: &str = "md2png-boxed-body";

const MAX_MATH_BYTES: usize = 64 * 1024;
const MAX_MATH_DEPTH: usize = 64;

pub(crate) fn convert(tex: &str) -> Result<String, String> {
    if tex.len() > MAX_MATH_BYTES {
        return Err("单个公式超过 64 KiB，请拆分公式".into());
    }
    let tokens = tokenize(tex)?;
    let complex_commands = tokens
        .iter()
        .filter(|token| {
            let name = token.value.strip_prefix('\\').unwrap_or("");
            token.token_type == TexTokenType::Command
                && (tex2typst_rs::command_registry::UNARY_COMMANDS.contains(&name)
                    || tex2typst_rs::command_registry::BINARY_COMMANDS.contains(&name)
                    || tex2typst_rs::command_registry::OPTION_BINARY_COMMANDS.contains(&name)
                    || matches!(name, "begin" | "left"))
        })
        .count();
    if complex_commands > 256 {
        return Err("单个公式包含超过 256 个复合命令，请拆分公式".into());
    }
    let converted = std::panic::catch_unwind(|| {
        let parsed = LatexParser::new(false, false).parse(tokens)?;
        let tree = crate::math_tree::convert(parsed, tex)?;
        let mut writer = TypstWriter::new();
        writer.serialize(&tree)?;
        for token in &mut writer.queue {
            if token.token_type == TypstTokenType::Text {
                let quoted = crate::markdown::typst_string(&token.value);
                quoted
                    .strip_prefix('"')
                    .and_then(|value| value.strip_suffix('"'))
                    .ok_or("公式文本转义失败")?
                    .clone_into(&mut token.value);
            }
        }
        writer.finalize()
    })
    .map_err(|_| "公式转换库发生内部错误，请检查 LaTeX 语法".to_owned())??;
    let converted = converted.trim().to_owned();
    validate_output(&converted)?;
    let syntax = typst::syntax::parse(&format!("$ {converted} $"));
    if syntax.diagnosis().errors {
        return Err("公式转换结果不是有效的 Typst 数学语法".into());
    }
    Ok(converted)
}

fn token(kind: TexTokenType, value: impl Into<String>) -> TexToken {
    TexToken::new(kind, value.into())
}

fn tokenize(tex: &str) -> Result<Vec<TexToken>, String> {
    let mut chars = tex.chars().peekable();
    let mut result = Vec::new();
    let mut depth = 0_usize;
    while let Some(ch) = chars.next() {
        match ch {
            '%' => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '\\' => read_command(&mut chars, &mut result, &mut depth)?,
            '{' => {
                depth += 1;
                if depth > MAX_MATH_DEPTH {
                    return Err("公式嵌套超过 64 层".into());
                }
                result.push(token(TexTokenType::Control, "{"));
            }
            '}' => {
                depth = depth.checked_sub(1).ok_or("公式包含未匹配的 }")?;
                result.push(token(TexTokenType::Control, "}"));
            }
            '_' | '^' | '&' => result.push(token(TexTokenType::Control, ch.to_string())),
            '~' => result.push(token(TexTokenType::NoBreakSpace, "space.nobreak")),
            // TeX ignores whitespace outside text. Removing it also avoids the
            // upstream parser treating whitespace as a macro argument.
            ch if ch.is_whitespace() => {}
            ch if ch.is_control() => return Err("公式含不可见控制字符".into()),
            ch => result.push(token(TexTokenType::Element, ch.to_string())),
        }
    }
    if depth != 0 {
        return Err("公式包含未闭合的 {".into());
    }
    Ok(result)
}

fn read_command(
    chars: &mut Peekable<Chars<'_>>,
    result: &mut Vec<TexToken>,
    depth: &mut usize,
) -> Result<(), String> {
    let first = chars.next().ok_or("公式末尾的反斜线没有对应命令")?;
    if !first.is_ascii_alphabetic() {
        return read_control_symbol(first, result);
    }
    let mut command = String::from("\\");
    command.push(first);
    while let Some(ch) = chars.next_if(char::is_ascii_alphabetic) {
        command.push(ch);
    }
    if let Some(delimiter) = sizes::read(&command, chars) {
        result.push(token(TexTokenType::Element, delimiter?));
        return Ok(());
    }
    if command == "\\boxed" {
        sizes::skip_ignored(chars);
        if chars.next() != Some('{') {
            return Err("\\boxed 命令缺少 {公式}".into());
        }
        *depth += 1;
        if *depth > MAX_MATH_DEPTH {
            return Err("公式嵌套超过 64 层".into());
        }
        // Let the upstream unary parser own the argument and its scripts.
        // math_tree restores the box before the ordinary overline conversion.
        result.push(token(TexTokenType::Command, "\\overline"));
        result.push(token(TexTokenType::Control, "{"));
        result.push(token(TexTokenType::Element, BOXED_MARKER));
        return Ok(());
    }
    validate_command(&command)?;
    let text_argument = matches!(
        command.as_str(),
        "\\text" | "\\operatorname" | "\\begin" | "\\end"
    );
    result.push(token(TexTokenType::Command, command));
    if text_argument {
        let body = read_braced_text(chars)?;
        result.push(token(TexTokenType::Control, "{"));
        result.push(token(TexTokenType::Text, body));
        result.push(token(TexTokenType::Control, "}"));
    }
    Ok(())
}

fn validate_command(command: &str) -> Result<(), String> {
    use tex2typst_rs::{command_registry, map::SYMBOL_MAP};
    const NATIVE: &[&str] = &[
        "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh",
        "tanh", "coth", "sech", "csch", "exp", "ln", "log", "lim", "limsup", "liminf", "max",
        "min", "inf", "sup", "arg", "det", "dim", "gcd", "ker", "hom", "deg", "Pr", "mod", "id",
        "im", "begin", "end", "left", "right",
    ];
    let name = command.strip_prefix('\\').unwrap_or(command);
    if SYMBOL_MAP.contains_key(name)
        || NATIVE.contains(&name)
        || command_registry::UNARY_COMMANDS.contains(&name)
        || command_registry::BINARY_COMMANDS.contains(&name)
        || command_registry::OPTION_BINARY_COMMANDS.contains(&name)
    {
        Ok(())
    } else {
        Err(format!("不支持的 LaTeX 命令: {command}"))
    }
}

fn read_control_symbol(first: char, result: &mut Vec<TexToken>) -> Result<(), String> {
    let spacing = match first {
        ':' | '>' => Some("med"),
        ';' => Some("thick"),
        '!' => Some("#h(-0.1666666667em)"),
        '/' => return Ok(()), // Typst performs math italic correction itself.
        _ => None,
    };
    if let Some(spacing) = spacing {
        result.push(token(TexTokenType::Element, spacing));
        return Ok(());
    }
    let value = format!("\\{first}");
    let kind = match first {
        '\\' | ',' => TexTokenType::Control,
        '{' | '}' | '%' | '$' | '&' | '#' | '_' | '|' => TexTokenType::Element,
        ch if ch.is_whitespace() => {
            result.push(token(TexTokenType::NoBreakSpace, "space.nobreak"));
            return Ok(());
        }
        _ => return Err(format!("不支持的 LaTeX 控制符: {value}")),
    };
    result.push(token(kind, value));
    Ok(())
}

fn read_braced_text(chars: &mut Peekable<Chars<'_>>) -> Result<String, String> {
    while chars.next_if(|ch| ch.is_whitespace()).is_some() {}
    if chars.next() != Some('{') {
        return Err("LaTeX 文本或环境命令缺少 {参数}".into());
    }
    let mut depth = 1_usize;
    let mut result = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let next = chars.next().ok_or("LaTeX 文本末尾的转义未完成")?;
                if !matches!(next, '{' | '}' | '\\' | '$' | '&' | '#' | '_' | '%') {
                    result.push('\\');
                }
                result.push(next);
            }
            '{' => {
                depth += 1;
                if depth > MAX_MATH_DEPTH {
                    return Err("公式文本嵌套超过 64 层".into());
                }
                result.push(ch);
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(result);
                }
                result.push(ch);
            }
            ch => result.push(ch),
        }
    }
    Err("LaTeX 文本或环境参数缺少 }".into())
}

/// Check converter output before embedding it into Typst math markup.
///
/// # Errors
/// Returns an error for empty math, code escapes, comments or invalid strings.
pub fn validate_output(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("公式转换结果为空".into());
    }
    let mut quoted = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                // Outside a string, a backslash followed by whitespace is a
                // Typst math line break. Preserve a final line break too.
                if quoted || chars.peek().is_some_and(|next| !next.is_whitespace()) {
                    chars.next().ok_or("公式转换结果含未完成的字符串转义")?;
                }
            }
            '"' => quoted = !quoted,
            '$' if !quoted => return Err("公式转换结果含不允许的公式分隔符".into()),
            '#' if !quoted => consume_safe_literal(&mut chars)?,
            '/' if !quoted && matches!(chars.peek(), Some('/' | '*')) => {
                return Err("公式转换结果含不允许的 Typst 注释".into());
            }
            _ => {}
        }
    }
    if quoted {
        return Err("公式转换结果含未闭合的字符串".into());
    }
    Ok(())
}

fn consume_safe_literal(chars: &mut Peekable<Chars<'_>>) -> Result<(), String> {
    if sizes::consume_length(chars) {
        return Ok(());
    }
    let literal = match chars.peek() {
        Some('n') => "none",
        Some('h') => "h(-0.1666666667em)",
        _ => return Err("公式转换结果包含不允许的 Typst 代码".into()),
    };
    for expected in literal.chars() {
        if chars.next() != Some(expected) {
            return Err("公式转换结果包含不允许的 Typst 代码".into());
        }
    }
    if chars
        .peek()
        .is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '(' | '['))
    {
        return Err("公式转换结果包含不允许的 Typst 代码".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "math_tests.rs"]
mod tests;
