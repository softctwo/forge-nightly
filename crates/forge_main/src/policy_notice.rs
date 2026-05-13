use std::fmt;
use std::path::{Path, PathBuf};

use colored::Colorize;

/// A single row rendered inside a [`PolicyNotice`].
enum Row {
    /// A bold label followed by a plain value on the same line. If `value` is
    /// empty the label is rendered alone.
    KeyValue { label: String, value: String },
    /// A bold label followed by a comma-separated, truncated item list.
    Items { label: String, items: Vec<String>, max_display: usize },
}

impl Row {
    fn render(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Row::KeyValue { label, value } if value.is_empty() => {
                write!(f, "  {}", label.bold())
            }
            Row::KeyValue { label, value } => {
                write!(f, "  {} {value}", label.bold())
            }
            Row::Items { label, items, max_display } => {
                let shown = items
                    .iter()
                    .take(*max_display)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let list = if items.len() > *max_display {
                    format!("{shown} +{} more", items.len() - max_display)
                } else {
                    shown
                };
                write!(f, "  {} {list}", label.bold())
            }
        }
    }
}

/// A composable terminal notice for policy-blocked items.
///
/// Build up any combination of key-value rows, plain text rows, and truncated
/// item-list rows, then optionally attach a dimmed docs hyperlink at the end.
/// The `Display` impl renders each row indented with bold labels.
///
/// # Example
///
/// ```rust,ignore
/// let notice = PolicyNotice::new()
///     .row("To enable them, configure", tilde_path(&permissions_path))
///     .row("See docs for permission examples:", "")
///     .text("https://forgecode.dev/docs/permissions/")
///     .items("Blocked servers:", server_names, 3);
///
/// // Or use the built-in docs hyperlink:
/// let notice = PolicyNotice::new()
///     .row("Configure permissions:", tilde_path(&permissions_path))
///     .items("Blocked servers:", server_names, 3)
///     .docs("https://forgecode.dev/docs/permissions/");
/// ```
#[derive(Default)]
pub struct PolicyNotice {
    rows: Vec<Row>,
    docs: Option<String>,
}

impl PolicyNotice {
    /// Creates an empty notice.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a bold-label + plain-value row. Pass an empty string as `value`
    /// to render the label alone (e.g. as a section header).
    pub fn row(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.rows.push(Row::KeyValue { label: label.into(), value: value.into() });
        self
    }

    /// Appends a bold-label + truncated item-list row.
    pub fn items(
        mut self,
        label: impl Into<String>,
        items: Vec<String>,
        max_display: usize,
    ) -> Self {
        self.rows.push(Row::Items { label: label.into(), items, max_display });
        self
    }

    /// Attaches a dimmed OSC 8 clickable hyperlink rendered as the last line.
    pub fn docs(mut self, url: impl Into<String>) -> Self {
        self.docs = Some(url.into());
        self
    }
}

impl fmt::Display for PolicyNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for row in &self.rows {
            if !first {
                writeln!(f)?;
            }
            row.render(f)?;
            first = false;
        }
        if let Some(url) = &self.docs {
            let link = format!("\x1b]8;;{url}\x1b\\{url}\x1b]8;;\x1b\\");
            if !first {
                writeln!(f)?;
            }
            write!(f, "  {}", format!("Docs: {link}").dimmed())?;
        }
        Ok(())
    }
}

/// Abbreviates a path by replacing the home directory prefix with `~`.
pub fn tilde_path(path: &PathBuf) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = Path::new(&home);
        path.strip_prefix(home_path)
            .map(|p| format!("~/{}", p.display()))
            .unwrap_or_else(|_| path.display().to_string())
    } else {
        path.display().to_string()
    }
}
