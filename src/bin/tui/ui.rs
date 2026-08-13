use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use super::app::{ActivePanel, App, ExecutionMode, Screen};

const TITLE: &str = " R E P L A Y   O S ";
const SUBTITLE: &str = "Deterministic AI Kernel · v0.4.6";

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(size);

    draw_header(f, chunks[0], app);

    match app.current_screen {
        Screen::Dashboard => draw_dashboard(f, chunks[1], app),
        Screen::RunTask => draw_run_task(f, chunks[1], app),
        Screen::RecentRuns => draw_recent_runs(f, chunks[1], app),
        Screen::RuntimeDetails => draw_runtime_details(f, chunks[1], app),
        Screen::Tests => draw_tests(f, chunks[1], app),
        Screen::Diagnostics => draw_diagnostics(f, chunks[1], app),
        Screen::Config => draw_config(f, chunks[1], app),
        Screen::WorkflowOps => draw_workflow_ops(f, chunks[1], app),
        Screen::Tools => draw_tools(f, chunks[1], app),
    }

    draw_footer(f, chunks[2], app);

    // Command palette overlay
    if app.palette_open {
        draw_palette(f, app);
    }

    // Tool confirmation dialog overlay
    if app.tool_confirm_pending.is_some() {
        draw_tool_confirmation(f, app);
    }

    // History detail overlay
    if app.history_detail.is_some() {
        draw_history_detail(f, app);
    }

    // Toast layer
    if let Some(ref msg) = app.toast_message {
        draw_toast(f, app, msg);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let header_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let title = Paragraph::new(Line::from(vec![Span::styled(
        TITLE,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )]));
    f.render_widget(title, header_chunks[0]);

    let subtitle = Paragraph::new(Line::from(vec![Span::styled(
        SUBTITLE,
        Style::default().fg(Color::DarkGray),
    )]));
    f.render_widget(subtitle, header_chunks[1]);

    let runtime_status = match app.runtime.server_state {
        super::runtime::ServerState::Online => Span::styled(
            "● ONLINE",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        super::runtime::ServerState::Crashed => Span::styled(
            "✖ CRASHED",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        super::runtime::ServerState::Recovering => Span::styled(
            "↻ RECOVERING",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        super::runtime::ServerState::Offline => Span::styled(
            "○ OFFLINE",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        super::runtime::ServerState::Unknown => {
            Span::styled("? UNKNOWN", Style::default().fg(Color::DarkGray))
        }
    };

    let ownership_badge = match app.runtime.server_ownership {
        super::runtime::ServerOwnership::Managed => {
            Span::styled(" [managed]", Style::default().fg(Color::DarkGray))
        }
        super::runtime::ServerOwnership::External => {
            Span::styled(" [external]", Style::default().fg(Color::DarkGray))
        }
        super::runtime::ServerOwnership::Unknown => Span::raw(""),
    };

    let recovery_info = if app.recovery_in_progress {
        Span::styled(
            "  ↻ Recovery in progress...",
            Style::default().fg(Color::Yellow),
        )
    } else if app.runtime.recovery_attempts > 0 {
        Span::styled(
            format!("  ({} recovery attempt(s))", app.runtime.recovery_attempts),
            Style::default().fg(Color::DarkGray),
        )
    } else {
        Span::raw("")
    };

    let status = Paragraph::new(Line::from(vec![
        Span::raw("Runtime: "),
        runtime_status,
        ownership_badge,
        Span::raw("  Model: "),
        Span::styled(&app.runtime.model_id, Style::default().fg(Color::Cyan)),
        Span::raw("  History: "),
        Span::styled(
            app.history.len().to_string(),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw(" runs"),
        recovery_info,
    ]));
    f.render_widget(status, header_chunks[2]);

    let separator = Paragraph::new(Line::from(vec![Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(Color::DarkGray),
    )]));
    f.render_widget(separator, header_chunks[3]);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    if app.task_running || app.test_running {
        let footer = Paragraph::new(Line::from(vec![
            Span::styled("  ⏳ Running... ", Style::default().fg(Color::Yellow)),
            Span::styled("Press Esc to cancel", Style::default().fg(Color::DarkGray)),
        ]));
        f.render_widget(footer, area);
        return;
    }

    if let Some(ref msg) = app.status_message {
        let color = if app.status_is_error {
            Color::Red
        } else {
            Color::Green
        };
        let prefix = if app.status_is_error { "✗ " } else { "✓ " };
        let footer = Paragraph::new(Line::from(vec![Span::styled(
            format!("  {}{}", prefix, msg),
            Style::default().fg(color),
        )]));
        f.render_widget(footer, area);
        return;
    }

    let scroll_hint = if app.active_panel == ActivePanel::Output && !app.output_lines.is_empty() {
        if app.follow_output {
            " [FOLLOWING]"
        } else {
            " [SCROLLED UP]"
        }
    } else {
        ""
    };

    let hints = match app.current_screen {
        Screen::Dashboard => ": Palette · ↑↓/jk Navigate · Enter Select · q Quit",
        Screen::RunTask => {
            if app.active_panel == ActivePanel::Output {
                "Tab Input · ↑↓/jk Scroll · PgUp/PgDn · g/G Top/Bot · Esc Back"
            } else if !app.runtime.is_running
                && app.execution_mode == super::app::ExecutionMode::Verified
            {
                "Tab Output · R Restart Server · m Switch to Plan-only · Esc Back"
            } else {
                "Tab Output · Enter Run · m Mode · Esc Back · ←→ Cursor"
            }
        }
        Screen::RecentRuns => {
            if app.active_panel == ActivePanel::Search {
                "Type to filter · Esc Clear"
            } else {
                ": Palette · ↑↓/jk Nav · Enter Details · d Delete · / Search · r Rerun · Esc Back"
            }
        }
        Screen::RuntimeDetails => {
            if app.tail_mode {
                "t Stop tail · ↑↓/jk Scroll · PgUp/PgDn · g/G Top/Bot · Esc Back"
            } else {
                ": Palette · r Refresh · R Restart · t Tail · Esc Back"
            }
        }
        Screen::Tests => ": Palette · 1 Quick · 2 Full · Esc Back",
        Screen::Diagnostics => ": Palette · 1 Doctor · 2 JSON · Esc Back",
        Screen::Config => "Esc Back",
        Screen::Tools => ": Palette · ↑↓/jk Select · t Test · Esc Back",
        Screen::WorkflowOps => {
            if app.confirm_action.is_some() && app.active_panel == ActivePanel::Confirm {
                "y Confirm · n Cancel"
            } else if app.workflow_input_mode {
                "Enter Confirm · Esc Cancel"
            } else {
                "1-4 Select op · Esc Back"
            }
        }
    };

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(hints, Style::default().fg(Color::DarkGray)),
        Span::styled(scroll_hint, Style::default().fg(Color::Yellow)),
    ]));
    f.render_widget(footer, area);
}

fn draw_dashboard(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(25), Constraint::Min(0)])
        .split(area);

    // Left: navigation
    let nav_items: Vec<ListItem> = app
        .nav_items()
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == app.nav_index && app.active_panel == ActivePanel::LeftNav {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(*item, style)))
        })
        .collect();

    let nav_list = List::new(nav_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Menu "),
    );
    f.render_widget(nav_list, chunks[0]);

    // Right: split into status + recent runs
    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(chunks[1]);

    // Status cards
    let runtime_badge = if app.runtime.is_running {
        Span::styled(
            " ● ONLINE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " ○ OFFLINE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )
    };

    let mode_badge = match app.execution_mode {
        ExecutionMode::Verified => Span::styled(
            " VERIFIED ",
            Style::default().fg(Color::Black).bg(Color::Green),
        ),
        ExecutionMode::PlanOnly => Span::styled(
            " PLAN-ONLY ",
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ),
    };

    let tool_count = app.tool_registry.all().len();

    let status_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Runtime ", Style::default().fg(Color::DarkGray)),
            runtime_badge,
            Span::styled("   Mode ", Style::default().fg(Color::DarkGray)),
            mode_badge,
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Model:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.runtime.model_id, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("  History: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} runs", app.history.len()),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled("   Tools: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} available", tool_count),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  ↑↓ Navigate  Enter Select  : Command Palette  q Quit",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let status_widget = Paragraph::new(status_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Status "),
    );
    f.render_widget(status_widget, right_chunks[0]);

    // Recent runs preview
    let recent_lines: Vec<Line> = if app.history.is_empty() {
        vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  No runs yet",
                Style::default().fg(Color::DarkGray),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Press Enter on Run Task to get started",
                Style::default().fg(Color::DarkGray),
            )]),
        ]
    } else {
        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Recent Runs",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
        ];
        for entry in app.history.iter().take(5) {
            let icon = if entry.ok { "✓" } else { "✗" };
            let icon_color = if entry.ok { Color::Green } else { Color::Red };
            let task_preview = if entry.task.len() > 40 {
                format!("{}...", &entry.task[..40])
            } else {
                entry.task.clone()
            };
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", icon), Style::default().fg(icon_color)),
                Span::styled(&entry.timestamp, Style::default().fg(Color::DarkGray)),
                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(task_preview, Style::default().fg(Color::White)),
            ]));
        }
        if app.history.len() > 5 {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![Span::styled(
                format!("  ... and {} more", app.history.len() - 5),
                Style::default().fg(Color::DarkGray),
            )]));
        }
        lines
    };

    let recent_widget = Paragraph::new(recent_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(" Recent Runs ({}) ", app.history.len())),
    );
    f.render_widget(recent_widget, right_chunks[1]);
}

