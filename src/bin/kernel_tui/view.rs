//! TUI view layer — thin ratatui rendering over the kernel snapshot.
//!
//! This layer maps canonical kernel data (adapter snapshot) to widgets. It
//! never computes state: every status/state string rendered here comes from
//! the kernel snapshot untouched. Colors are display styling only.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use super::model::{InputMode, Model, Screen};

fn state_color(state: &str) -> Color {
    match state {
        "committed" | "completed" => Color::Green,
        "rejected" | "failed" => Color::Red,
        "dispatched" | "started" | "running" => Color::Yellow,
        "ready" => Color::Cyan,
        "active" => Color::Yellow,
        "expired" => Color::DarkGray,
        "pending" => Color::Gray,
        _ => Color::White,
    }
}

fn worker_from_payload(payload: &str) -> String {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| {
            v.get("worker_id")
                .and_then(|w| w.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "-".into())
}

pub fn draw(f: &mut Frame, m: &Model) {
    let area = f.area();
    // Controlled degradation on very small terminals: render a warning
    // instead of a corrupted layout. No kernel state is involved.
    if area.width < 40 || area.height < 10 {
        let warn = Paragraph::new("terminal too small — enlarge to use kernel-tui")
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(warn, area);
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Min(3),    // body
            Constraint::Length(2), // status
        ])
        .split(area);

    draw_header(f, m, chunks[0]);
    draw_body(f, m, chunks[1]);
    draw_status(f, m, chunks[2]);
}

fn draw_header(f: &mut Frame, m: &Model, area: Rect) {
    let tabs: Vec<Span> = [
        (Screen::Dashboard, "1:Dash"),
        (Screen::Tasks, "2:Tasks"),
        (Screen::Events, "3:Events"),
        (Screen::Workers, "4:Workers"),
        (Screen::Replay, "5:Replay"),
        (Screen::System, "6:System"),
    ]
    .iter()
    .map(|(s, label)| {
        let style = if m.screen == *s {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::Cyan)
        };
        Span::styled(format!(" {label} "), style)
    })
    .collect();

    let title = Line::from(vec![
        Span::styled(
            format!(" {} ", m.screen.title()),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ]);
    let mut title_spans = title.spans;
    title_spans.extend(tabs);
    let line1 = Line::from(title_spans);
    let line2 = Line::from(Span::styled(
        format!(" db: {}   (?) help  (q) quit", m.db_path),
        Style::default().fg(Color::DarkGray),
    ));
    let p = Paragraph::new(vec![line1, line2]);
    f.render_widget(p, area);
}

fn draw_status(f: &mut Frame, m: &Model, area: Rect) {
    let mut lines = vec![];

    // Confirmation modal for side-effecting actions takes priority.
    if let Some(pending) = &m.pending_confirm {
        lines.push(Line::from(Span::styled(
            format!("confirm: {} ? (y/n)", pending.describe()),
            Style::default().fg(Color::Magenta),
        )));
    }

    if m.input_mode != InputMode::None {
        let prompt = match m.input_mode {
            InputMode::PromptTaskId => "new task id> ",
            InputMode::Filter => "filter> ",
            InputMode::EventTypeFilter => "event type filter> ",
            InputMode::EventTextQuery => "event text query> ",
            InputMode::None => "",
        };
        lines.push(Line::from(Span::styled(
            format!("{prompt}{}", m.input_buffer),
            Style::default().fg(Color::Yellow),
        )));
    }

    if m.stale {
        lines.push(Line::from(Span::styled(
            "STALE VIEW: last refresh failed — showing previous snapshot",
            Style::default().fg(Color::Yellow),
        )));
    }
    if let Some(msg) = &m.status_msg {
        let style = if m.status_is_error {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };
        lines.push(Line::from(Span::styled(msg.clone(), style)));
    } else if let Some(err) = &m.data.error {
        lines.push(Line::from(Span::styled(
            format!("kernel query error: {err}"),
            Style::default().fg(Color::Red),
        )));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            " r:refresh  j/k:navigate  enter:open  esc:back  /:filter",
            Style::default().fg(Color::DarkGray),
        )));
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn draw_body(f: &mut Frame, m: &Model, area: Rect) {
    match m.screen {
        Screen::Dashboard => draw_dashboard(f, m, area),
        Screen::Tasks => draw_tasks(f, m, area),
        Screen::TaskDetail => draw_task_detail(f, m, area),
        Screen::Events => draw_events(f, m, area),
        Screen::Workers => draw_workers(f, m, area),
        Screen::Replay => draw_replay(f, m, area),
        Screen::System => draw_system(f, m, area),
        Screen::Help => draw_help(f, area),
    }
}

