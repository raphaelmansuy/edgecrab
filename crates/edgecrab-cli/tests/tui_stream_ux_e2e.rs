//! Semantic TUI stream snapshot (no ANSI and no wall-clock assertions).

#[allow(unused_imports)]
#[path = "../src/presentation/mod.rs"]
mod presentation;
#[path = "../src/stream_presentation.rs"]
mod stream_presentation;

use presentation::{CardStatus, RenderEntry, RenderOpts, ToolEntryArgs, render_entry_lines};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::widgets::Paragraph;
use stream_presentation::{DisplayMode, StreamPresentation};

fn semantic_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let width = terminal.backend().buffer().area.width as usize;
    terminal
        .backend()
        .buffer()
        .content()
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .filter(|row| !row.is_empty())
        .collect()
}

#[test]
fn tui_stream_ux_semantic_rows_snapshot() {
    let mut stream = StreamPresentation::new();
    stream.on_reasoning("Inspect current state.\nChoose the smallest edit.");
    let thought = stream
        .on_tool_exec("edit-1".into(), "write_file".into())
        .expect("tool transition finishes reasoning");
    stream.on_tool_progress("edit-1", "+added semantic row\n-removed stale row");
    let tool = stream.on_tool_done("edit-1").expect("finished tool card");
    let tool_body = tool.tail_lines(3);
    stream.record_edit("src/app.rs", 1, 1);
    let _ = stream.on_token();

    let entries = [
        RenderEntry::from_finished_thinking(&thought),
        RenderEntry::tool(ToolEntryArgs {
            name: tool.name,
            kind: tool.kind,
            status: CardStatus::Success,
            caption: "Edited src/app.rs".into(),
            body: tool_body,
            mode: DisplayMode::Truncated,
            duration: None,
            is_error: false,
        }),
        RenderEntry::agent("Done. Tests green.", DisplayMode::Expanded),
        RenderEntry::footer(format!(
            "{} · {}",
            stream.tool_usage_caption().expect("tool usage"),
            stream.edit_ledger.caption().expect("edit ledger")
        )),
    ];

    let opts = RenderOpts {
        width: 64,
        ..RenderOpts::default()
    };
    let lines = entries
        .iter()
        .flat_map(|entry| render_entry_lines(entry, opts))
        .collect::<Vec<_>>();

    let backend = TestBackend::new(64, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(Paragraph::new(lines.clone()), frame.area()))
        .expect("draw semantic entries");

    assert_eq!(
        semantic_rows(&terminal),
        vec![
            "Thought",
            "Edited src/app.rs",
            "+added semantic row",
            "-removed stale row",
            "Done. Tests green.",
            "Edit 1 · files 1  +1 −1",
        ]
    );
}