fn draw_run_task(f: &mut Frame, area: Rect, app: &App) {
    // If we have a final answer, show result layout; otherwise normal layout
    if !app.last_final_answer.is_empty() && !app.task_running {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Length(8),
                Constraint::Min(0),
            ])
            .split(area);

        draw_run_task_header(f, chunks[0], app);
        draw_run_task_summary(f, chunks[1], app);
        draw_final_answer_panel(f, chunks[2], app);
        draw_output_panel(f, chunks[3], app);
        return;
    }

    let has_activity = !app.tool_activity.is_empty();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(7),
            if has_activity {
                Constraint::Length(5)
            } else {
                Constraint::Length(0)
            },
            Constraint::Min(0),
        ])
        .split(area);

    // Input
    draw_run_task_header(f, chunks[0], app);

    // Summary + mode badge + runtime warning
    draw_run_task_summary(f, chunks[1], app);

    // Live tool activity
    if has_activity {
        draw_tool_activity(f, chunks[2], app);
    }

    // Output with scroll
    let output_idx = if has_activity { 3 } else { 2 };
    draw_output_panel(f, chunks[output_idx], app);
}

fn draw_tool_activity(f: &mut Frame, area: Rect, app: &App) {
    let lines: Vec<Line> = app
        .tool_activity
        .iter()
        .rev()
        .take(3)
        .map(|entry| {
            let (status_icon, status_color) = match entry.status.as_str() {
                "running" => ("⏳", Color::Yellow),
                "success" => ("✓", Color::Green),
                "error" => ("✗", Color::Red),
                "awaiting_confirmation" => ("?", Color::Yellow),
                "cancelled" => ("—", Color::DarkGray),
                _ => ("·", Color::DarkGray),
            };
            let duration = if entry.duration_ms > 0 {
                format!("{}ms", entry.duration_ms)
            } else {
                String::new()
            };
            Line::from(vec![
                Span::styled(
                    format!(" {status_icon} "),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    &entry.tool_name,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled(&entry.status, Style::default().fg(status_color)),
                Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                Span::styled(&entry.args_preview, Style::default().fg(Color::White)),
                if !duration.is_empty() {
                    Span::styled(
                        format!(" · {duration}"),
                        Style::default().fg(Color::DarkGray),
                    )
                } else {
                    Span::raw("")
                },
                if !entry.result_preview.is_empty() {
                    Span::styled(
                        format!(
                            " · {}",
                            &entry.result_preview[..entry.result_preview.len().min(40)]
                        ),
                        Style::default().fg(Color::DarkGray),
                    )
                } else {
                    Span::raw("")
                },
            ])
        })
        .collect();

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(format!(" Tool Activity ({}) ", app.tool_activity.len())),
    );
    f.render_widget(widget, area);
}

