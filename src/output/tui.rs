use crate::error::{ConcertoError, Result};
use crate::install_event::{InstallEvent, InstallEventKind, InstallSummary};
use crate::output::{
    event_message, package_count, recv_finished, summary_message, worker_disconnected,
};
use ratatui::crossterm::terminal::disable_raw_mode;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::{TerminalOptions, Viewport};
use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::Duration;

const MAX_RENDERED_EVENTS: usize = 4;
const SUMMARY_BODY_LINES: u16 = 4;
const SUMMARY_HEIGHT: u16 = SUMMARY_BODY_LINES + 2;
const INLINE_VIEWPORT_HEIGHT: u16 = SUMMARY_HEIGHT + 1;
const SUMMARY_WIDTH: u16 = 72;

struct TuiApp {
    events: VecDeque<String>,
    status: String,
    reused_sources: usize,
    prepared_sources: usize,
    linked_packages: usize,
    autoload_packages: Option<usize>,
    lockfile_written: bool,
    summary: Option<InstallSummary>,
}

pub(super) fn start() -> Result<ratatui::DefaultTerminal> {
    let options = TerminalOptions {
        viewport: Viewport::Inline(INLINE_VIEWPORT_HEIGHT),
    };

    ratatui::try_init_with_options(options)
        .map_err(|error| ConcertoError::internal(format!("Could not start terminal UI: {error}")))
}

pub(super) fn run(
    mut terminal: ratatui::DefaultTerminal,
    events: mpsc::Receiver<InstallEvent>,
    result: mpsc::Receiver<Result<InstallSummary>>,
) -> Result<()> {
    let outcome = run_loop(&mut terminal, events, result);
    let cursor_result = terminal
        .show_cursor()
        .map_err(|error| ConcertoError::internal(format!("Could not restore cursor: {error}")));
    let restore_result = restore_inline_terminal();

    restore_result?;
    cursor_result?;

    let outcome = outcome?;

    outcome.install_result.map(|_| ())
}

fn restore_inline_terminal() -> Result<()> {
    disable_raw_mode()
        .map_err(|error| ConcertoError::internal(format!("Could not restore terminal: {error}")))
}

struct TuiRunOutcome {
    install_result: Result<InstallSummary>,
}

fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    events: mpsc::Receiver<InstallEvent>,
    result: mpsc::Receiver<Result<InstallSummary>>,
) -> Result<TuiRunOutcome> {
    let mut app = TuiApp {
        events: VecDeque::new(),
        status: "Installing".to_string(),
        reused_sources: 0,
        prepared_sources: 0,
        linked_packages: 0,
        autoload_packages: None,
        lockfile_written: false,
        summary: None,
    };

    loop {
        drain_events(&mut app, &events);

        if let Some(result) = recv_finished(&result).map_err(|_| worker_disconnected())? {
            drain_remaining_events(&mut app, &events);
            if let Ok(summary) = result.as_ref() {
                app.status = summary_message(*summary);
                app.summary = Some(*summary);
                draw_app(terminal, &app)?;
            } else {
                app.status = "Install failed".to_string();
                draw_app(terminal, &app)?;
            }
            return Ok(TuiRunOutcome {
                install_result: result,
            });
        }

        draw_app(terminal, &app)?;

        std::thread::sleep(Duration::from_millis(50));
    }
}

fn drain_events(app: &mut TuiApp, events: &mpsc::Receiver<InstallEvent>) {
    while let Ok(event) = events.try_recv() {
        record_event(app, &event);
    }
}

fn drain_remaining_events(app: &mut TuiApp, events: &mpsc::Receiver<InstallEvent>) {
    while let Ok(event) = events.recv() {
        record_event(app, &event);
    }
}

fn record_event(app: &mut TuiApp, event: &InstallEvent) {
    update_stats(app, event);
    let message = event_message(event);

    app.status = message.clone();
    app.events.push_back(message);

    while app.events.len() > MAX_RENDERED_EVENTS {
        app.events.pop_front();
    }
}

