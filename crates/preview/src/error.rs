//! Typed boundary errors for supplied-byte preview decoding.

use thiserror::Error;

/// Typed error categories produced by bounded preview decoders and confined
/// helper wire protocols.
#[derive(Debug, Error)]
pub enum PreviewError {
    #[error("{0}")]
    InputTooLarge(String),
    #[error("{0}")]
    UnsupportedFormat(String),
    #[error("{0}")]
    InvalidDocument(String),
    #[error("{0}")]
    Cancelled(String),
    #[error("{0}")]
    DecodeFailed(String),
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[error("PDF preview helper failed")]
    PdfHelper(crate::pdf_helper::ErrorKind),
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[error("Image preview helper failed")]
    ImageHelper(crate::pdf_helper::ImageErrorKind),
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl From<crate::pdf_helper::ErrorKind> for PreviewError {
    fn from(kind: crate::pdf_helper::ErrorKind) -> Self {
        Self::PdfHelper(kind)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
impl From<crate::pdf_helper::ImageErrorKind> for PreviewError {
    fn from(kind: crate::pdf_helper::ImageErrorKind) -> Self {
        Self::ImageHelper(kind)
    }
}