fn draw_run_task_header(f: &mut Frame, area: Rect, app: &App) {
    let input_display = if app.task_running {
        format!("{}⏳", &app.task_input)
    } else {
        format!("{}█", &app.task_input)
    };

    let input_style = if app.task_running {
        Style::default().fg(Color::Yellow)
    } else if app.active_panel == ActivePanel::Input {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default()
    };

    let input_title = if app.task_running {
        let elapsed = app.start_time.map(|t| t.elapsed().as_millis()).unwrap_or(0);
        format!(
            " ⏳ Running [{}] ({}ms) ",
            app.execution_mode.label(),
            elapsed
        )
    } else {
        format!(" Task Input ({}) ", app.task_input_cursor)
    };

    let input = Paragraph::new(Line::from(vec![Span::styled(&input_display, input_style)])).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(input_style)
            .title(input_title),
    );
    f.render_widget(input, area);
}

fn draw_run_task_summary(f: &mut Frame, area: Rect, app: &App) {
    let mut summary = vec![
        Line::from(vec![
            Span::styled("  Mode: ", Style::default().fg(Color::DarkGray)),
            match app.execution_mode {
                ExecutionMode::Verified => Span::styled(
                    "VERIFIED",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                ExecutionMode::PlanOnly => Span::styled(
                    "PLAN-ONLY",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            },
            Span::styled("  (m to toggle)", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled("  Model: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.runtime.model_id, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("  Runtime: ", Style::default().fg(Color::DarkGray)),
            if app.runtime.is_running {
                Span::styled("Online", Style::default().fg(Color::Green))
            } else {
                Span::styled("Offline", Style::default().fg(Color::Red))
            },
        ]),
    ];

    if !app.runtime.is_running && app.execution_mode == ExecutionMode::Verified {
        summary.push(Line::from(""));
        summary.push(Line::from(vec![Span::styled(
            "  ⚠ MLX server offline — verified mode requires a running LLM",
            Style::default().fg(Color::Red),
        )]));
        summary.push(Line::from(vec![Span::styled(
            "    Start server or press m to switch to plan-only",
            Style::default().fg(Color::DarkGray),
        )]));
    }

    let summary_para = Paragraph::new(summary).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Summary "),
    );
    f.render_widget(summary_para, area);
}

fn draw_final_answer_panel(f: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Answer",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    // Word-wrap the final answer to fit the panel width
    let max_width = area.width.saturating_sub(4) as usize;
    let answer = &app.last_final_answer;
    if answer.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no answer)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for wrapped_line in wrap_text(answer, max_width) {
            lines.push(Line::from(Span::styled(
                format!("  {}", wrapped_line),
                Style::default(),
            )));
        }
    }

    // Critique status line
    if !app.last_critique_status.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Critique: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.last_critique_status, Style::default().fg(Color::Green)),
        ]));
    }

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Result "),
    );
    f.render_widget(widget, area);
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= max_width {
                current.push(' ');
                current.push_str(word);
            } else {
                result.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    result
}

