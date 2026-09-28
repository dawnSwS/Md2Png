use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag};
use std::fmt::Write as _;

pub const PREAMBLE: &str = include_str!("preamble.typ");
const MAX_DEPTH: usize = 128;

type ConvertResult<T> = Result<T, String>;

struct Node<'a> {
    offset: usize,
    kind: NodeKind<'a>,
}

enum NodeKind<'a> {
    Element(Tag<'a>, Vec<Node<'a>>),
    Leaf(Event<'a>),
}

struct OpenNode<'a> {
    tag: Tag<'a>,
    offset: usize,
    children: Vec<Node<'a>>,
}

/// Convert Markdown to Typst. Literal text and raw code are passed as string
/// arguments, not interpolated into Typst markup delimiters.
pub fn md_to_typst(md: &str) -> ConvertResult<String> {
    if md.len() > crate::input::MAX_INPUT_BYTES {
        return Err("Markdown 内容超过 8 MiB，请先拆分文档".into());
    }
    let md = md.strip_prefix('\u{feff}').unwrap_or(md);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH
        | Options::ENABLE_TASKLISTS;
    let mut root = Vec::new();
    let mut stack: Vec<OpenNode<'_>> = Vec::new();
    for (event, range) in Parser::new_ext(md, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if stack.len() >= MAX_DEPTH {
                    return Err("Markdown 嵌套超过 128 层，请简化列表或引用".into());
                }
                stack.push(OpenNode {
                    tag,
                    offset: range.start,
                    children: Vec::new(),
                });
            }
            Event::End(end) => {
                let open = stack.pop().ok_or("Markdown 结束标签没有匹配的开始标签")?;
                if open.tag.to_end() != end {
                    return Err("Markdown 标签不匹配".into());
                }
                push_node(
                    &mut root,
                    &mut stack,
                    Node {
                        offset: open.offset,
                        kind: NodeKind::Element(open.tag, open.children),
                    },
                );
            }
            event => push_node(
                &mut root,
                &mut stack,
                Node {
                    offset: range.start,
                    kind: NodeKind::Leaf(event),
                },
            ),
        }
    }
    if !stack.is_empty() {
        return Err("Markdown 标签没有闭合".into());
    }
    let mut output = PREAMBLE.to_owned();
    output.push_str(&Renderer { md }.nodes(&root)?);
    Ok(output)
}

fn push_node<'a>(root: &mut Vec<Node<'a>>, stack: &mut [OpenNode<'a>], node: Node<'a>) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        root.push(node);
    }
}

/// Typst string escaping is different from Markdown escaping and Rust Debug.
pub fn typst_string(text: &str) -> String {
    let mut output = String::with_capacity(text.len() + 2);
    output.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() => {
                let _ = write!(output, "\\u{{{:x}}}", ch as u32);
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
    output
}

fn text(value: &str) -> String {
    format!("#text({})", typst_string(value))
}

fn raw(value: &str, block: bool, lang: Option<&str>) -> String {
    let language = lang.map(typst_string).unwrap_or_else(|| "none".into());
    format!(
        "#raw({}, block: {block}, lang: {language})",
        typst_string(value)
    )
}

struct Renderer<'a> {
    md: &'a str,
}