fn update_stats(app: &mut TuiApp, event: &InstallEvent) {
    match event.kind {
        InstallEventKind::SourceReused { .. } => app.reused_sources += 1,
        InstallEventKind::SourcePrepared { .. } => app.prepared_sources += 1,
        InstallEventKind::VendorLinked { .. } => app.linked_packages += 1,
        InstallEventKind::AutoloadWritten { packages } => app.autoload_packages = Some(packages),
        InstallEventKind::LockfileWritten => app.lockfile_written = true,
        _ => {}
    }
}

fn draw_app(terminal: &mut ratatui::DefaultTerminal, app: &TuiApp) -> Result<()> {
    terminal
        .draw(|frame| draw(frame, app))
        .map(|_| ())
        .map_err(|error| ConcertoError::internal(format!("Could not draw UI: {error}")))
}

fn draw(frame: &mut ratatui::Frame<'_>, app: &TuiApp) {
    frame.render_widget(Clear, frame.area());

    if app.summary.is_some() {
        draw_summary(frame, app);
        return;
    }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(frame.area());

    let header = Paragraph::new(format!("Concerto install\n{}", app.status)).style(
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(header, layout[0]);

    let items = visible_lines(app)
        .into_iter()
        .map(|line| {
            ListItem::new(format!("  {line}")).style(
                Style::default()
                    .fg(Color::Gray)
                    .remove_modifier(Modifier::BOLD),
            )
        })
        .collect::<Vec<_>>();
    let events = List::new(items).style(
        Style::default()
            .fg(Color::Gray)
            .remove_modifier(Modifier::BOLD),
    );

    frame.render_widget(events, layout[1]);
}

fn draw_summary(frame: &mut ratatui::Frame<'_>, app: &TuiApp) {
    let Some(summary) = app.summary else {
        return;
    };
    let area = top_left_area(frame.area(), SUMMARY_WIDTH, SUMMARY_HEIGHT);

    frame.render_widget(summary_widget(app, summary), area);
    frame.set_cursor_position((0, area.bottom()));
}

fn summary_widget(app: &TuiApp, summary: InstallSummary) -> Paragraph<'static> {
    Paragraph::new(summary_lines(app, summary)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" ♪ ♫ concerto ♫ ♪ "),
    )
}

fn top_left_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);

    Rect {
        x: area.x,
        y: area.y,
        width,
        height,
    }
}

fn summary_line(parts: Vec<Span<'static>>) -> Line<'static> {
    Line::from(parts)
}

fn visible_lines(app: &TuiApp) -> Vec<String> {
    app.events.iter().cloned().collect()
}

fn summary_lines(app: &TuiApp, summary: InstallSummary) -> Vec<Line<'static>> {
    vec![
        summary_line(vec![label("status   "), success(summary_message(summary))]),
        summary_line(vec![
            label("packages "),
            value(package_count(summary.packages)),
            muted("  linked "),
            value(app.linked_packages.to_string()),
            muted("  autoload "),
            value(autoload_count(app)),
        ]),
        summary_line(vec![
            label("store    "),
            value(format!("{}% hit", store_hit_rate(app, summary))),
            muted("  reused "),
            value(app.reused_sources.to_string()),
            muted("  prepared "),
            value(app.prepared_sources.to_string()),
        ]),
        summary_line(vec![
            label("lockfile "),
            value(lockfile_status(app).to_string()),
        ]),
    ]
}

fn success(text: String) -> Span<'static> {
    Span::styled(text, Style::default().fg(Color::Green))
}