fn draw_output_panel(f: &mut Frame, area: Rect, app: &App) {
    let visible_height = area.height as usize;
    let total_lines = app.output_lines.len();

    let start = if total_lines > visible_height {
        app.output_scroll.saturating_sub(visible_height)
    } else {
        0
    };
    let end = app.output_scroll.min(total_lines);

    let output_lines: Vec<Line> = app
        .output_lines
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|l| Line::from(Span::raw(l.as_str())))
        .collect();

    // Title with elapsed time when running
    let output_title = if app.task_running || app.test_running {
        let count = app.output_lines.len();
        let elapsed = app.start_time.map(|t| t.elapsed().as_millis()).unwrap_or(0);
        if elapsed > 1000 {
            format!(" Output ({} lines) ⏳ {}s ", count, elapsed / 1000)
        } else {
            format!(" Output ({} lines) ⏳ ", count)
        }
    } else if !app.output_lines.is_empty() {
        let count = app.output_lines.len();
        let scroll_info = if total_lines > visible_height {
            format!(" {}/{}", end, total_lines)
        } else {
            String::new()
        };
        format!(" Output ({} lines){} ", count, scroll_info)
    } else {
        " Output ".to_string()
    };

    let border_color = if app.active_panel == ActivePanel::Output {
        Color::Cyan
    } else if app.task_running || app.test_running {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let output = if (app.task_running || app.test_running) && output_lines.is_empty() {
        Paragraph::new(Line::from(vec![
            Span::styled("  ⏳ ", Style::default().fg(Color::Yellow)),
            Span::styled("Executing...", Style::default().fg(Color::DarkGray)),
            Span::styled(
                " (press Esc to cancel)",
                Style::default().fg(Color::DarkGray),
            ),
        ]))
    } else if output_lines.is_empty() {
        Paragraph::new(Line::from(Span::styled(
            "  Output will appear here...",
            Style::default().fg(Color::DarkGray),
        )))
    } else {
        // Color-code output lines by type
        let colored_lines: Vec<Line> = app.output_lines[start..end]
            .iter()
            .map(|l| {
                let style = if l.starts_with("observability:") {
                    Style::default().fg(Color::DarkGray)
                } else if l.starts_with("[tool]") {
                    if l.contains("✓") {
                        Style::default().fg(Color::Green)
                    } else if l.contains("✗") {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::Yellow)
                    }
                } else if l.starts_with("[mode:") {
                    Style::default().fg(Color::Cyan)
                } else if l.starts_with('$') || l.starts_with('>') {
                    Style::default().fg(Color::DarkGray)
                } else if l.starts_with('{') || l.starts_with('[') {
                    Style::default().fg(Color::Yellow)
                } else if l.contains("error") || l.contains("Error") || l.contains("FAILED") {
                    Style::default().fg(Color::Red)
                } else if l.contains("success") || l.contains("Success") || l.contains("passed") {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(l.as_str(), style))
            })
            .collect();

        // Add blinking cursor line when actively streaming
        if app.task_running || app.test_running {
            let mut lines_with_cursor = colored_lines;
            lines_with_cursor.push(Line::from(vec![Span::styled(
                "  ▸ ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
            Paragraph::new(lines_with_cursor)
        } else {
            Paragraph::new(colored_lines)
        }
    };

    let output_widget = output.block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(output_title),
    );
    f.render_widget(output_widget, area);
}

fn draw_recent_runs(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // Left: search + filters + list
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(chunks[0]);

    // Search bar
    let search_display = if app.active_panel == ActivePanel::Search {
        format!("🔍 {}█", app.search_query)
    } else if app.search_query.is_empty() {
        "🔍  Press / to search".to_string()
    } else {
        format!(
            "🔍 {} ({} matches)",
            app.search_query,
            app.filtered_history.len()
        )
    };

    let search_style = if app.active_panel == ActivePanel::Search {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let search = Paragraph::new(Line::from(vec![Span::styled(
        &search_display,
        search_style,
    )]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(search_style)
            .title(" Search "),
    );
    f.render_widget(search, left_chunks[0]);

    // Filter bar
    let type_label = match &app.history_filter_type {
        super::app::HistoryFilterType::All => "all",
        super::app::HistoryFilterType::TaskRun => "tasks",
        super::app::HistoryFilterType::ToolInvocation => "tools",
    };
    let status_label = match &app.history_filter_status {
        super::app::HistoryFilterStatus::All => "all",
        super::app::HistoryFilterStatus::Success => "ok",
        super::app::HistoryFilterStatus::Error => "err",
        super::app::HistoryFilterStatus::Cancelled => "cancelled",
        super::app::HistoryFilterStatus::Denied => "denied",
    };
    let has_filters = app.history_filter_type != super::app::HistoryFilterType::All
        || app.history_filter_status != super::app::HistoryFilterStatus::All
        || !app.history_filter_tool.is_empty();

    let filter_color = if has_filters {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let filter_line = Line::from(vec![
        Span::styled(
            " 1:",
            Style::default()
                .fg(filter_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            type_label,
            Style::default().fg(
                if app.history_filter_type != super::app::HistoryFilterType::All {
                    Color::Cyan
                } else {
                    Color::DarkGray
                },
            ),
        ),
        Span::styled(
            "  2:",
            Style::default()
                .fg(filter_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            status_label,
            Style::default().fg(
                if app.history_filter_status != super::app::HistoryFilterStatus::All {
                    Color::Cyan
                } else {
                    Color::DarkGray
                },
            ),
        ),
        Span::styled(
            "  0:reset",
            Style::default().fg(if has_filters {
                Color::Red
            } else {
                Color::DarkGray
            }),
        ),
    ]);

    let filter_widget = Paragraph::new(filter_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(filter_color))
            .title(" Filters "),
    );
    f.render_widget(filter_widget, left_chunks[1]);

    // History list
    if app.filtered_history.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  📭  No matches",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Try a different search query.",
                Style::default().fg(Color::DarkGray),
            )]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(
                    " Recent Runs ({}/{}) ",
                    app.filtered_history.len(),
                    app.history.len()
                )),
        );
        f.render_widget(empty, left_chunks[2]);
        return;
    }

    let items: Vec<ListItem> = app
        .filtered_history
        .iter()
        .enumerate()
        .map(|(display_idx, &history_idx)| {
            let entry = &app.history[history_idx];
            let is_tool = entry.entry_type == "tool_invocation";

            let (icon, icon_color) = if is_tool {
                ("⚙", Color::Yellow)
            } else if entry.ok {
                ("✓", Color::Green)
            } else {
                ("✗", Color::Red)
            };

            let style = if display_idx == app.history_index {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let type_badge = if is_tool {
                Span::styled(
                    " [tool] ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("")
            };

            let task_text = if is_tool && !entry.tool_name.is_empty() {
                format!(
                    "{} → {}",
                    entry.tool_name,
                    entry.task.replace("[tool] ", "")
                )
            } else {
                entry.task.clone()
            };
            let task_preview = if task_text.len() > 35 {
                format!("{}...", &task_text[..35])
            } else {
                task_text
            };

            let line = Line::from(vec![
                Span::styled(format!("{} ", icon), Style::default().fg(icon_color)),
                type_badge,
                Span::styled(&entry.timestamp, Style::default().fg(Color::DarkGray)),
                Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                Span::styled(task_preview, style),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(
                " Recent Runs ({}/{}) ",
                app.filtered_history.len(),
                app.history.len()
            )),
    );

    let mut state = ListState::default();
    state.select(Some(app.history_index));
    f.render_stateful_widget(list, left_chunks[2], &mut state);

    // Right: detail preview
    if let Some(&idx) = app.filtered_history.get(app.history_index) {
        if let Some(entry) = app.history.get(idx) {
            let is_tool = entry.entry_type == "tool_invocation";

            let mut detail = vec![
                Line::from(""),
                Line::from(vec![Span::styled(
                    "  Details",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Type:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&entry.entry_type, Style::default().fg(Color::Yellow)),
                ]),
                Line::from(vec![
                    Span::styled("  Status:   ", Style::default().fg(Color::DarkGray)),
                    if entry.ok {
                        Span::styled("SUCCESS", Style::default().fg(Color::Green))
                    } else {
                        Span::styled("FAILED", Style::default().fg(Color::Red))
                    },
                ]),
                Line::from(vec![
                    Span::styled("  Time:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&entry.timestamp, Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled("  Elapsed:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&entry.elapsed_ms, Style::default().fg(Color::Cyan)),
                    Span::styled("ms", Style::default().fg(Color::DarkGray)),
                ]),
            ];

            if is_tool {
                detail.push(Line::from(vec![
                    Span::styled("  Tool:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        &entry.tool_name,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                detail.push(Line::from(vec![
                    Span::styled("  Confirmed:", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        if entry.confirmed { " yes" } else { " no" },
                        Style::default().fg(if entry.confirmed {
                            Color::Green
                        } else {
                            Color::Yellow
                        }),
                    ),
                ]));
                if let Some(ref err) = entry.error {
                    detail.push(Line::from(vec![
                        Span::styled("  Error:    ", Style::default().fg(Color::DarkGray)),
                        Span::styled(err.as_str(), Style::default().fg(Color::Red)),
                    ]));
                }
            } else {
                if !entry.seed.is_empty() {
                    detail.push(Line::from(vec![
                        Span::styled("  Seed:     ", Style::default().fg(Color::DarkGray)),
                        Span::styled(&entry.seed, Style::default().fg(Color::White)),
                    ]));
                }
                if !entry.plan_id.is_empty() {
                    detail.push(Line::from(vec![
                        Span::styled("  Plan ID:  ", Style::default().fg(Color::DarkGray)),
                        Span::styled(&entry.plan_id, Style::default().fg(Color::Cyan)),
                    ]));
                }
            }

            detail.push(Line::from(""));
            detail.push(Line::from(vec![Span::styled(
                "  Task:",
                Style::default().fg(Color::DarkGray),
            )]));
            detail.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(&entry.task, Style::default()),
            ]));

            if !entry.answer.is_empty() && entry.answer != "n/a" {
                detail.push(Line::from(""));
                detail.push(Line::from(vec![Span::styled(
                    "  Answer:",
                    Style::default().fg(Color::DarkGray),
                )]));
                let preview = if entry.answer.len() > 100 {
                    format!("{}...", &entry.answer[..100])
                } else {
                    entry.answer.clone()
                };
                detail.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(preview, Style::default().fg(Color::Cyan)),
                ]));
            }

            detail.push(Line::from(""));
            detail.push(Line::from(vec![Span::styled(
                "  Enter Details · r Rerun · d Delete",
                Style::default().fg(Color::DarkGray),
            )]));

            let widget = Paragraph::new(detail).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(" Details "),
            );
            f.render_widget(widget, chunks[1]);
        }
    } else {
        let empty = Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Select a run to preview",
                Style::default().fg(Color::DarkGray),
            )]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Details "),
        );
        f.render_widget(empty, chunks[1]);
    }
}

