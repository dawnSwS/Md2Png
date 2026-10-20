//! Preserve LaTeX environment semantics missing from the pinned converter.
use std::collections::HashMap;
use tex2typst_rs::definitions::{
    TexNode, TexNodeData, TexNodeType, TypstNode, TypstNodeData, TypstNodeType,
};

type Replacements = HashMap<String, TypstNode>;

pub(crate) fn convert(mut node: TexNode, tex: &str) -> Result<TypstNode, String> {
    let mut prefix = String::from("md2png-environment-");
    while tex.contains(&prefix) {
        prefix.push('_');
    }
    let mut replacements = Replacements::new();
    prepare(&mut node, &prefix, &mut replacements)?;
    let mut output = tex2typst_rs::converter::convert_tree(&node)?;
    restore(&mut output, &mut replacements);
    Ok(output)
}

fn prepare(
    node: &mut TexNode,
    prefix: &str,
    replacements: &mut Replacements,
) -> Result<(), String> {
    if let Some(args) = &mut node.args {
        for child in args {
            prepare(child, prefix, replacements)?;
        }
    }
    if let Some(data) = node.data.as_deref_mut() {
        match data {
            TexNodeData::Supsub(data) => {
                prepare(&mut data.base, prefix, replacements)?;
                for child in [&mut data.sup, &mut data.sub].into_iter().flatten() {
                    prepare(child, prefix, replacements)?;
                }
            }
            TexNodeData::Array(rows) => {
                for child in rows.iter_mut().flatten() {
                    prepare(child, prefix, replacements)?;
                }
            }
        }
    }
    if node.node_type == TexNodeType::BeginEnd {
        let environment = environment(node)?;
        let key = format!("{prefix}{}", replacements.len());
        replacements.insert(key.clone(), environment);
        *node = TexNode::new(TexNodeType::Text, key, None, None);
    }
    Ok(())
}

fn environment(node: &TexNode) -> Result<TypstNode, String> {
    let Some(TexNodeData::Array(rows)) = node.data.as_deref() else {
        return Err("公式环境没有行数据".into());
    };
    let rows = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(tex2typst_rs::converter::convert_tree)
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let name = node.content.trim_end_matches('*');
    if matches!(name, "align" | "aligned" | "gather" | "gathered" | "split") {
        return Ok(TypstNode::new(
            TypstNodeType::Align,
            String::new(),
            None,
            Some(Box::new(TypstNodeData::Array(rows))),
        ));
    }
    if matches!(name, "cases" | "dcases") {
        return Ok(cases(rows));
    }
    let delimiter = match name {
        "matrix" | "smallmatrix" => "#none",
        "pmatrix" => "\"(\"",
        "bmatrix" => "\"[\"",
        "Bmatrix" => "\"{\"",
        "vmatrix" => "\"|\"",
        "Vmatrix" => "\"||\"",
        _ => return Err(format!("不支持的 LaTeX 公式环境: {}", node.content)),
    };
    let mut result = TypstNode::new(
        TypstNodeType::Matrix,
        String::new(),
        None,
        Some(Box::new(TypstNodeData::Array(rows))),
    );
    result.set_options(HashMap::from([("delim".to_owned(), delimiter.to_owned())]));
    Ok(result)
}

fn cases(rows: Vec<Vec<TypstNode>>) -> TypstNode {
    let args = rows
        .into_iter()
        .map(|row| {
            let mut cells = Vec::new();
            for (index, cell) in row.into_iter().enumerate() {
                if index != 0 {
                    cells.push(TypstNode::new(TypstNodeType::Atom, "&".into(), None, None));
                }
                cells.push(cell);
            }
            TypstNode::new(TypstNodeType::Group, String::new(), Some(cells), None)
        })
        .collect();
    TypstNode::new(TypstNodeType::FuncCall, "cases".into(), Some(args), None)
}

fn restore(node: &mut TypstNode, replacements: &mut Replacements) {
    if node.node_type == TypstNodeType::Text {
        if let Some(replacement) = replacements.remove(&node.content) {
            *node = replacement;
        }
    }
    if let Some(args) = &mut node.args {
        for child in args {
            restore(child, replacements);
        }
    }
    if let Some(data) = node.data.as_deref_mut() {
        match data {
            TypstNodeData::Supsub(data) => {
                restore(&mut data.base, replacements);
                for child in [&mut data.sup, &mut data.sub].into_iter().flatten() {
                    restore(child, replacements);
                }
            }
            TypstNodeData::Array(rows) => {
                for child in rows.iter_mut().flatten() {
                    restore(child, replacements);
                }
            }
        }
    }
}
