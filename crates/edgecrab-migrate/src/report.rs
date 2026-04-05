//! # Migration report — structured output from migration operations
//!
//! WHY a report: Migrations touch multiple files and can partially
//! succeed. A structured report lets the CLI display exactly what
//! was migrated, skipped, or failed.

use std::fmt;

/// Status of a single migration item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStatus {
    Success,
    Skipped,
    Failed,
}

/// One migration step result.
#[derive(Debug, Clone)]
pub struct MigrationItem {
    pub name: String,
    pub status: MigrationStatus,
    pub detail: String,
}

impl MigrationItem {
    pub fn new(name: &str, status: MigrationStatus, detail: &str) -> Self {
        Self {
            name: name.to_string(),
            status,
            detail: detail.to_string(),
        }
    }

    pub fn success(name: &str) -> Self {
        Self::new(name, MigrationStatus::Success, "ok")
    }

    pub fn skipped(name: &str, reason: &str) -> Self {
        Self::new(name, MigrationStatus::Skipped, reason)
    }

    pub fn failed(name: &str, reason: &str) -> Self {
        Self::new(name, MigrationStatus::Failed, reason)
    }
}

/// Summary of all migration steps.
#[derive(Debug)]
pub struct MigrationReport {
    pub source: String,
    pub items: Vec<MigrationItem>,
}

impl MigrationReport {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
            items: Vec::new(),
        }
    }

    pub fn add(&mut self, item: MigrationItem) {
        self.items.push(item);
    }

    pub fn success_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == MigrationStatus::Success)
            .count()
    }

    pub fn skipped_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == MigrationStatus::Skipped)
            .count()
    }

    pub fn failed_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.status == MigrationStatus::Failed)
            .count()
    }

    pub fn has_failures(&self) -> bool {
        self.failed_count() > 0
    }
}

impl fmt::Display for MigrationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Migration: {}", self.source)?;
        writeln!(f, "{}", "─".repeat(40))?;
        for item in &self.items {
            let icon = match item.status {
                MigrationStatus::Success => "✓",
                MigrationStatus::Skipped => "○",
                MigrationStatus::Failed => "✗",
            };
            writeln!(f, "  {icon} {:<12} {}", item.name, item.detail)?;
        }
        writeln!(f, "{}", "─".repeat(40))?;
        writeln!(
            f,
            "  {} migrated, {} skipped, {} failed",
            self.success_count(),
            self.skipped_count(),
            self.failed_count()
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_counts() {
        let mut report = MigrationReport::new("test");
        report.add(MigrationItem::success("config"));
        report.add(MigrationItem::skipped("memory", "not found"));
        report.add(MigrationItem::failed("skills", "permission denied"));

        assert_eq!(report.success_count(), 1);
        assert_eq!(report.skipped_count(), 1);
        assert_eq!(report.failed_count(), 1);
        assert!(report.has_failures());
    }

    #[test]
    fn report_display() {
        let mut report = MigrationReport::new("hermes → edgecrab");
        report.add(MigrationItem::success("config"));
        report.add(MigrationItem::skipped("env", "already exists"));

        let output = report.to_string();
        assert!(output.contains("hermes → edgecrab"));
        assert!(output.contains("✓"));
        assert!(output.contains("○"));
    }

    #[test]
    fn no_failures() {
        let mut report = MigrationReport::new("test");
        report.add(MigrationItem::success("a"));
        report.add(MigrationItem::skipped("b", "n/a"));
        assert!(!report.has_failures());
    }
}