fn draw_runtime_details(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(25), Constraint::Min(0)])
        .split(area);

    // Left: actions
    let tail_status = if app.tail_mode { "ON" } else { "OFF" };
    let tail_color = if app.tail_mode {
        Color::Green
    } else {
        Color::DarkGray
    };

    let actions = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  ⚙️  Actions",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  r",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Refresh", Style::default()),
        ]),
        Line::from(vec![
            Span::styled(
                "  R",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Restart Server", Style::default()),
        ]),
        Line::from(vec![
            Span::styled(
                "  t",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  Tail: {}", tail_status),
                Style::default().fg(tail_color),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Back", Style::default()),
        ]),
    ];

    let actions_widget = Paragraph::new(actions).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Actions "),
    );
    f.render_widget(actions_widget, chunks[0]);

    // Right: details or tail log
    if app.tail_mode {
        // Tail log view
        let visible_height = chunks[1].height as usize;
        let total = app.runtime_log_lines.len();
        let start = if total > visible_height {
            app.runtime_log_scroll.saturating_sub(visible_height)
        } else {
            0
        };
        let end = app.runtime_log_scroll.min(total);

        let log_lines: Vec<Line> = app
            .runtime_log_lines
            .iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|l| Line::from(Span::raw(l.as_str())))
            .collect();

        let log_title = if total > 0 {
            let follow = if app.runtime_log_follow {
                " [FOLLOWING]"
            } else {
                " [SCROLLED]"
            };
            format!(" Runtime Log ({} lines){} ", total, follow)
        } else {
            " Runtime Log (empty) ".to_string()
        };

        let log_widget = if log_lines.is_empty() {
            Paragraph::new(Line::from(Span::styled(
                "  No log lines found. Check /tmp/replay_os_mlx.log",
                Style::default().fg(Color::DarkGray),
            )))
        } else {
            Paragraph::new(log_lines)
        }
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(log_title),
        );
        f.render_widget(log_widget, chunks[1]);
    } else {
        // Details view
        let detail = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Status: ", Style::default().fg(Color::DarkGray)),
                match app.runtime.server_state {
                    super::runtime::ServerState::Online => Span::styled(
                        "ONLINE",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    super::runtime::ServerState::Crashed => Span::styled(
                        "CRASHED",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    super::runtime::ServerState::Recovering => Span::styled(
                        "RECOVERING",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    super::runtime::ServerState::Offline => Span::styled(
                        "OFFLINE",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    super::runtime::ServerState::Unknown => {
                        Span::styled("UNKNOWN", Style::default().fg(Color::DarkGray))
                    }
                },
            ]),
            Line::from(vec![
                Span::styled("  Model ID: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&app.runtime.model_id, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("  Base URL: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&app.runtime.base_url, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("  Ownership: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    app.runtime.server_ownership.label(),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Probes: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!(
                        "{} total, {} failures",
                        app.probe_stats.total_probes, app.probe_stats.failures
                    ),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
            if app.runtime.recovery_attempts > 0 {
                Line::from(vec![
                    Span::styled("  Recovery: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{} attempt(s)", app.runtime.recovery_attempts),
                        Style::default().fg(Color::Yellow),
                    ),
                ])
            } else {
                Line::from("")
            },
            if !app.runtime.last_error_reason.is_empty() {
                Line::from(vec![
                    Span::styled("  Last error: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        &app.runtime.last_error_reason,
                        Style::default().fg(Color::Red),
                    ),
                ])
            } else {
                Line::from("")
            },
            Line::from(""),
            Line::from(vec![Span::styled(
                "  r Refresh  R Restart  t Tail  Esc Back",
                Style::default().fg(Color::DarkGray),
            )]),
        ];

        let detail_widget = Paragraph::new(detail).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Runtime Details "),
        );
        f.render_widget(detail_widget, chunks[1]);
    }
}

fn draw_tests(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(25), Constraint::Min(0)])
        .split(area);

    // Left: actions
    let actions = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  🧪  Tests",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  1",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Quick (lib)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled(
                "  2",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Full suite", Style::default()),
        ]),
    ];

    let actions_widget = Paragraph::new(actions).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Actions "),
    );
    f.render_widget(actions_widget, chunks[0]);

    // Right: output
    draw_output_panel(f, chunks[1], app);
}