impl Renderer<'_> {
    fn fail(&self, offset: usize, message: impl std::fmt::Display) -> String {
        let line = self.md[..offset.min(self.md.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1;
        format!("Markdown 第 {line} 行: {message}")
    }

    fn nodes(&self, nodes: &[Node<'_>]) -> ConvertResult<String> {
        let mut result = String::new();
        for node in nodes {
            result.push_str(&self.node(node)?);
        }
        Ok(result)
    }

    fn node(&self, node: &Node<'_>) -> ConvertResult<String> {
        let (tag, children) = match &node.kind {
            NodeKind::Leaf(event) => return self.leaf(event, node.offset),
            NodeKind::Element(tag, children) => (tag, children),
        };
        match tag {
            Tag::CodeBlock(kind) => {
                let value = plain_text(children);
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next(),
                    CodeBlockKind::Indented => None,
                };
                // Only one parser-added terminating newline is removed.
                let value = value.strip_suffix('\n').unwrap_or(&value);
                Ok(format!("\n{}\n", raw(value, true, lang)))
            }
            Tag::HtmlBlock => Ok(format!(
                "\n{}\n",
                raw(&plain_text(children), true, Some("html"))
            )),
            Tag::List(start) => {
                let tight = !children.iter().any(|item| match &item.kind {
                    NodeKind::Element(Tag::Item, body) => body
                        .iter()
                        .any(|node| matches!(&node.kind, NodeKind::Element(Tag::Paragraph, _))),
                    _ => false,
                });
                let mut result = match start {
                    Some(start) => format!("\n#enum(start: {start}, tight: {tight},\n"),
                    None => format!("\n#list(tight: {tight},\n"),
                };
                for item in children {
                    match &item.kind {
                        NodeKind::Element(Tag::Item, body) => {
                            result.push_str(&format!("[{}],\n", self.nodes(body)?));
                        }
                        _ => return Err(self.fail(item.offset, "列表中出现非列表项")),
                    }
                }
                result.push_str(")\n");
                Ok(result)
            }
            Tag::Table(alignments) => self.table(alignments, children, node.offset),
            Tag::Image { dest_url, .. } => {
                let path = normalize_image_path(dest_url).map_err(|e| self.fail(node.offset, e))?;
                Ok(format!(
                    "#image({}, width: 100%, alt: {})",
                    typst_string(&path),
                    typst_string(&plain_text(children))
                ))
            }
            _ => {
                let body = self.nodes(children)?;
                Ok(match tag {
                    Tag::Paragraph => format!("\n\n{body}\n\n"),
                    Tag::Heading { level, .. } => {
                        let level = match level {
                            HeadingLevel::H1 => 1,
                            HeadingLevel::H2 => 2,
                            HeadingLevel::H3 => 3,
                            HeadingLevel::H4 => 4,
                            HeadingLevel::H5 => 5,
                            HeadingLevel::H6 => 6,
                        };
                        format!("\n#heading(level: {level})[{body}]\n")
                    }
                    Tag::Strong => format!("#strong[{body}]"),
                    Tag::Emphasis => format!("#emph[{body}]"),
                    Tag::Strikethrough => format!("#strike[{body}]"),
                    Tag::BlockQuote(_) => format!("\n#quote(block: true, quotes: false)[{body}]\n"),
                    // A raster image cannot preserve clickable link annotations.
                    // Keep the visible link text and distinguish it visually.
                    Tag::Link { dest_url, .. } => {
                        let body = if body.is_empty() {
                            text(dest_url)
                        } else {
                            body
                        };
                        format!("#text(fill: rgb(\"0969da\"))[#underline[{body}]]")
                    }
                    _ => {
                        return Err(
                            self.fail(node.offset, format!("尚未支持的 Markdown 标签: {tag:?}"))
                        )
                    }
                })
            }
        }
    }

    fn leaf(&self, event: &Event<'_>, offset: usize) -> ConvertResult<String> {
        Ok(match event {
            Event::Text(value) => text(value),
            Event::Code(value) => raw(value, false, None),
            Event::SoftBreak => " ".into(),
            Event::HardBreak => "#linebreak()".into(),
            Event::Rule => "\n#block(width: 100%)[#line(length: 100%, stroke: 0.6pt)]\n".into(),
            Event::TaskListMarker(checked) => text(if *checked { "☑ " } else { "☐ " }),
            Event::InlineMath(value) => self.math(value, false, offset)?,
            Event::DisplayMath(value) => self.math(value, true, offset)?,
            Event::Html(value) | Event::InlineHtml(value) => {
                let lower = value.trim().to_ascii_lowercase();
                if matches!(lower.as_str(), "<br>" | "<br/>" | "<br />") {
                    "#linebreak()".into()
                } else {
                    // HTML/CSS rendering is intentionally unsupported. Never drop it silently.
                    text(value)
                }
            }
            _ => return Err(self.fail(offset, format!("尚未支持的 Markdown 事件: {event:?}"))),
        })
    }

    fn math(&self, tex: &str, block: bool, offset: usize) -> ConvertResult<String> {
        let converted = std::panic::catch_unwind(|| tex2typst_rs::tex2typst(tex))
            .map_err(|_| self.fail(offset, "公式转换库发生内部错误，请检查 LaTeX 语法"))?
            .map_err(|error| self.fail(offset, format!("LaTeX 公式转换失败: {error}")))?;
        let converted = converted.trim();
        validate_math_output(converted).map_err(|e| self.fail(offset, e))?;
        if block {
            // The whitespace must be INSIDE the dollars to select a block equation.
            Ok(format!("\n$ {converted} $\n"))
        } else {
            // Do not add extra spaces around inline formulas in CJK text.
            Ok(format!("${converted}$"))
        }
    }

    fn table(
        &self,
        alignments: &[Alignment],
        children: &[Node<'_>],
        offset: usize,
    ) -> ConvertResult<String> {
        if alignments.is_empty() {
            return Err(self.fail(offset, "表格没有列"));
        }
        let alignments = alignments
            .iter()
            .map(|alignment| match alignment {
                Alignment::Center => "center",
                Alignment::Right => "right",
                Alignment::Left | Alignment::None => "left",
            })
            .collect::<Vec<_>>();
        let mut result = format!(
            "\n#table(columns: {}, align: (x, y) => ({},).at(x),\n",
            alignments.len(),
            alignments.join(", ")
        );
        for row in children {
            match &row.kind {
                NodeKind::Element(Tag::TableHead, cells) => {
                    result.push_str("table.header(\n");
                    result.push_str(&self.cells(cells, true)?);
                    result.push_str("),\n");
                }
                NodeKind::Element(Tag::TableRow, cells) => {
                    result.push_str(&self.cells(cells, false)?)
                }
                _ => return Err(self.fail(row.offset, "无效的表格行结构")),
            }
        }
        result.push_str(")\n");
        Ok(result)
    }

    fn cells(&self, cells: &[Node<'_>], header: bool) -> ConvertResult<String> {
        let mut result = String::new();
        for cell in cells {
            match &cell.kind {
                NodeKind::Element(Tag::TableCell, content) => {
                    let body = self.nodes(content)?;
                    if header {
                        result.push_str(&format!("[#strong[{body}]],\n"));
                    } else {
                        result.push_str(&format!("[{body}],\n"));
                    }
                }
                NodeKind::Element(Tag::TableRow, nested) => {
                    result.push_str(&self.cells(nested, header)?)
                }
                _ => return Err(self.fail(cell.offset, "无效的表格单元格结构")),
            }
        }
        Ok(result)
    }
}

fn plain_text(nodes: &[Node<'_>]) -> String {
    let mut result = String::new();
    for node in nodes {
        match &node.kind {
            NodeKind::Element(_, children) => result.push_str(&plain_text(children)),
            NodeKind::Leaf(
                Event::Text(value)
                | Event::Code(value)
                | Event::Html(value)
                | Event::InlineHtml(value)
                | Event::InlineMath(value)
                | Event::DisplayMath(value),
            ) => result.push_str(value),
            NodeKind::Leaf(Event::SoftBreak | Event::HardBreak) => result.push('\n'),
            _ => {}
        }
    }
    result
}

// Do not allow generated math to escape its delimiters into Typst code.
// Escaped symbols and symbols inside a correctly escaped string remain valid.
pub fn validate_math_output(value: &str) -> ConvertResult<()> {
    if value.is_empty() {
        return Err("公式转换结果为空".into());
    }
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if !quoted
            && ((ch == '/' && matches!(chars.peek().copied(), Some('/' | '*')))
                || (ch == '*' && chars.peek() == Some(&'/')))
        {
            return Err("公式转换结果含不允许的 Typst 注释分隔符".into());
        }
        match ch {
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            '#' | '$' if !quoted => {
                return Err("公式转换结果包含不允许的 Typst 代码或公式分隔符".into())
            }
            _ => {}
        }
    }
    if quoted || escaped {
        return Err("公式转换结果含未闭合的字符串或转义符".into());
    }
    Ok(())
}

/// Decode Markdown URL escapes, then restrict images to relative project paths.
/// World::file performs a second canonical-path check, including symlinks.
pub fn normalize_image_path(value: &str) -> ConvertResult<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .ok_or("图片路径含不完整的百分号编码")?;
            let hex = std::str::from_utf8(hex).map_err(|_| "图片路径编码无效")?;
            decoded.push(u8::from_str_radix(hex, 16).map_err(|_| "图片路径编码无效")?);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(decoded)
        .map_err(|_| "图片路径不是 UTF-8")?
        .replace('\\', "/");
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(':')
        || path.chars().any(char::is_control)
    {
        return Err("仅支持文档目录内的相对图片路径；不支持网络 URL、绝对路径或 data URI".into());
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            ".." => return Err("图片路径不能使用 .. 访问父目录".into()),
            "" | "." => {}
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err("图片路径不能为空".into());
    }
    Ok(parts.join("/"))
}
