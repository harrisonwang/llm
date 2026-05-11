use llm_core::attachments::{
    AttachmentKind, ImageAttachment, attachment_kind, build_user_message_content, image_data_url,
    read_image_attachments,
};
use std::path::{Path, PathBuf};

#[test]
fn image_data_url_base64_encodes_bytes() {
    assert_eq!(
        image_data_url("image/png", b"hello"),
        "data:image/png;base64,aGVsbG8="
    );
}

#[test]
fn attachment_kind_detects_supported_images() {
    assert_eq!(
        attachment_kind(Path::new("a.PNG")),
        AttachmentKind::Image("image/png")
    );
    assert_eq!(
        attachment_kind(Path::new("a.jpeg")),
        AttachmentKind::Image("image/jpeg")
    );
    assert_eq!(
        attachment_kind(Path::new("a.webp")),
        AttachmentKind::Image("image/webp")
    );
}

#[test]
fn attachment_kind_rejects_pdf_boundary() {
    assert_eq!(attachment_kind(Path::new("doc.pdf")), AttachmentKind::Pdf);
}

#[test]
fn pdf_attachment_errors_with_pith_guidance() {
    let err = read_image_attachments(&[PathBuf::from("doc.pdf")])
        .unwrap_err()
        .to_string();

    assert!(err.contains("PDF attachments are not supported"));
    assert!(err.contains("pith doc.pdf | llm"));
}

#[test]
fn user_message_with_attachment_uses_openai_image_block() {
    let content = build_user_message_content(
        "检查 UI".to_string(),
        vec![ImageAttachment {
            data_url: "data:image/png;base64,AAAA".to_string(),
        }],
    );
    let value = serde_json::to_value(&content).unwrap();

    assert_eq!(value[0]["type"], "text");
    assert_eq!(value[0]["text"], "检查 UI");
    assert_eq!(value[1]["type"], "image_url");
    assert_eq!(value[1]["image_url"]["url"], "data:image/png;base64,AAAA");
}