fn kv_line(k: &str, v: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<22}"), Style::default().fg(Color::DarkGray)),
        Span::raw(v),
    ])
}

fn draw_dashboard(f: &mut Frame, m: &Model, area: Rect) {
    let d = &m.data.dashboard;
    let mut lines = vec![
        kv_line("kernel", "deterministic_ai_kernel".into()),
        kv_line("db path", d.db_path.clone()),
        Line::from(""),
        kv_line("tasks (total)", d.total_tasks.to_string()),
        kv_line("  pending", d.pending_tasks.to_string()),
        kv_line("  running", d.running_tasks.to_string()),
        kv_line("  completed", d.completed_tasks.to_string()),
        kv_line("  failed", d.failed_tasks.to_string()),
        Line::from(""),
        kv_line("active leases", d.active_leases.to_string()),
        kv_line("workers", d.workers.to_string()),
        Line::from(""),
        kv_line("events", d.total_events.to_string()),
        kv_line("causal units", d.causal_units.to_string()),
        kv_line("max generation", d.max_generation.to_string()),
        Line::from(""),
        kv_line(
            "replay valid tasks",
            format!("{} / {}", d.replay_valid_tasks, d.replay_checked_tasks),
        ),
        kv_line(
            "last refresh",
            if m.stale {
                format!("#{} FAILED (stale view)", m.refresh_seq)
            } else {
                format!("#{} ok", m.refresh_seq)
            },
        ),
    ];

    // Recent events (last 8), straight from the kernel event log.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "recent events",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    let total = m.data.events.len();
    for ev in m.data.events.iter().skip(total.saturating_sub(8)) {
        lines.push(Line::from(Span::styled(
            format!(
                " g{:<5} u{:<5} {:<22} {} {}",
                ev.system_generation,
                ev.causal_unit_id,
                ev.event_type,
                ev.task_id,
                ev.step_id.as_deref().unwrap_or("-")
            ),
            Style::default().fg(Color::Gray),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Dashboard — canonical kernel state ");
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_tasks(f: &mut Frame, m: &Model, area: Rect) {
    let idx = m.filtered_task_indices();
    let rows: Vec<Row> = idx
        .iter()
        .enumerate()
        .map(|(pos, &i)| {
            let t = &m.data.tasks[i];
            let style = if pos == m.tasks_sel {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(t.task_id.clone()),
                Cell::from(t.task_class.clone()),
                Cell::from(Span::styled(
                    t.state.clone(),
                    Style::default().fg(state_color(&t.state)),
                )),
                Cell::from(t.current_step.clone()),
                Cell::from(t.active_lease_worker.clone()),
                Cell::from(t.latest_generation.to_string()),
            ])
            .style(style)
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("TASK ID"),
        Cell::from("CLASS"),
        Cell::from("STATE"),
        Cell::from("CURRENT STEP"),
        Cell::from("LEASE"),
        Cell::from("GEN"),
    ])
    .style(Style::default().fg(Color::Cyan));

    let mut title = format!(" Tasks ({}) ", idx.len());
    if !m.task_filter.is_empty() {
        title.push_str(&format!("id~'{}' ", m.task_filter));
    }
    if let Some(st) = &m.task_state_filter {
        title.push_str(&format!("state='{}' ", st));
    }
    if let Some(cl) = &m.task_class_filter {
        title.push_str(&format!("class='{}' ", cl));
    }
    title.push_str("— /:id  f:state  c:class  n:new  s:sched ");

    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(16),
            Constraint::Length(10),
            Constraint::Length(24),
            Constraint::Length(18),
            Constraint::Length(8),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(table, area);
}

fn draw_task_detail(f: &mut Frame, m: &Model, area: Rect) {
    let Some(d) = &m.data.detail else {
        let p = Paragraph::new("no task selected — open one from the Tasks screen").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Task Detail "),
        );
        f.render_widget(p, area);
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // Left: header + steps + leases
    let mut lines = vec![
        kv_line("task", d.task_id.clone()),
        kv_line("class", d.task_class.clone()),
        kv_line("task state", d.task_state.clone()),
        kv_line(
            "exec spec",
            if d.has_exec_spec { "present" } else { "absent" }.into(),
        ),
        kv_line("spec id", d.spec_id.clone()),
        kv_line("current step", d.current_step.clone()),
        kv_line("latest generation", d.latest_generation.to_string()),
        kv_line(
            "replay",
            if d.replay_valid {
                "VALID".to_string()
            } else {
                "INVALID".to_string()
            },
        ),
        Line::from(""),
        Line::from(Span::styled(
            "steps (canonical states)",
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ];
    for s in &d.steps {
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<26}", s.step_id)),
            Span::styled(
                s.status.clone(),
                Style::default().fg(state_color(&s.status)),
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "leases",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for l in d.leases.iter().take(12) {
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<34} step={:<24} state={} acq={} exp={}",
                l.lease_id, l.step_id, l.state, l.acquired_generation, l.expires_at_generation
            ),
            Style::default().fg(state_color(&l.state)),
        )));
    }

    let left = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} — b:rebuild snapshot  esc:back ", d.task_id)),
        )
        .wrap(Wrap { trim: false })
        .scroll((m.detail_scroll as u16, 0));
    f.render_widget(left, cols[0]);

    // Right: effects + artifacts + recent events + violations
    let mut rlines = vec![Line::from(Span::styled(
        "effects",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    for e in &d.effects {
        rlines.push(Line::from(Span::styled(
            format!(
                "  {} step={} g={}",
                e.state, e.step_id, e.reservation_generation
            ),
            Style::default().fg(state_color(&e.state)),
        )));
    }
    if d.effects.is_empty() {
        rlines.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    rlines.push(Line::from(""));
    rlines.push(Line::from(Span::styled(
        "artifacts",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for a in d.artifacts.iter().take(8) {
        rlines.push(Line::from(format!(
            "  [{}] {} step={} g={}",
            a.artifact_id, a.artifact_type, a.step_id, a.source_generation
        )));
    }
    if d.artifacts.is_empty() {
        rlines.push(Line::from(Span::styled(
            "  (none)",
            Style::default().fg(Color::DarkGray),
        )));
    }
    rlines.push(Line::from(""));
    rlines.push(Line::from(Span::styled(
        "recent events",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for ev in d.events.iter().rev().take(10) {
        rlines.push(Line::from(Span::styled(
            format!(
                " g{:<4} u{:<4} {} {}",
                ev.system_generation,
                ev.causal_unit_id,
                ev.event_type,
                ev.step_id.as_deref().unwrap_or("-")
            ),
            Style::default().fg(Color::Gray),
        )));
    }
    if !d.replay_violations.is_empty() {
        rlines.push(Line::from(""));
        rlines.push(Line::from(Span::styled(
            "replay violations",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        for v in d.replay_violations.iter().take(12) {
            rlines.push(Line::from(Span::styled(
                format!("  {v}"),
                Style::default().fg(Color::Red),
            )));
        }
    }
    let right = Paragraph::new(rlines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" effects / artifacts / events "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(right, cols[1]);
}

fn draw_events(f: &mut Frame, m: &Model, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(8)])
        .split(area);

    let filtered = m.filtered_event_indices();
    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .map(|(pos, &i)| {
            let ev = &m.data.events[i];
            let style = if pos == m.events_sel {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(ev.system_generation.to_string()),
                Cell::from(ev.causal_unit_id.to_string()),
                Cell::from(ev.sequence_in_unit.to_string()),
                Cell::from(ev.event_type.clone()),
                Cell::from(ev.task_id.clone()),
                Cell::from(ev.step_id.clone().unwrap_or_else(|| "-".into())),
                Cell::from(worker_from_payload(&ev.payload)),
            ])
            .style(style)
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("GEN"),
        Cell::from("UNIT"),
        Cell::from("SEQ"),
        Cell::from("EVENT"),
        Cell::from("TASK"),
        Cell::from("STEP"),
        Cell::from("WORKER"),
    ])
    .style(Style::default().fg(Color::Cyan));

    let mut title = format!(" Events ({}/{}) ", filtered.len(), m.data.events.len());
    if !m.event_task_filter.is_empty() {
        title.push_str(&format!("task='{}' ", m.event_task_filter));
    }
    if !m.event_type_filter.is_empty() {
        title.push_str(&format!("type='{}' ", m.event_type_filter));
    }
    if !m.event_text_query.is_empty() {
        title.push_str(&format!("text='{}' ", m.event_text_query));
    }
    title.push_str("— /:task  f:type  t:text ");

    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(4),
            Constraint::Length(22),
            Constraint::Length(20),
            Constraint::Length(24),
            Constraint::Length(16),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(table, chunks[0]);

    // Detail pane for the selected event: kernel event_log fields verbatim.
    // Unparseable payloads are shown raw with an explicit marker — never a
    // panic on malformed historical data.
    let selected = filtered
        .get(m.events_sel)
        .and_then(|&i| m.data.events.get(i));
    let mut lines: Vec<Line> = match selected {
        None => vec![Line::from(Span::styled(
            "no event selected",
            Style::default().fg(Color::DarkGray),
        ))],
        Some(ev) => {
            let mut l = vec![
                Line::from(vec![
                    Span::styled("generation ", Style::default().fg(Color::DarkGray)),
                    Span::raw(ev.system_generation.to_string()),
                    Span::styled("  unit ", Style::default().fg(Color::DarkGray)),
                    Span::raw(ev.causal_unit_id.to_string()),
                    Span::styled("  seq ", Style::default().fg(Color::DarkGray)),
                    Span::raw(ev.sequence_in_unit.to_string()),
                ]),
                Line::from(vec![
                    Span::styled("type ", Style::default().fg(Color::DarkGray)),
                    Span::raw(ev.event_type.clone()),
                    Span::styled("  task ", Style::default().fg(Color::DarkGray)),
                    Span::raw(ev.task_id.clone()),
                    Span::styled("  step ", Style::default().fg(Color::DarkGray)),
                    Span::raw(ev.step_id.clone().unwrap_or_else(|| "-".into())),
                    Span::styled("  worker ", Style::default().fg(Color::DarkGray)),
                    Span::raw(worker_from_payload(&ev.payload)),
                ]),
            ];
            match serde_json::from_str::<serde_json::Value>(&ev.payload) {
                Ok(v) => {
                    let pretty =
                        serde_json::to_string_pretty(&v).unwrap_or_else(|_| ev.payload.clone());
                    for line in pretty.lines().take(4) {
                        l.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::Gray),
                        )));
                    }
                }
                Err(_) => {
                    l.push(Line::from(Span::styled(
                        format!(
                            "[unparsed payload] {}",
                            ev.payload.chars().take(120).collect::<String>()
                        ),
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }
            l
        }
    };
    while lines.len() < 6 {
        lines.push(Line::from(""));
    }
    let detail = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" event detail "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(detail, chunks[1]);
}

fn draw_workers(f: &mut Frame, m: &Model, area: Rect) {
    let rows: Vec<Row> = m
        .data
        .workers
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let style = if i == m.workers_sel {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(w.worker_id.clone()),
                Cell::from(w.active_leases.to_string()),
                Cell::from(w.current_task.clone()),
                Cell::from(w.current_step.clone()),
                Cell::from(w.max_expires_at_generation.to_string()),
                Cell::from(w.observed_states.clone()),
            ])
            .style(style)
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("WORKER"),
        Cell::from("ACTIVE"),
        Cell::from("TASK"),
        Cell::from("STEP"),
        Cell::from("MAX EXPIRES"),
        Cell::from("STATES"),
    ])
    .style(Style::default().fg(Color::Cyan));

    let note = " Observable lease-derived status. Capability inference is informational only —\n it is NOT authorization (documented kernel limitation L3).";
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    let table = Table::new(
        rows,
        [
            Constraint::Length(22),
            Constraint::Length(7),
            Constraint::Length(20),
            Constraint::Length(24),
            Constraint::Length(12),
            Constraint::Length(24),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" Workers ({}) ", m.data.workers.len())),
    );
    f.render_widget(table, chunks[0]);
    f.render_widget(
        Paragraph::new(note).style(Style::default().fg(Color::DarkGray)),
        chunks[1],
    );
}

fn draw_replay(f: &mut Frame, m: &Model, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    // Left: task list for selection
    let idx = m.filtered_task_indices();
    let rows: Vec<Row> = idx
        .iter()
        .enumerate()
        .map(|(pos, &i)| {
            let t = &m.data.tasks[i];
            let style = if pos == m.replay_sel {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            Row::new(vec![
                Cell::from(t.task_id.clone()),
                Cell::from(Span::styled(
                    t.state.clone(),
                    Style::default().fg(state_color(&t.state)),
                )),
            ])
            .style(style)
        })
        .collect();
    let table = Table::new(rows, [Constraint::Length(24), Constraint::Length(12)])
        .header(
            Row::new(vec![Cell::from("TASK"), Cell::from("STATE")])
                .style(Style::default().fg(Color::Cyan)),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" select task — enter/r: validate "),
        );
    f.render_widget(table, cols[0]);

    // Right: kernel-produced replay report
    let mut lines = vec![];
    match &m.data.replay {
        Some(rep) => {
            // Three distinct outcomes: the kernel verdict (OK/INVALID) and
            // CHECK FAILED (the check itself could not run — semantically
            // different from INVALID).
            let (label, color) = if rep.check_error.is_some() {
                ("REPLAY CHECK FAILED", Color::Yellow)
            } else if rep.valid {
                ("REPLAY OK", Color::Green)
            } else {
                ("REPLAY INVALID", Color::Red)
            };
            lines.push(Line::from(Span::styled(
                format!("task: {}", rep.task_id),
                Style::default().fg(Color::White),
            )));
            lines.push(Line::from(Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                format!(
                    "events: {}   generations: {}..{}   violations: {}",
                    rep.event_count,
                    rep.min_generation,
                    rep.max_generation,
                    rep.violations.len()
                ),
                Style::default().fg(Color::DarkGray),
            )));
            if let Some(err) = &rep.check_error {
                lines.push(Line::from(Span::styled(
                    format!("check error: {err}"),
                    Style::default().fg(Color::Yellow),
                )));
            }
            lines.push(Line::from(""));
            if rep.violations.is_empty() {
                lines.push(Line::from(Span::styled(
                    "no violations (canonical fold)",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                lines.push(Line::from(Span::styled(
                    "violations reported by the kernel:",
                    Style::default().fg(Color::Yellow),
                )));
                for v in &rep.violations {
                    lines.push(Line::from(Span::styled(
                        format!("  {v}"),
                        Style::default().fg(Color::Red),
                    )));
                }
            }
        }
        None => {
            lines.push(Line::from(Span::styled(
                "select a task and press enter — validation runs through the kernel's own replay_violations/replay_validate (no UI-side validator).",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    // Integrity self-check (on-demand, key 'i') — result from the kernel's
    // api::integrity_json_report (scratch DB, non-destructive). A failed
    // check is shown as CHECK FAILED, never conflated with a passing check.
    lines.push(Line::from(""));
    if let Some(integ) = &m.data.integrity {
        let (label, color) = if integ.ok {
            ("INTEGRITY OK", Color::Green)
        } else {
            ("INTEGRITY CHECK FAILED", Color::Red)
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            integ.summary.clone(),
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "press i: run integrity self-check (kernel scratch DB, non-destructive)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let p = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" replay validation + integrity (kernel) — i:integrity "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(p, cols[1]);
}

fn draw_system(f: &mut Frame, m: &Model, area: Rect) {
    let s = &m.data.system;
    let lines = vec![
        kv_line("kernel version", s.kernel_version.clone()),
        kv_line("db path", s.db_path.clone()),
        kv_line(
            "database",
            if s.db_accessible {
                "ACCESSIBLE".to_string()
            } else {
                format!(
                    "UNAVAILABLE ({})",
                    s.db_error.as_deref().unwrap_or("unknown error")
                )
            },
        ),
        kv_line(
            "refresh",
            if m.stale {
                format!("#{} FAILED (stale view)", m.refresh_seq)
            } else {
                format!("#{} ok", m.refresh_seq)
            },
        ),
        Line::from(""),
        kv_line("events", s.total_events.to_string()),
        kv_line("causal units", s.causal_units.to_string()),
        kv_line("max generation", s.max_generation.to_string()),
        kv_line("tasks (tasks table)", s.total_tasks.to_string()),
        kv_line("tasks (event log)", s.tasks_in_event_log.to_string()),
        kv_line("leases", s.total_leases.to_string()),
        Line::from(""),
        Line::from(Span::styled(
            "effect ledger: per-task (kernel exposes no global count;",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "see Task Detail for committed/reserved/rejected effects)",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "schema tables present",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            s.schema_tables.join(", "),
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "health: read-only observation; integrity checks run through",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "api::integrity_json_report (non-destructive scratch copy).",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" System "))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "kernel-tui — observation + control over the deterministic kernel",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  q        quit"),
        Line::from("  1        dashboard"),
        Line::from("  2        tasks"),
        Line::from("  3        events"),
        Line::from("  4        workers"),
        Line::from("  5        replay / integrity"),
        Line::from("  6        system"),
        Line::from("  j / ↓    next"),
        Line::from("  k / ↑    previous"),
        Line::from("  enter    open (task detail / revalidate replay)"),
        Line::from("  esc      back"),
        Line::from("  r        refresh"),
        Line::from("  /        filter (tasks / events by task id)"),
        Line::from("  n        new task (Tasks screen)"),
        Line::from("  s        schedule selected task (asks y/n)"),
        Line::from("  b        rebuild snapshot (asks y/n)"),
        Line::from("  i        integrity self-check (Replay screen)"),
        Line::from("  y / n    confirm / cancel a pending action"),
        Line::from("  ?        this help"),
        Line::from(""),
        Line::from(Span::styled(
            "All canonical state comes from kernel queries. The TUI never derives",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "task/step/lease/effect state and never exposes shell execution.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Help "))
        .wrap(Wrap { trim: false });
    f.render_widget(p, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::{
        DashboardData, ReplayReportVm, StepVm, SystemVm, TaskDetailVm, TaskRowVm, WorkerVm,
    };
    use crate::model::Model;
    use deterministic_ai_kernel::providers::storage::EventDetailRow;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render(model: &Model) -> String {
        let backend = TestBackend::new(120, 34);
        let mut term = Terminal::new(backend).expect("test backend");
        term.draw(|f| draw(f, model)).expect("draw");
        let size = term.size().expect("size");
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..size.height {
            for x in 0..size.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn sample_model(screen: Screen) -> Model {
        let mut m = Model::new("/tmp/test.db");
        m.screen = screen;
        m.data.dashboard = DashboardData {
            db_path: "/tmp/test.db".into(),
            total_tasks: 2,
            pending_tasks: 1,
            running_tasks: 1,
            completed_tasks: 0,
            failed_tasks: 0,
            active_leases: 1,
            workers: 1,
            total_events: 5,
            causal_units: 4,
            max_generation: 4,
            replay_checked_tasks: 2,
            replay_valid_tasks: 2,
            last_error: None,
        };
        m.data.tasks = vec![
            TaskRowVm {
                task_id: "task-alpha".into(),
                task_class: "Generic".into(),
                state: "running".into(),
                current_step: "00_analyze_task".into(),
                active_lease_worker: "worker-planner".into(),
                latest_generation: 3,
            },
            TaskRowVm {
                task_id: "task-beta".into(),
                task_class: "CodeFix".into(),
                state: "pending".into(),
                current_step: "-".into(),
                active_lease_worker: "-".into(),
                latest_generation: 0,
            },
        ];
        m.data.workers = vec![WorkerVm {
            worker_id: "worker-planner".into(),
            active_leases: 1,
            current_task: "task-alpha".into(),
            current_step: "00_analyze_task".into(),
            max_expires_at_generation: 5,
            observed_states: "active".into(),
        }];
        m.data.events = vec![EventDetailRow {
            id: 1,
            system_generation: 2,
            causal_unit_id: 2,
            sequence_in_unit: 0,
            task_id: "task-alpha".into(),
            step_id: Some("00_analyze_task".into()),
            event_type: "STEP_STARTED".into(),
            payload: "{\"worker_id\":\"worker-planner\"}".into(),
        }];
        m.data.system = SystemVm {
            db_path: "/tmp/test.db".into(),
            db_accessible: true,
            db_error: None,
            total_events: 5,
            causal_units: 4,
            max_generation: 4,
            tasks_in_event_log: 2,
            total_tasks: 2,
            total_leases: 1,
            schema_tables: vec!["event_log".into(), "tasks".into()],
            kernel_version: "0.1.0".into(),
        };
        m
    }

    #[test]
    fn dashboard_renders_canonical_counters() {
        let m = sample_model(Screen::Dashboard);
        let text = render(&m);
        assert!(text.contains("DASHBOARD"));
        assert!(text.contains("/tmp/test.db"));
        assert!(text.contains("task"));
        assert!(text.contains("active leases"));
        assert!(text.contains("STEP_STARTED"));
    }

    #[test]
    fn tasks_screen_renders_rows_and_states() {
        let m = sample_model(Screen::Tasks);
        let text = render(&m);
        assert!(text.contains("TASK ID"));
        assert!(text.contains("task-alpha"));
        assert!(text.contains("task-beta"));
        assert!(text.contains("running"));
        assert!(text.contains("pending"));
        assert!(text.contains("00_analyze_task"));
    }

    #[test]
    fn task_detail_renders_steps_from_canonical_states() {
        let mut m = sample_model(Screen::TaskDetail);
        m.data.detail = Some(TaskDetailVm {
            task_id: "task-alpha".into(),
            task_class: "Generic".into(),
            task_state: "running".into(),
            has_exec_spec: true,
            spec_id: "spec-1".into(),
            steps: vec![
                StepVm {
                    step_id: "00_analyze_task".into(),
                    status: "committed".into(),
                },
                StepVm {
                    step_id: "01_plan_execution".into(),
                    status: "pending".into(),
                },
            ],
            leases: vec![],
            events: vec![],
            effects: vec![],
            artifacts: vec![],
            replay_valid: true,
            replay_violations: vec![],
            latest_generation: 3,
            current_step: "01_plan_execution".into(),
        });
        let text = render(&m);
        assert!(text.contains("task-alpha"));
        assert!(text.contains("00_analyze_task"));
        assert!(text.contains("committed"));
        assert!(text.contains("01_plan_execution"));
        assert!(text.contains("VALID"));
    }

    #[test]
    fn events_screen_renders_real_event_fields() {
        let m = sample_model(Screen::Events);
        let text = render(&m);
        assert!(text.contains("GEN"));
        assert!(text.contains("UNIT"));
        assert!(text.contains("SEQ"));
        assert!(text.contains("STEP_STARTED"));
        assert!(text.contains("task-alpha"));
        // worker column is a display projection from the event payload
        assert!(text.contains("worker-planner"));
    }

    #[test]
    fn workers_screen_renders_and_notes_informational_only() {
        let m = sample_model(Screen::Workers);
        let text = render(&m);
        assert!(text.contains("worker-planner"));
        assert!(text.contains("task-alpha"));
        assert!(text.contains("NOT authorization"));
    }

    #[test]
    fn replay_screen_renders_kernel_verdict_and_violations() {
        let mut m = sample_model(Screen::Replay);
        m.data.replay = Some(ReplayReportVm {
            task_id: "task-alpha".into(),
            valid: false,
            violations: vec!["INVALID step 00_analyze_task: broken".into()],
            check_error: None,
            event_count: 4,
            min_generation: 1,
            max_generation: 4,
        });
        let text = render(&m);
        assert!(text.contains("REPLAY INVALID"));
        assert!(text.contains("INVALID step 00_analyze_task: broken"));

        m.data.replay = Some(ReplayReportVm {
            task_id: "task-alpha".into(),
            valid: true,
            violations: vec![],
            check_error: None,
            event_count: 4,
            min_generation: 1,
            max_generation: 4,
        });
        let text = render(&m);
        assert!(text.contains("REPLAY OK"));
    }

    #[test]
    fn system_screen_renders_db_and_schema_info() {
        let m = sample_model(Screen::System);
        let text = render(&m);
        assert!(text.contains("/tmp/test.db"));
        assert!(text.contains("event_log"));
        assert!(text.contains("0.1.0"));
    }

    #[test]
    fn snapshot_error_is_rendered_in_status_bar() {
        let mut m = sample_model(Screen::Dashboard);
        m.data.error = Some("tasks query failed: db locked".into());
        let text = render(&m);
        assert!(text.contains("tasks query failed: db locked"));
    }

    #[test]
    fn help_screen_renders_keymap() {
        let m = sample_model(Screen::Help);
        let text = render(&m);
        assert!(text.contains("quit"));
        assert!(text.contains("refresh"));
        assert!(text.contains("filter"));
    }

    #[test]
    fn empty_snapshot_renders_all_screens_without_panic() {
        // A completely empty kernel snapshot (fresh DB) must render every
        // screen without panicking and without fabricating data.
        for screen in [
            Screen::Dashboard,
            Screen::Tasks,
            Screen::TaskDetail,
            Screen::Events,
            Screen::Workers,
            Screen::Replay,
            Screen::System,
        ] {
            let mut m = Model::new("/tmp/fresh.db");
            m.screen = screen;
            let text = render(&m);
            assert!(!text.is_empty(), "screen {screen:?} must render");
        }
    }

    #[test]
    fn system_screen_labels_effect_ledger_as_per_task() {
        // The kernel exposes no global effect count; the UI must not
        // fabricate one (phase 9: accurate labeling).
        let m = sample_model(Screen::System);
        let text = render(&m);
        assert!(text.contains("effect ledger: per-task"));
    }

    #[test]
    fn replay_check_failed_is_distinct_from_invalid() {
        let mut m = sample_model(Screen::Replay);
        m.data.replay = Some(ReplayReportVm {
            task_id: "task-alpha".into(),
            valid: false,
            violations: vec![],
            check_error: Some("db unreachable".into()),
            event_count: 0,
            min_generation: 0,
            max_generation: 0,
        });
        let text = render(&m);
        assert!(text.contains("REPLAY CHECK FAILED"));
        assert!(!text.contains("REPLAY INVALID"));
        assert!(text.contains("db unreachable"));
    }

    #[test]
    fn event_detail_handles_unparsed_payload_without_panic() {
        let mut m = sample_model(Screen::Events);
        m.data.events[0].payload = "{not valid json".to_string();
        let text = render(&m);
        assert!(text.contains("[unparsed payload]"));
    }

    #[test]
    fn system_screen_shows_db_unavailable() {
        let mut m = sample_model(Screen::System);
        m.data.system.db_accessible = false;
        m.data.system.db_error = Some("cannot open".into());
        let text = render(&m);
        assert!(text.contains("UNAVAILABLE"));
        assert!(text.contains("cannot open"));
    }

    #[test]
    fn stale_banner_renders_on_failed_refresh() {
        let mut m = sample_model(Screen::Dashboard);
        m.stale = true;
        let text = render(&m);
        assert!(text.contains("STALE VIEW"));
    }

    #[test]
    fn task_detail_shows_current_step() {
        let mut m = sample_model(Screen::TaskDetail);
        m.data.detail = Some(TaskDetailVm {
            task_id: "task-alpha".into(),
            task_class: "Generic".into(),
            task_state: "running".into(),
            has_exec_spec: true,
            spec_id: "spec-1".into(),
            steps: vec![],
            leases: vec![],
            events: vec![],
            effects: vec![],
            artifacts: vec![],
            replay_valid: true,
            replay_violations: vec![],
            latest_generation: 3,
            current_step: "01_plan_execution".into(),
        });
        let text = render(&m);
        assert!(text.contains("current step"));
        assert!(text.contains("01_plan_execution"));
    }

    #[test]
    fn integrity_result_renders_ok_and_failed_distinctly() {
        use crate::adapter::IntegrityVm;
        // Passing integrity check.
        let mut m = sample_model(Screen::Replay);
        m.data.integrity = Some(IntegrityVm {
            ok: true,
            summary: "snapshot_version=1 schema_version=1".into(),
        });
        let text = render(&m);
        assert!(text.contains("INTEGRITY OK"));
        assert!(!text.contains("INTEGRITY CHECK FAILED"));
        // Failing integrity check renders distinctly.
        m.data.integrity = Some(IntegrityVm {
            ok: false,
            summary: "integrity check failed: boom".into(),
        });
        let text = render(&m);
        assert!(text.contains("INTEGRITY CHECK FAILED"));
        assert!(text.contains("integrity check failed: boom"));
    }

    #[test]
    fn replay_screen_hints_integrity_key_when_no_result() {
        let m = sample_model(Screen::Replay);
        let text = render(&m);
        assert!(text.contains("press i: run integrity self-check"));
    }

    #[test]
    fn confirmation_banner_renders_for_pending_action() {
        use crate::model::PendingAction;
        let mut m = sample_model(Screen::Tasks);
        m.pending_confirm = Some(PendingAction::ScheduleTask("task-x".into()));
        let text = render(&m);
        assert!(text.contains("confirm:"));
        assert!(text.contains("schedule task"));
        assert!(text.contains("task-x"));
        assert!(text.contains("(y/n)"));
    }

    #[test]
    fn too_small_terminal_renders_controlled_warning() {
        let m = sample_model(Screen::Dashboard);
        let backend = TestBackend::new(30, 8);
        let mut term = Terminal::new(backend).expect("backend");
        term.draw(|f| draw(f, &m)).expect("draw");
        let size = term.size().expect("size");
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..size.height {
            for x in 0..size.width {
                out.push_str(buf[(x, y)].symbol());
            }
        }
        assert!(out.contains("terminal too small"));
    }
}
