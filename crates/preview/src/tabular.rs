//! Bounded RFC 4180 CSV / TSV tabular preview from supplied UTF-8 text.
//!
//! Never opens files, allocates beyond fixed caps, or runs external code.
//! Literal Git patches remain separate in `crates/app`.

use std::path::Path;

/// Maximum input document size parsed into a table preview (2 MiB).
pub const MAX_TABULAR_BYTES: usize = 2 * 1024 * 1024;
/// Maximum retained data rows (excluding the optional header row).
pub const MAX_TABULAR_ROWS: usize = 10_000;
/// Maximum retained columns per row.
pub const MAX_TABULAR_COLUMNS: usize = 128;
/// Maximum retained UTF-8 bytes per cell before explicit truncation.
pub const MAX_TABULAR_CELL_BYTES: usize = 4 * 1024;
/// Maximum aggregate retained bytes across all cells (4 MiB).
pub const MAX_TABULAR_RETAINED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabularPreview {
    pub delimiter: char,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub column_count: usize,
    pub total_rows: usize,
    pub truncated_rows: bool,
    pub truncated_columns: bool,
    pub truncated_cells: bool,
    pub truncated_bytes: bool,
}

impl TabularPreview {
    pub fn retained_bytes(&self) -> usize {
        let header_bytes: usize = self
            .headers
            .iter()
            .map(|cell| cell.len() + std::mem::size_of::<String>())
            .sum();
        let row_bytes: usize = self
            .rows
            .iter()
            .map(|row| {
                std::mem::size_of::<Vec<String>>()
                    + row
                        .iter()
                        .map(|cell| cell.len() + std::mem::size_of::<String>())
                        .sum::<usize>()
            })
            .sum();
        std::mem::size_of::<Self>() + header_bytes + row_bytes
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated_rows
            || self.truncated_columns
            || self.truncated_cells
            || self.truncated_bytes
    }
}

/// Returns `Some(delimiter)` for supported tabular extensions (`.csv`, `.tsv`).
pub fn tabular_delimiter(path: &Path) -> Option<char> {
    let ext = path.extension()?.to_str()?;
    if ext.eq_ignore_ascii_case("csv") {
        Some(',')
    } else if ext.eq_ignore_ascii_case("tsv") {
        Some('\t')
    } else {
        None
    }
}

/// Parse supplied CSV/TSV text into a bounded [`TabularPreview`].
/// Returns `None` when `path` is not `.csv`/`.tsv`, or when the source is empty
/// or exceeds [`MAX_TABULAR_BYTES`].
pub fn parse_tabular(source: &str, path: &Path) -> Option<TabularPreview> {
    let delimiter = tabular_delimiter(path)?;
    if source.is_empty() || source.len() > MAX_TABULAR_BYTES {
        return None;
    }
    let text = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut all_rows: Vec<Vec<String>> = Vec::new();
    let mut column_count = 0usize;
    let mut total_rows = 0usize;
    let mut truncated_rows = false;
    let mut truncated_columns = false;
    let mut truncated_cells = false;
    let mut truncated_bytes = false;
    let mut retained_bytes = 0usize;

    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut in_quotes = false;
    let mut field_start = true;
    let mut cell_truncated = false;
    let mut row_col_count = 0usize;

    let mut chars = text.chars().peekable();
    let flush_cell = |row: &mut Vec<String>,
                      cell: &mut String,
                      row_col_count: &mut usize,
                      truncated_columns: &mut bool,
                      truncated_cells: &mut bool,
                      truncated_bytes: &mut bool,
                      retained_bytes: &mut usize,
                      cell_truncated: &mut bool,
                      keep_row: bool| {
        *row_col_count += 1;
        if !keep_row {
            cell.clear();
            *cell_truncated = false;
            return;
        }
        if *cell_truncated {
            *truncated_cells = true;
        }
        if row.len() < MAX_TABULAR_COLUMNS {
            let value = std::mem::take(cell);
            let cost = value.len() + std::mem::size_of::<String>();
            if retained_bytes.saturating_add(cost) <= MAX_TABULAR_RETAINED_BYTES {
                *retained_bytes += cost;
                row.push(value);
            } else {
                *truncated_bytes = true;
            }
        } else {
            *truncated_columns = true;
            cell.clear();
        }
        *cell_truncated = false;
    };

    while let Some(ch) = chars.next() {
        let keep_row = all_rows.len() <= MAX_TABULAR_ROWS && !truncated_bytes;
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    if keep_row {
                        push_bounded_cell(&mut cell, '"', &mut cell_truncated);
                    }
                } else {
                    in_quotes = false;
                }
            } else if keep_row {
                push_bounded_cell(&mut cell, ch, &mut cell_truncated);
            }
            continue;
        }

        if field_start && ch == '"' {
            in_quotes = true;
            field_start = false;
            continue;
        }

        if ch == delimiter {
            flush_cell(
                &mut row,
                &mut cell,
                &mut row_col_count,
                &mut truncated_columns,
                &mut truncated_cells,
                &mut truncated_bytes,
                &mut retained_bytes,
                &mut cell_truncated,
                keep_row,
            );
            field_start = true;
        } else if ch == '\r' || ch == '\n' {
            if ch == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            flush_cell(
                &mut row,
                &mut cell,
                &mut row_col_count,
                &mut truncated_columns,
                &mut truncated_cells,
                &mut truncated_bytes,
                &mut retained_bytes,
                &mut cell_truncated,
                keep_row,
            );
            total_rows += 1;
            if keep_row
                && retained_bytes.saturating_add(std::mem::size_of::<Vec<String>>())
                    <= MAX_TABULAR_RETAINED_BYTES
            {
                retained_bytes += std::mem::size_of::<Vec<String>>();
                column_count = column_count.max(row.len());
                all_rows.push(std::mem::take(&mut row));
            } else {
                if keep_row {
                    truncated_bytes = true;
                } else {
                    truncated_rows = true;
                }
                row.clear();
            }
            row_col_count = 0;
            field_start = true;
        } else {
            field_start = false;
            if keep_row {
                push_bounded_cell(&mut cell, ch, &mut cell_truncated);
            }
        }
    }

    if !field_start || !row.is_empty() || !cell.is_empty() {
        let keep_row = all_rows.len() <= MAX_TABULAR_ROWS && !truncated_bytes;
        flush_cell(
            &mut row,
            &mut cell,
            &mut row_col_count,
            &mut truncated_columns,
            &mut truncated_cells,
            &mut truncated_bytes,
            &mut retained_bytes,
            &mut cell_truncated,
            keep_row,
        );
        total_rows += 1;
        if keep_row
            && retained_bytes.saturating_add(std::mem::size_of::<Vec<String>>())
                <= MAX_TABULAR_RETAINED_BYTES
        {
            column_count = column_count.max(row.len());
            all_rows.push(row);
        } else if keep_row {
            truncated_bytes = true;
        } else {
            truncated_rows = true;
        }
    }

    if all_rows.is_empty() || column_count == 0 {
        return None;
    }

    let headers = all_rows.remove(0);
    let data_total = total_rows.saturating_sub(1);
    Some(TabularPreview {
        delimiter,
        headers,
        rows: all_rows,
        column_count,
        total_rows: data_total,
        truncated_rows,
        truncated_columns,
        truncated_cells,
        truncated_bytes,
    })
}

fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{061C}'
    )
}

fn push_bounded_cell(cell: &mut String, ch: char, truncated: &mut bool) {
    if is_bidi_control(ch) || ch == '\0' {
        return;
    }
    let normalized = match ch {
        '\r' | '\n' => ' ',
        '\t' => ' ',
        other if other.is_control() => return,
        other => other,
    };
    if cell.len() + normalized.len_utf8() <= MAX_TABULAR_CELL_BYTES {
        cell.push(normalized);
    } else {
        *truncated = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc4180_csv_with_quotes_and_embedded_newlines() {
        let csv =
            "name,role,notes\r\n\"Ada, L.\",Engineer,\"line 1\n\"\"quoted\"\"\"\r\nBob,PM,plain\n";
        let preview = parse_tabular(csv, Path::new("team.csv")).expect("csv preview");
        assert_eq!(preview.delimiter, ',');
        assert_eq!(preview.headers, vec!["name", "role", "notes"]);
        assert_eq!(preview.rows.len(), 2);
        assert_eq!(
            preview.rows[0],
            vec!["Ada, L.", "Engineer", "line 1 \"quoted\""]
        );
        assert_eq!(preview.rows[1], vec!["Bob", "PM", "plain"]);
        assert!(!preview.is_truncated());
    }

    #[test]
    fn parses_tsv_and_strips_bidi_overrides() {
        let tsv = "col1\tcol2\n\u{202E}abc\t123\n";
        let preview = parse_tabular(tsv, Path::new("metrics.TSV")).expect("tsv preview");
        assert_eq!(preview.delimiter, '\t');
        assert_eq!(preview.rows[0], vec!["abc", "123"]);
    }

    #[test]
    fn enforces_column_and_row_bounds() {
        let wide_header = (0..140)
            .map(|i| format!("c{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let wide = format!(
            "{wide_header}\n{}\n",
            (0..140)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let preview = parse_tabular(&wide, Path::new("wide.csv")).expect("wide csv");
        assert_eq!(preview.column_count, MAX_TABULAR_COLUMNS);
        assert!(preview.truncated_columns);
    }

    #[test]
    fn empty_cells_respect_retained_byte_budget_including_struct_overhead() {
        let row = ",".repeat(127);
        let csv = std::iter::repeat_n(row.as_str(), 4_000)
            .collect::<Vec<_>>()
            .join("\n");
        let preview = parse_tabular(&csv, Path::new("sparse.csv")).expect("sparse csv");
        assert!(preview.truncated_bytes || preview.retained_bytes() <= MAX_TABULAR_RETAINED_BYTES);
        assert!(
            preview.retained_bytes() <= MAX_TABULAR_RETAINED_BYTES + 4096,
            "retained bytes {} exceeded cap",
            preview.retained_bytes()
        );
    }
}