fn label(text: &'static str) -> Span<'static> {
    Span::styled(text, Style::default().fg(Color::Gray))
}

fn muted(text: &'static str) -> Span<'static> {
    Span::styled(text, Style::default().fg(Color::DarkGray))
}

fn value(text: String) -> Span<'static> {
    Span::styled(text, Style::default().fg(Color::White))
}

fn autoload_count(app: &TuiApp) -> String {
    app.autoload_packages
        .map(package_count)
        .unwrap_or_else(|| "not written".to_string())
}

fn store_hit_rate(app: &TuiApp, summary: InstallSummary) -> usize {
    if summary.packages == 0 {
        return 0;
    }

    app.reused_sources * 100 / summary.packages
}

fn lockfile_status(app: &TuiApp) -> &'static str {
    if app.lockfile_written {
        "updated"
    } else {
        "unchanged"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn app() -> TuiApp {
        TuiApp {
            events: VecDeque::new(),
            status: "Installing".to_string(),
            reused_sources: 0,
            prepared_sources: 0,
            linked_packages: 0,
            autoload_packages: None,
            lockfile_written: false,
            summary: None,
        }
    }

    #[test]
    fn records_final_summary_stats() {
        let mut app = app();

        record_event(
            &mut app,
            &InstallEvent {
                kind: InstallEventKind::SourceReused {
                    package: "psr/log".to_string(),
                    path: "/store/psr/log".to_string(),
                },
            },
        );
        record_event(
            &mut app,
            &InstallEvent {
                kind: InstallEventKind::VendorLinked {
                    package: "psr/log".to_string(),
                    version: "3.0.2".to_string(),
                    path: "/vendor/psr/log".to_string(),
                },
            },
        );
        record_event(
            &mut app,
            &InstallEvent {
                kind: InstallEventKind::AutoloadWritten { packages: 1 },
            },
        );
        record_event(
            &mut app,
            &InstallEvent {
                kind: InstallEventKind::LockfileWritten,
            },
        );
        app.summary = Some(InstallSummary {
            packages: 1,
            duration: Duration::from_millis(42),
        });

        let summary = app.summary.unwrap();

        assert_eq!(
            SUMMARY_HEIGHT,
            summary_lines(&app, summary).len() as u16 + 2
        );
        assert_eq!(autoload_count(&app), "1 package");
        assert_eq!(store_hit_rate(&app, summary), 100);
        assert_eq!(lockfile_status(&app), "updated");
    }

    #[test]
    fn shows_recent_events_before_summary() {
        let mut app = app();

        for index in 0..10 {
            record_event(
                &mut app,
                &InstallEvent {
                    kind: InstallEventKind::MetadataFetched {
                        package: format!("package/{index}"),
                        bytes: index,
                    },
                },
            );
        }

        let lines = visible_lines(&app);

        assert_eq!(lines.len(), MAX_RENDERED_EVENTS);
        assert_eq!(lines[0], "package/6: fetched 6 bytes");
        assert_eq!(lines[3], "package/9: fetched 9 bytes");
    }

    #[test]
    fn drains_remaining_events_before_summary() {
        let mut app = app();
        let (sender, receiver) = mpsc::channel();

        sender
            .send(InstallEvent {
                kind: InstallEventKind::SourcePrepared {
                    package: "psr/log".to_string(),
                    path: "/store/psr/log".to_string(),
                },
            })
            .unwrap();
        drop(sender);

        drain_remaining_events(&mut app, &receiver);
        app.summary = Some(InstallSummary {
            packages: 1,
            duration: Duration::from_millis(99),
        });

        let summary = app.summary.unwrap();

        assert_eq!(
            summary_lines(&app, summary).len(),
            SUMMARY_BODY_LINES as usize
        );
        assert_eq!(autoload_count(&app), "not written");
        assert_eq!(store_hit_rate(&app, summary), 0);
        assert_eq!(lockfile_status(&app), "unchanged");
    }

    #[test]
    fn renders_summary_bottom_border() {
        let mut app = app();
        app.summary = Some(InstallSummary {
            packages: 1,
            duration: Duration::from_millis(42),
        });

        let backend = TestBackend::new(SUMMARY_WIDTH, SUMMARY_HEIGHT);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| draw(frame, &app)).unwrap();

        let bottom_y = SUMMARY_HEIGHT - 1;
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer.cell((0, bottom_y)).unwrap().symbol(), "└");
        assert_eq!(
            buffer.cell((SUMMARY_WIDTH - 1, bottom_y)).unwrap().symbol(),
            "┘"
        );
    }
}
