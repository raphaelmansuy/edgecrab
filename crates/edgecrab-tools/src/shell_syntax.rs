pub(crate) fn parse_heredoc_marker(opener: &str) -> Option<String> {
    let idx = opener.find("<<")?;
    let mut marker = opener.get(idx + 2..)?.trim_start();
    if let Some(stripped) = marker.strip_prefix('<') {
        // `<<<` is a here-string, not a heredoc block.
        if !stripped.is_empty() {
            return None;
        }
    }
    if let Some(stripped) = marker.strip_prefix('-') {
        marker = stripped.trim_start();
    }
    if marker.is_empty() {
        return None;
    }

    if let Some(quoted) = marker.strip_prefix('\'') {
        return quoted.split('\'').next().map(str::to_string);
    }
    if let Some(quoted) = marker.strip_prefix('"') {
        return quoted.split('"').next().map(str::to_string);
    }

    let end = marker
        .find(|ch: char| ch.is_whitespace() || matches!(ch, '|' | ';'))
        .unwrap_or(marker.len());
    let marker = marker[..end].trim();
    (!marker.is_empty()).then(|| marker.to_string())
}

pub(crate) fn command_contains_heredoc(command: &str) -> bool {
    let Some(opener) = command.lines().next().map(str::trim) else {
        return false;
    };
    let Some(marker) = parse_heredoc_marker(opener) else {
        return false;
    };

    let allows_tab_indented_terminator = opener.contains("<<-");
    command.lines().skip(1).any(|line| {
        let terminator = if allows_tab_indented_terminator {
            line.trim_start_matches('\t')
        } else {
            line
        };
        terminator == marker
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_basic_heredoc() {
        assert!(command_contains_heredoc("cat <<'EOF'\nhello\nEOF"));
    }

    #[test]
    fn ignores_here_strings() {
        assert!(!command_contains_heredoc("grep foo <<< \"$bar\""));
    }

    #[test]
    fn parses_tab_indented_terminator() {
        assert!(command_contains_heredoc("cat <<-EOF\n\thello\n\tEOF"));
    }
}
