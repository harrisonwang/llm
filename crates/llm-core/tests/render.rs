use llm_core::render::{should_render_for, write_markdown_output};

#[test]
fn should_render_only_for_tty_without_override() {
    assert!(should_render_for(false, true));
    assert!(!should_render_for(false, false));
    assert!(!should_render_for(true, true));
    assert!(!should_render_for(true, false));
}

#[test]
fn raw_markdown_output_preserves_text() {
    let mut out = Vec::new();

    write_markdown_output("## Title\n\n**bold**", false, &mut out).unwrap();

    assert_eq!(String::from_utf8(out).unwrap(), "## Title\n\n**bold**\n");
}

#[test]
fn rendered_markdown_output_writes_terminal_text() {
    let mut out = Vec::new();

    write_markdown_output("## Title", true, &mut out).unwrap();

    assert!(!out.is_empty());
    assert!(String::from_utf8(out).unwrap().contains("Title"));
}
