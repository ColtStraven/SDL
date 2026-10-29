//! Structured warnings and errors with section, offset and reason.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable machine-readable code, e.g. `unknown_format_magic`.
    pub code: &'static str,
    pub section: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet_index: Option<u64>,
    pub message: String,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(transparent)]
pub struct Diagnostics(pub Vec<Diagnostic>);

impl Diagnostics {
    pub fn push(
        &mut self,
        severity: Severity,
        code: &'static str,
        section: &'static str,
        offset: Option<u64>,
        message: impl Into<String>,
    ) {
        self.0.push(Diagnostic {
            severity,
            code,
            section,
            offset,
            packet_index: None,
            message: message.into(),
        });
    }

    pub fn info(&mut self, code: &'static str, section: &'static str, offset: Option<u64>, msg: impl Into<String>) {
        self.push(Severity::Info, code, section, offset, msg);
    }

    pub fn warn(&mut self, code: &'static str, section: &'static str, offset: Option<u64>, msg: impl Into<String>) {
        self.push(Severity::Warning, code, section, offset, msg);
    }

    pub fn error(&mut self, code: &'static str, section: &'static str, offset: Option<u64>, msg: impl Into<String>) {
        self.push(Severity::Error, code, section, offset, msg);
    }

    pub fn has_errors(&self) -> bool {
        self.0.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn count(&self, severity: Severity) -> usize {
        self.0.iter().filter(|d| d.severity == severity).count()
    }
}
