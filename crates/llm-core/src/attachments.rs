use crate::chat::{ContentPart, ImageUrl, MessageContent};
use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ImageAttachment {
    pub data_url: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AttachmentKind {
    Image(&'static str),
    Pdf,
    Unsupported,
}

pub fn build_user_message_content(
    prompt: String,
    attachments: Vec<ImageAttachment>,
) -> MessageContent {
    if attachments.is_empty() {
        return MessageContent::Text(prompt);
    }

    let mut parts = Vec::with_capacity(attachments.len() + 1);
    parts.push(ContentPart::Text { text: prompt });
    parts.extend(
        attachments
            .into_iter()
            .map(|attachment| ContentPart::ImageUrl {
                image_url: ImageUrl {
                    url: attachment.data_url,
                },
            }),
    );
    MessageContent::Parts(parts)
}

pub fn read_image_attachments(paths: &[PathBuf]) -> Result<Vec<ImageAttachment>> {
    paths
        .iter()
        .map(|path| {
            let kind = attachment_kind(path);
            match kind {
                AttachmentKind::Image(mime_type) => {
                    let bytes = fs::read(path)
                        .with_context(|| format!("failed to read attachment: {}", path.display()))?;
                    Ok(ImageAttachment {
                        data_url: image_data_url(mime_type, &bytes),
                    })
                }
                AttachmentKind::Pdf => Err(anyhow!(
                    "PDF attachments are not supported; use `pith {} | llm \"...\"` instead",
                    path.display()
                )),
                AttachmentKind::Unsupported => Err(anyhow!(
                    "unsupported attachment type: {}; only png, jpg, jpeg, gif, and webp images are supported",
                    path.display()
                )),
            }
        })
        .collect()
}

pub fn image_data_url(mime_type: &str, bytes: &[u8]) -> String {
    format!("data:{mime_type};base64,{}", base64_encode(bytes))
}

pub fn attachment_kind(path: &Path) -> AttachmentKind {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return AttachmentKind::Unsupported;
    };

    match extension.to_ascii_lowercase().as_str() {
        "png" => AttachmentKind::Image("image/png"),
        "jpg" | "jpeg" => AttachmentKind::Image("image/jpeg"),
        "gif" => AttachmentKind::Image("image/gif"),
        "webp" => AttachmentKind::Image("image/webp"),
        "pdf" => AttachmentKind::Pdf,
        _ => AttachmentKind::Unsupported,
    }
}

pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;

        encoded.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(n & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}