fn draw_diagnostics(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(25), Constraint::Min(0)])
        .split(area);

    // Left: actions
    let actions = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  🩺  Diagnostics",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  1",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Doctor (text)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled(
                "  2",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Doctor (JSON)", Style::default()),
        ]),
    ];

    let actions_widget = Paragraph::new(actions).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Actions "),
    );
    f.render_widget(actions_widget, chunks[0]);

    // Right: output
    draw_output_panel(f, chunks[1], app);
}

fn draw_toast(f: &mut Frame, app: &App, msg: &str) {
    let area = f.area();
    let toast_width = (msg.len() as u16 + 4).min(area.width - 4);
    let x = area.width.saturating_sub(toast_width + 1);
    let y = area.height.saturating_sub(3);
    let toast_area = Rect::new(x, y, toast_width, 1);

    let color = if app.toast_is_error {
        Color::Red
    } else {
        Color::Green
    };
    let icon = if app.toast_is_error { "✗" } else { "✓" };

    let toast = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {icon} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(msg, Style::default().fg(Color::White)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color)),
    );
    f.render_widget(toast, toast_area);
}

fn draw_history_detail(f: &mut Frame, app: &App) {
    let area = f.area();
    if let Some(ref entry) = app.history_detail {
        let is_tool = entry.entry_type == "tool_invocation";

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  History Details",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Type:      ", Style::default().fg(Color::DarkGray)),
                Span::styled(&entry.entry_type, Style::default().fg(Color::Yellow)),
            ]),
            Line::from(vec![
                Span::styled("  Timestamp: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&entry.timestamp, Style::default().fg(Color::White)),
            ]),
        ];

        if is_tool {
            lines.push(Line::from(vec![
                Span::styled("  Tool:      ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    &entry.tool_name,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Status:    ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    &entry.task,
                    Style::default().fg(if entry.ok { Color::Green } else { Color::Red }),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Confirmed: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if entry.confirmed { "yes" } else { "no" },
                    Style::default().fg(if entry.confirmed {
                        Color::Green
                    } else {
                        Color::Yellow
                    }),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Duration:  ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}ms", entry.elapsed_ms),
                    Style::default().fg(Color::White),
                ),
            ]));

            // Arguments
            if entry.arguments != serde_json::Value::Null {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "  Arguments:",
                    Style::default().fg(Color::DarkGray),
                )]));
                if let Some(obj) = entry.arguments.as_object() {
                    for (k, v) in obj {
                        let val_str = match v {
                            serde_json::Value::String(s) => {
                                if s.len() > 50 {
                                    format!("{}...", &s[..50])
                                } else {
                                    s.clone()
                                }
                            }
                            other => other.to_string(),
                        };
                        lines.push(Line::from(vec![
                            Span::styled("    ", Style::default()),
                            Span::styled(format!("{k}: "), Style::default().fg(Color::DarkGray)),
                            Span::styled(val_str, Style::default().fg(Color::White)),
                        ]));
                    }
                }
            }

            // Error
            if let Some(ref err) = entry.error {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::styled("  Error:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(err.as_str(), Style::default().fg(Color::Red)),
                ]));
            }

            // Output summary
            if !entry.answer.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "  Output:",
                    Style::default().fg(Color::DarkGray),
                )]));
                for line in entry.answer.lines().take(10) {
                    lines.push(Line::from(vec![
                        Span::styled("    ", Style::default()),
                        Span::styled(line, Style::default().fg(Color::White)),
                    ]));
                }
            }
        } else {
            // Task run entry
            lines.push(Line::from(vec![
                Span::styled("  Task:      ", Style::default().fg(Color::DarkGray)),
                Span::styled(&entry.task, Style::default().fg(Color::White)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Status:    ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    if entry.ok { "SUCCESS" } else { "FAILED" },
                    Style::default().fg(if entry.ok { Color::Green } else { Color::Red }),
                ),
            ]));
            if !entry.seed.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  Seed:      ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&entry.seed, Style::default().fg(Color::White)),
                ]));
            }
            if !entry.plan_id.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("  Plan ID:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&entry.plan_id, Style::default().fg(Color::Cyan)),
                ]));
            }
            lines.push(Line::from(vec![
                Span::styled("  Elapsed:   ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}ms", entry.elapsed_ms),
                    Style::default().fg(Color::White),
                ),
            ]));
            if !entry.answer.is_empty() && entry.answer != "n/a" {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![Span::styled(
                    "  Answer:",
                    Style::default().fg(Color::DarkGray),
                )]));
                for line in entry.answer.lines().take(10) {
                    lines.push(Line::from(vec![
                        Span::styled("    ", Style::default()),
                        Span::styled(line, Style::default().fg(Color::White)),
                    ]));
                }
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "  Esc Back",
            Style::default().fg(Color::DarkGray),
        )]));

        let dialog_height = (lines.len() as u16 + 2).min(area.height - 4);
        let dialog_width = 64.min(area.width - 4);
        let x = (area.width - dialog_width) / 2;
        let y = (area.height - dialog_height) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" History Details "),
        );
        f.render_widget(widget, dialog_area);
    }
}

