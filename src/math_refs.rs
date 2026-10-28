//! Resolve equation labels in two passes, including forward references.

use std::{collections::HashMap, fmt::Write as _, iter::Peekable, str::Chars};

#[derive(Default)]
pub(crate) struct References {
    labels: HashMap<String, String>,
    next_number: usize,
}

struct Equation {
    body: String,
    label: Option<String>,
    tag: Option<String>,
}

impl References {
    pub(crate) fn register(&mut self, tex: &str, block: bool) -> Result<(), String> {
        let equation = metadata(tex)?;
        if equation.label.is_none() && equation.tag.is_none() {
            return Ok(());
        }
        if !block {
            return Err("只有块级公式可以使用 \\label 或 \\tag".into());
        }
        self.next_number += 1;
        let number = equation.tag.unwrap_or_else(|| self.next_number.to_string());
        if let Some(label) = equation.label {
            if self.labels.insert(label.clone(), number).is_some() {
                return Err(format!("公式标签重复: {label}"));
            }
        }
        Ok(())
    }

    pub(crate) fn equation(&self, tex: &str, block: bool) -> Result<String, String> {
        let equation = metadata(tex)?;
        let body = self.expand(&equation.body, true)?;
        let converted = crate::math::convert(&body)?;
        let number = equation.tag.as_ref().or_else(|| {
            equation
                .label
                .as_ref()
                .and_then(|label| self.labels.get(label))
        });
        if let Some(number) = number {
            let number = crate::markdown::typst_string(&format!("({number})"));
            Ok(format!(
                "\n#math.equation(block: true, numbering: n => {number}, ${converted} $)\n"
            ))
        } else if block {
            Ok(format!("\n$ {converted} $\n"))
        } else {
            Ok(format!("${converted} $"))
        }
    }

    pub(crate) fn text(&self, value: &str) -> Result<String, String> {
        let expanded = self.expand(value, false)?;
        Ok(format!(
            "#text({})",
            crate::markdown::typst_string(&expanded)
        ))
    }

    fn expand(&self, value: &str, math: bool) -> Result<String, String> {
        let mut chars = value.chars().peekable();
        let mut result = String::with_capacity(value.len());
        let mut scope = TextScope::default();
        while let Some(ch) = chars.next() {
            if math && !scope.in_text() && ch == '%' {
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
                continue;
            }
            if ch != '\\' {
                scope.character(ch);
                result.push(ch);
                continue;
            }
            let command = command_name(&mut chars);
            if !matches!(command.as_str(), "ref" | "eqref") {
                scope.command(&command);
                result.push('\\');
                result.push_str(&command);
                continue;
            }
            let label = argument(&mut chars)?;
            let number = self
                .labels
                .get(&label)
                .ok_or_else(|| format!("引用了不存在的公式标签: {label}"))?;
            let number = if command == "eqref" {
                format!("({number})")
            } else {
                number.clone()
            };
            if math && !scope.in_text() {
                let _ = write!(result, "\\text{{{number}}}");
            } else {
                result.push_str(&number);
            }
        }
        Ok(result)
    }
}

fn metadata(tex: &str) -> Result<Equation, String> {
    let mut tex = tex.trim();
    for environment in ["equation", "equation*"] {
        let begin = format!("\\begin{{{environment}}}");
        let end = format!("\\end{{{environment}}}");
        if let Some(body) = tex
            .strip_prefix(&begin)
            .and_then(|body| body.strip_suffix(&end))
        {
            tex = body;
        }
    }
    let mut result = Equation {
        body: String::new(),
        label: None,
        tag: None,
    };
    let mut chars = tex.chars().peekable();
    let mut depth = 0_usize;
    while let Some(ch) = chars.next() {
        match ch {
            '%' => {
                result.body.push(ch);
                for next in chars.by_ref() {
                    result.body.push(next);
                    if next == '\n' {
                        break;
                    }
                }
            }
            '\\' => {
                let command = command_name(&mut chars);
                if depth == 0 && matches!(command.as_str(), "label" | "tag") {
                    let value = argument(&mut chars)?;
                    if value.is_empty()
                        || value
                            .chars()
                            .any(|c| c.is_control() || matches!(c, '{' | '}' | '\\' | '%'))
                    {
                        return Err("公式标签和编号必须是非空纯文本".into());
                    }
                    let target = if command == "label" {
                        &mut result.label
                    } else {
                        &mut result.tag
                    };
                    if target.replace(value).is_some() {
                        return Err(format!("同一公式不能多次使用 \\{command}"));
                    }
                } else {
                    result.body.push(ch);
                    result.body.push_str(&command);
                }
            }
            '{' => {
                depth += 1;
                result.body.push(ch);
            }
            '}' => {
                depth = depth.saturating_sub(1);
                result.body.push(ch);
            }
            ch => result.body.push(ch),
        }
    }
    Ok(result)
}

fn command_name(chars: &mut Peekable<Chars<'_>>) -> String {
    let mut result = String::new();
    while let Some(ch) = chars.next_if(char::is_ascii_alphabetic) {
        result.push(ch);
    }
    if result.is_empty() {
        if let Some(ch) = chars.next() {
            result.push(ch);
        }
    }
    result
}

fn argument(chars: &mut Peekable<Chars<'_>>) -> Result<String, String> {
    while chars.next_if(|ch| ch.is_whitespace()).is_some() {}
    if chars.next() != Some('{') {
        return Err("公式标签、编号或引用缺少 {参数}".into());
    }
    let mut result = String::new();
    for ch in chars.by_ref() {
        if ch == '}' {
            return Ok(result);
        }
        if matches!(ch, '{' | '\\') {
            return Err("公式标签和编号不支持嵌套命令".into());
        }
        result.push(ch);
    }
    Err("公式标签、编号或引用缺少 }".into())
}

#[derive(Default)]
struct TextScope {
    depth: usize,
    text_depth: Option<usize>,
    pending: bool,
}

impl TextScope {
    fn in_text(&self) -> bool {
        self.text_depth.is_some()
    }

    fn command(&mut self, name: &str) {
        self.pending = !self.in_text() && matches!(name, "text" | "operatorname");
    }

    fn character(&mut self, ch: char) {
        match ch {
            '{' => {
                self.depth += 1;
                if self.pending {
                    self.text_depth = Some(self.depth);
                    self.pending = false;
                }
            }
            '}' => {
                if self.text_depth == Some(self.depth) {
                    self.text_depth = None;
                }
                self.depth = self.depth.saturating_sub(1);
            }
            ch if !ch.is_whitespace() => self.pending = false,
            _ => {}
        }
    }
}
