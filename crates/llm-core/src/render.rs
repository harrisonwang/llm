use anyhow::{Context, Result};
use std::io::Write;

pub fn should_render_for(no_render: bool, stdout_is_tty: bool) -> bool {
    !no_render && stdout_is_tty
}

pub fn write_markdown_output<W: Write>(text: &str, render: bool, out: &mut W) -> Result<()> {
    if render {
        termimad::MadSkin::default()
            .write_text_on(out, text)
            .context("failed to render markdown output")?;
        writeln!(out).context("failed to write final newline")?;
    } else {
        writeln!(out, "{text}").context("failed to write output")?;
    }
    Ok(())
}

pub enum StreamOutput {
    Raw,
    Rendered(String),
}

impl StreamOutput {
    pub fn new(render: bool) -> Self {
        if render {
            Self::Rendered(String::new())
        } else {
            Self::Raw
        }
    }

    pub fn write_text<W: Write>(&mut self, text: &str, out: &mut W) -> Result<()> {
        match self {
            Self::Raw => {
                out.write_all(text.as_bytes())
                    .context("failed to write streamed output")?;
                out.flush().context("failed to flush streamed output")?;
            }
            Self::Rendered(buffer) => buffer.push_str(text),
        }
        Ok(())
    }

    pub fn finish<W: Write>(&mut self, out: &mut W) -> Result<()> {
        match self {
            Self::Raw => writeln!(out).context("failed to write final newline")?,
            Self::Rendered(buffer) => write_markdown_output(buffer, true, out)?,
        }
        Ok(())
    }
}