fn draw_tool_confirmation(f: &mut Frame, app: &App) {
    let area = f.area();

    if let Some(ref req) = app.tool_confirm_pending {
        // Build detail lines based on tool type
        let mut detail_lines = Vec::new();

        // Extract tool-specific details from arguments
        let path = req
            .arguments
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content = req
            .arguments
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let old_str = req
            .arguments
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let new_str = req
            .arguments
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let cmd = req
            .arguments
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let url = req
            .arguments
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match req.tool.as_str() {
            "write_file" => {
                detail_lines.push(Line::from(vec![
                    Span::styled("  Path:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(path, Style::default().fg(Color::Cyan)),
                ]));
                let preview = if content.len() > 40 {
                    format!("{}...", &content[..40])
                } else {
                    content.to_string()
                };
                detail_lines.push(Line::from(vec![
                    Span::styled("  Content:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(preview, Style::default().fg(Color::White)),
                ]));
                detail_lines.push(Line::from(vec![
                    Span::styled("  Size:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{} bytes", content.len()),
                        Style::default().fg(Color::Yellow),
                    ),
                ]));
            }
            "edit_file" => {
                detail_lines.push(Line::from(vec![
                    Span::styled("  Path:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(path, Style::default().fg(Color::Cyan)),
                ]));
                let old_preview = if old_str.len() > 30 {
                    format!("{}...", &old_str[..30])
                } else {
                    old_str.to_string()
                };
                let new_preview = if new_str.len() > 30 {
                    format!("{}...", &new_str[..30])
                } else {
                    new_str.to_string()
                };
                detail_lines.push(Line::from(vec![
                    Span::styled("  Replace:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(old_preview, Style::default().fg(Color::Red)),
                ]));
                detail_lines.push(Line::from(vec![
                    Span::styled("  With:     ", Style::default().fg(Color::DarkGray)),
                    Span::styled(new_preview, Style::default().fg(Color::Green)),
                ]));
            }
            "shell_execute" => {
                detail_lines.push(Line::from(vec![
                    Span::styled("  Command:  ", Style::default().fg(Color::DarkGray)),
                    Span::styled(cmd, Style::default().fg(Color::Yellow)),
                ]));
            }
            "open_url" => {
                detail_lines.push(Line::from(vec![
                    Span::styled("  URL:      ", Style::default().fg(Color::DarkGray)),
                    Span::styled(url, Style::default().fg(Color::Cyan)),
                ]));
            }
            _ => {
                detail_lines.push(Line::from(vec![
                    Span::styled("  Action:   ", Style::default().fg(Color::DarkGray)),
                    Span::styled(&req.description, Style::default().fg(Color::White)),
                ]));
            }
        }

        let mut lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Confirm Tool Execution",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Tool:     ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    &req.tool,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        lines.append(&mut detail_lines);
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                "y",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Approve   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "n",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Deny", Style::default().fg(Color::DarkGray)),
        ]));

        let dialog_height = (lines.len() as u16 + 2).min(area.height - 4);
        let dialog_width = 60.min(area.width - 4);
        let x = (area.width - dialog_width) / 2;
        let y = (area.height - dialog_height) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" ⚠ Confirm "),
        );
        f.render_widget(widget, dialog_area);
    }
}

