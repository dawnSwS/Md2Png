use super::{convert, validate_output};

#[test]
fn all_sized_delimiters_preserve_size_and_class() -> Result<(), String> {
    for (command, size) in [
        ("big", "1.2em"),
        ("Big", "1.8em"),
        ("bigg", "2.4em"),
        ("Bigg", "3em"),
    ] {
        for (suffix, class) in [
            ("", "normal"),
            ("l", "opening"),
            ("r", "closing"),
            ("m", "relation"),
        ] {
            let converted = convert(&format!("\\{command}{suffix}|"))?;
            assert!(
                converted.contains(&format!("class(\"{class}\"")),
                "{converted}"
            );
            assert!(converted.contains(&format!("size: #{size}")), "{converted}");
        }
    }
    Ok(())
}

#[test]
fn sized_delimiters_support_escaped_named_and_unpaired_forms() -> crate::AppResult<()> {
    let mut markup = crate::markdown::PREAMBLE.to_owned();
    for source in [
        r"\bigl\{x\bigr\}",
        r"\Bigl\langle x\Bigm|y\Bigr\rangle",
        r"\biggl\lfloor x\biggr\rfloor",
        r"\Biggl\lceil x\Biggr\rceil",
        r"\bigl(x\bigr]^2",
        r"\Biggr|_0^1",
        r"\big/",
        r"\big\backslash",
        r"\bigl.x\bigr)",
        "\\bigl % delimiter comment\n (x\\bigr)",
    ] {
        markup.push_str("$ ");
        markup.push_str(&convert(source)?);
        markup.push_str(" $\n\n");
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    crate::render::compile_document(markup, root)?;
    Ok(())
}

#[test]
fn malformed_sized_delimiters_are_rejected() {
    for source in [r"\bigl", r"\bigl x", r"\Bigl\unknown", r"\bigl{", r"\bigl#"] {
        assert!(convert(source).is_err(), "{source}");
    }
}

#[test]
fn supplied_aligned_formula_and_boxes_compile() -> crate::AppResult<()> {
    let formulas = [
        r"\begin{aligned}
A(X)
={}&X^+
-\left(\frac4R+0.10\right)-0.02-0.02\\
&+\frac12\bigl(99.98-(100+X+0.10)\bigr)\\
&-0.01-0.01-0.04-0.50\\
={}&X^+-\frac X2-\left(\frac4R+0.76\right).
\end{aligned}",
        r"\boxed{N=W_T-30R=A-D-\frac14(A-D)^+-0.03.}",
        r"\boxed{\boxed{x}+\overline{y}}^2 + \boxed{}",
        r"\biggl\lfloor x\biggr\rfloor + \Biggl\lceil y\Biggr\rceil",
        r"\frac{\bigl(x\bigr)^2}{\Bigl[y\Bigr]} + \Biggr|_0^1",
    ];
    let mut markup = crate::markdown::PREAMBLE.to_owned();
    for formula in formulas {
        markup.push_str("$ ");
        markup.push_str(&convert(formula)?);
        markup.push_str(" $\n\n");
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let document = crate::render::compile_document(markup, root)?;
    assert!(!document.pages().is_empty());
    Ok(())
}

#[test]
fn boxes_preserve_contents_and_limits() -> Result<(), String> {
    let boxed = convert(r"\boxed{x+1}")?;
    assert!(boxed.contains("md2pngboxed("), "{boxed}");
    assert!(boxed.contains("x + 1"), "{boxed}");
    assert!(!boxed.contains("overline"), "{boxed}");
    assert!(convert(r"\boxed x").is_err());
    assert!(convert(r"\boxed{x").is_err());
    let nested = format!("{}x{}", "\\boxed{".repeat(65), "}".repeat(65));
    assert!(convert(&nested).is_err());
    Ok(())
}

#[test]
fn added_lengths_do_not_allow_code_execution() {
    for unsafe_output in [
        "#1.2em.foo",
        "#1.2em()",
        "#1.2em[body]",
        "#1.2em; panic()",
        "#0.2em+1",
        "#read(\"secret\")",
    ] {
        assert!(validate_output(unsafe_output).is_err(), "{unsafe_output}");
    }
}