fn draw_tools(f: &mut Frame, area: Rect, app: &App) {
    use ratatui::widgets::{Cell, Row, Table};

    let tools = app.tool_registry.all();

    let header = Row::new(vec![
        Cell::from("Tool"),
        Cell::from("Category"),
        Cell::from("Safety"),
        Cell::from("Confirm"),
    ])
    .style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = tools
        .iter()
        .enumerate()
        .map(|(i, tool)| {
            let style = if i == app.tool_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let safety_color = match tool.safety {
                deterministic_ai_kernel::tools::ToolSafety::ReadOnly => Color::Green,
                deterministic_ai_kernel::tools::ToolSafety::LocalMutating => Color::Yellow,
                deterministic_ai_kernel::tools::ToolSafety::ExternalMutating => Color::Red,
            };

            Row::new(vec![
                Cell::from(tool.name.to_string()),
                Cell::from(tool.category.to_string()),
                Cell::from(tool.safety.label()).style(Style::default().fg(safety_color)),
                Cell::from(if tool.confirmation_required {
                    "yes"
                } else {
                    "no"
                }),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(10),
            Constraint::Length(18),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(" Tools / Capabilities ({}) ", tools.len())),
    );

    f.render_widget(table, area);
}

fn draw_config(f: &mut Frame, area: Rect, app: &App) {
    let items = app.config_items();

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  📋  Configuration",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    for (label, value, ok) in &items {
        let status_icon = if *ok { "✓" } else { "⚠" };
        let status_color = if *ok { Color::Green } else { Color::Yellow };

        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} ", status_icon),
                Style::default().fg(status_color),
            ),
            Span::styled(
                format!("{:14}", label),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(" ", Style::default()),
            Span::styled(value, Style::default().fg(Color::Cyan)),
        ]));
    }

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Config / Paths "),
    );
    f.render_widget(widget, area);
}

fn draw_workflow_ops(f: &mut Frame, area: Rect, app: &App) {
    // Confirm dialog
    if app.confirm_action.is_some() && app.active_panel == ActivePanel::Confirm {
        let confirm_lines = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  ⚠️  Confirm Action",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Action: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    app.confirm_action.as_deref().unwrap_or(""),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(vec![
                Span::styled("  Task:   ", Style::default().fg(Color::DarkGray)),
                Span::styled(&app.workflow_task_id, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(
                    "y",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Confirm   ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    "n",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" Cancel", Style::default().fg(Color::DarkGray)),
            ]),
        ];

        let widget = Paragraph::new(confirm_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Confirm "),
        );
        f.render_widget(widget, area);
        return;
    }

    // Task ID input mode
    if app.workflow_input_mode {
        let input_display = format!("{}█", app.workflow_input);
        let input = Paragraph::new(Line::from(vec![Span::styled(
            &input_display,
            Style::default().fg(Color::Cyan),
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Enter Task ID "),
        );
        f.render_widget(input, area);
        return;
    }

    // Output if any
    if !app.output_lines.is_empty() {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(12), Constraint::Min(0)])
            .split(area);

        let menu = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  🔧  Workflow Ops",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  1",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Schedule", Style::default()),
            ]),
            Line::from(vec![
                Span::styled(
                    "  2",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Reconcile", Style::default()),
            ]),
            Line::from(vec![
                Span::styled(
                    "  3",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Execute effects", Style::default()),
            ]),
            Line::from(vec![
                Span::styled(
                    "  4",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Expire leases", Style::default()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Task ID: ", Style::default().fg(Color::DarkGray)),
                Span::styled(&app.workflow_task_id, Style::default().fg(Color::Cyan)),
            ]),
        ];

        let menu_widget = Paragraph::new(menu).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Actions "),
        );
        f.render_widget(menu_widget, chunks[0]);

        draw_output_panel(f, chunks[1], app);
        return;
    }

    // Menu
    let content = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  🔧  Workflow Ops",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  1",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Schedule", Style::default()),
        ]),
        Line::from(vec![
            Span::styled(
                "  2",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Reconcile", Style::default()),
        ]),
        Line::from(vec![
            Span::styled(
                "  3",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Execute effects", Style::default()),
        ]),
        Line::from(vec![
            Span::styled(
                "  4",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  Expire leases", Style::default()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Task ID: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&app.workflow_task_id, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Press 1-4 to select",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let widget = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Workflow Ops "),
    );
    f.render_widget(widget, area);
}

fn draw_palette(f: &mut Frame, app: &App) {
    let area = f.area();

    // Center the palette
    let palette_width = 60.min(area.width - 4);
    let palette_height = 15.min(area.height - 4);
    let x = (area.width - palette_width) / 2;
    let y = (area.height - palette_height) / 2;

    let palette_area = Rect::new(x, y, palette_width, palette_height);

    // Clear background
    let clear = Paragraph::new("");
    f.render_widget(clear, palette_area);

    // Search input
    let input_area = Rect::new(x + 1, y + 1, palette_width - 2, 3);
    let input_display = format!(" {}█", app.palette_query);
    let input = Paragraph::new(Line::from(vec![
        Span::styled("🔍", Style::default().fg(Color::Yellow)),
        Span::styled(&input_display, Style::default().fg(Color::Cyan)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Command Palette "),
    );
    f.render_widget(input, input_area);

    // Filtered items
    let items = app.get_filtered_palette_items();
    let visible_height = palette_height.saturating_sub(4) as usize;

    let start = if app.palette_index >= visible_height {
        app.palette_index - visible_height + 1
    } else {
        0
    };

    let list_items: Vec<ListItem> = items
        .iter()
        .skip(start)
        .take(visible_height)
        .enumerate()
        .map(|(_, (original_idx, item))| {
            let style = if *original_idx == app.palette_index {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let line = Line::from(vec![
                Span::styled("  ", style),
                Span::styled(&item.label, style),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list_area = Rect::new(x + 1, y + 4, palette_width - 2, visible_height as u16);
    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(format!(" {}/{} ", items.len(), app.palette_items.len())),
    );
    f.render_widget(list, list_area);
}
