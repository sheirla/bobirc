use crate::app::{
    filtered_commands, menu_is_open, App, ChatMessage, Popup, Role, Screen, SetupField, BOT_NAME,
    COMMANDS, ThinkState,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

// base color: #CDDC2A (Material Lime 500)
const BASE: Color = Color::Rgb(0xCD, 0xDC, 0x2A);
const TITLE: Color = BASE;
const BOT: Color = BASE;

const YOU: Color = Color::Rgb(0x66, 0xBB, 0x6A); // medium green, distinct from lime
const SYSTEM: Color = Color::Rgb(0xFF, 0xB3, 0x00); // amber
const ERROR: Color = Color::Rgb(0xFF, 0x52, 0x52); // red
const SEL: Color = Color::Rgb(0xE0, 0x40, 0xFB); // magenta accent
const DIM: Color = Color::Rgb(0x70, 0x70, 0x80);
const INPUT: Color = Color::Rgb(0xEC, 0xEC, 0xEC);
const BG: Color = Color::Rgb(0x14, 0x14, 0x1C);
const BG_HI: Color = Color::Rgb(0x28, 0x28, 0x18); // active field bg, slight base tint
const BG_SEL: Color = Color::Rgb(0x2A, 0x2A, 0x32);
const STREAMING: Color = Color::Rgb(0xFF, 0xB3, 0x00);

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(ratatui::widgets::Clear, area);

    // root vertical: top status (1) | middle (flex) | bottom input area (5)
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(5),
        ])
        .split(area);

    draw_status(f, app, v[0]);

    match app.screen {
        Screen::Setup => draw_setup(f, app, v[1]),
        Screen::ModelSelect => draw_model_select(f, app, v[1]),
        Screen::Chat => {
            // chat row: sessions panel on the left | chat area on the right
            let h = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(24),
                    Constraint::Min(20),
                ])
                .split(v[1]);
            draw_sessions_panel(f, app, h[0]);
            draw_chat(f, app, h[1]);
            if menu_is_open(app) {
                draw_menu(f, app, h[1]);
            }
        }
    }

    draw_input(f, app, v[2]);
    draw_toast(f, app, area);

    // popup renders LAST so it sits on top of every other widget
    if let Some(popup) = app.popup.as_ref() {
        // clone what we need so we don't hold a borrow on `app` while
        // rendering -- avoids a double-mutable-borrow conflict with
        // the other draw calls
        let popup_clone = popup.clone();
        draw_popup(f, &popup_clone, area);
    }
}

fn draw_sessions_panel(f: &mut Frame, app: &App, area: Rect) {
    let nav_mode = app.session_nav_mode;
    let border_color = if nav_mode { TITLE } else { DIM };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            if nav_mode { " Sessions (NAV) " } else { " Sessions " },
            Style::default().fg(border_color).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(block, area);

    let inner = inset(area, 1);
    let mut items: Vec<ListItem> = Vec::new();
    if app.sessions.is_empty() {
        items.push(ListItem::new(Span::styled(
            "  (no sessions)",
            Style::default().fg(DIM),
        )));
    } else {
        for (i, m) in app.sessions.iter().enumerate() {
            let is_active = m.id == app.active_session_id;
            let is_selected = i == app.session_panel_idx;
            let marker = if is_active { "● " } else { "○ " };
            let row_text = format!("{}{}", marker, m.name);
            let mut style = Style::default().fg(INPUT);
            if is_active {
                style = style.fg(BOT);
            }
            if is_selected {
                style = style.bg(BG_SEL).add_modifier(Modifier::BOLD);
            }
            if is_selected && nav_mode {
                style = style.fg(SEL);
            }
            let mut spans = vec![Span::styled(row_text, style)];
            // small timestamp column on the right
            if !m.updated_at.is_empty() {
                spans.push(Span::styled(
                    format!("  {}", m.updated_at),
                    Style::default().fg(DIM),
                ));
            }
            items.push(ListItem::new(Line::from(spans)));
        }
    }
    let list = List::new(items);
    f.render_widget(list, inner);

    // small hint at the bottom of the panel
    let hint = if nav_mode {
        " ↑↓ nav  ⏎ open  n new  d del  Esc "
    } else {
        " F2 navigate  Alt+1..9 jump "
    };
    let hint_y = area.y + area.height.saturating_sub(1);
    if hint_y >= area.y {
        let p = Paragraph::new(Span::styled(hint, Style::default().fg(DIM)));
        f.render_widget(p, Rect { x: area.x + 1, y: hint_y, width: area.width.saturating_sub(2), height: 1 });
    }
}

fn draw_toast(f: &mut Frame, app: &App, area: Rect) {
    // auto-dismiss after 3.5s
    const TOAST_TTL: std::time::Duration = std::time::Duration::from_millis(3500);
    let (msg, set_at) = match &app.toast {
        Some(t) => t,
        None => return,
    };
    if set_at.elapsed() > TOAST_TTL {
        return;
    }
    let w = (msg.chars().count() as u16 + 4).min(area.width.saturating_sub(4));
    let h = 3u16;
    let x = area.x + area.width.saturating_sub(w + 2);
    let y = area.y + 1;
    let toast_area = Rect { x, y, width: w, height: h };
    f.render_widget(Clear, toast_area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE))
        .title(Span::styled(
            " toast ",
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ));
    let p = Paragraph::new(msg.as_str())
        .block(block)
        .style(Style::default().fg(INPUT).bg(BG));
    f.render_widget(p, toast_area);
}

fn draw_popup(f: &mut Frame, app: &Popup, area: Rect) {
    // Centered modal, ~60% width, ~60% height, min 40x10.
    let popup_w = (area.width * 3 / 5).max(40).min(area.width.saturating_sub(4));
    let popup_h = (area.height * 3 / 5)
        .max(10)
        .min(area.height.saturating_sub(4));
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup_area = Rect { x, y, width: popup_w, height: popup_h };
    f.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE))
        .title(Span::styled(
            format!(" {} ", app.title),
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ));
    let p = Paragraph::new(app.body.as_str())
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((app.scroll, 0))
        .style(Style::default().fg(INPUT));
    f.render_widget(p, popup_area);

    // hint at the very bottom of the popup
    let hint = " Esc/Enter close · j/k · PgUp/PgDn · g/G ";
    let hint_y = popup_area.y + popup_area.height.saturating_sub(1);
    if hint_y >= popup_area.y {
        let hint_p = Paragraph::new(Span::styled(hint, Style::default().fg(DIM)));
        f.render_widget(
            hint_p,
            Rect {
                x: popup_area.x + 1,
                y: hint_y,
                width: popup_area.width.saturating_sub(2),
                height: 1,
            },
        );
    }
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let screen = match app.screen {
        Screen::Setup => "SETUP",
        Screen::ModelSelect => "MODEL-SELECT",
        Screen::Chat => "CHAT",
    };
    let left = format!(
        " bobric v0.2 │ screen:{} │ server:{} │ nick:{} ",
        screen,
        short(&mask_url(&app.cfg.base_url), 32),
        app.cfg.nick,
    );
    let right = if !app.cfg.model.is_empty() {
        format!(" model:{} ", app.cfg.model)
    } else {
        " no model selected ".to_string()
    };
    let usage = if let Some(u) = &app.last_usage {
        format!(" │ in:{} out:{} tot:{} ", u.prompt, u.completion, u.total)
    } else {
        String::new()
    };
    let state = if app.streaming {
        let label = if app.think_state == ThinkState::InThink {
            " ● thinking "
        } else if app.stream_buf.is_empty() {
            " ● awaiting "
        } else {
            " ● streaming "
        };
        Span::styled(label, Style::default().fg(STREAMING).add_modifier(Modifier::BOLD))
    } else {
        Span::styled("", Style::default())
    };
    let p = Paragraph::new(Line::from(vec![
        Span::styled(left, Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::styled("│", Style::default().fg(DIM)),
        Span::styled(right, Style::default().fg(BOT)),
        Span::styled(usage, Style::default().fg(DIM)),
        state,
    ]))
    .style(Style::default().bg(BG));
    f.render_widget(p, area);
}

fn draw_setup(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE))
        .title(Span::styled(
            " Connection Setup ",
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(block, area);

    let inner = inset(area, 2);

    // rows: 6 rows × single-line input + 4 rows for system prompt + status + hint
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // base url label
            Constraint::Length(3),  // base url input
            Constraint::Length(1),  // api key label
            Constraint::Length(3),  // api key input
            Constraint::Length(1),  // nick label
            Constraint::Length(3),  // nick input
            Constraint::Length(1),  // system prompt label
            Constraint::Length(5),  // system prompt input (multi-line)
            Constraint::Length(2),  // status
            Constraint::Length(1),  // hint
            Constraint::Min(0),
        ])
        .split(inner);

    let label = |s: &str, active: bool| -> Span {
        if active {
            Span::styled(format!("▶ {}", s), Style::default().fg(SEL).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(format!("  {}", s), Style::default().fg(DIM))
        }
    };

    f.render_widget(Paragraph::new(label("Base URL", app.setup_field == SetupField::BaseUrl)), rows[0]);
    f.render_widget(
        input_box(&app.setup_base, app.setup_field == SetupField::BaseUrl, false),
        rows[1],
    );

    f.render_widget(Paragraph::new(label("API Key", app.setup_field == SetupField::ApiKey)), rows[2]);
    f.render_widget(
        input_box_masked(&app.setup_key, app.setup_field == SetupField::ApiKey),
        rows[3],
    );

    f.render_widget(Paragraph::new(label("Nick", app.setup_field == SetupField::Nick)), rows[4]);
    f.render_widget(
        input_box(&app.setup_nick, app.setup_field == SetupField::Nick, false),
        rows[5],
    );

    f.render_widget(
        Paragraph::new(label("System Prompt (Shift+Enter = newline)", app.setup_field == SetupField::SystemPrompt)),
        rows[6],
    );
    f.render_widget(
        input_box(&app.setup_system, app.setup_field == SetupField::SystemPrompt, true),
        rows[7],
    );

    let hint = Line::from(vec![
        Span::styled("Tab", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" switch  "),
        Span::styled("Enter", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" save  "),
        Span::styled("Shift+Enter", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" newline in prompt  "),
        Span::styled("Esc", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" back"),
    ]);
    f.render_widget(Paragraph::new(hint), rows[9]);

    if let Some(s) = &app.setup_status {
        let style = if s.starts_with("OK") {
            Style::default().fg(SYSTEM)
        } else {
            Style::default().fg(ERROR)
        };
        f.render_widget(Paragraph::new(Span::styled(s, style)), rows[8]);
    }
}

fn draw_model_select(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE))
        .title(Span::styled(
            " Model Select ",
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(block, area);

    let inner = inset(area, 1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .split(inner);

    let hint = Line::from(vec![
        Span::styled("[T]", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" Test  "),
        Span::styled("[F]", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" Fetch  "),
        Span::styled("[Enter]", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" Select  "),
        Span::styled("[Esc]", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" Back to setup"),
    ]);
    f.render_widget(Paragraph::new(hint), rows[3]);

    if app.fetching || app.testing {
        let s = if app.fetching { "Fetching models..." } else { "Testing connection..." };
        f.render_widget(
            Paragraph::new(Span::styled(s, Style::default().fg(SYSTEM))),
            rows[0],
        );
    } else if let Some(s) = &app.model_status {
        let style = if s.starts_with("OK") {
            Style::default().fg(SYSTEM)
        } else {
            Style::default().fg(ERROR)
        };
        f.render_widget(Paragraph::new(Span::styled(s, style)), rows[0]);
    } else {
        f.render_widget(
            Paragraph::new(Span::styled("Press F to fetch models", Style::default().fg(DIM))),
            rows[0],
        );
    }

    let items: Vec<ListItem> = if app.models.is_empty() {
        vec![ListItem::new(Span::styled("  (no models)", Style::default().fg(DIM)))]
    } else {
        app.models
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let marker = if m.id == app.cfg.model { "●" } else { "○" };
                let row = format!("  {} {}", marker, m.id);
                let style = if i == app.model_idx {
                    Style::default().fg(INPUT).bg(BG_SEL).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(INPUT)
                };
                ListItem::new(Span::styled(row, style))
            })
            .collect()
    };
    let list = List::new(items);
    f.render_widget(list, rows[1]);

    let help = Line::from(vec![
        Span::styled("↑/↓", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" navigate  "),
        Span::styled("PgUp/PgDn", Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::raw(" page"),
    ]);
    f.render_widget(Paragraph::new(help), rows[2]);
}

fn draw_chat(f: &mut Frame, app: &mut App, area: Rect) {
    // split: chat (left) | user list (right 18 cols)
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(18)])
        .split(area);

    draw_chat_main(f, app, h[0]);
    draw_user_list(f, app, h[1]);
}

fn draw_chat_main(f: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE))
        .title(Span::styled(
            " #main ",
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(block, area);

    let nick = &app.cfg.nick;
    let inner = inset(area, 1);

    let has_conversation = app
        .messages
        .iter()
        .any(|m| matches!(m.role, Role::You | Role::Bot));
    let show_empty = !has_conversation && !app.streaming && app.stream_buf.is_empty();

    if show_empty {
        draw_empty_state(f, app, inner);
        return;
    }

    // The message index that the current /search match lives in (if
    // any). Everything else renders plain; that one message gets the
    // query substring highlighted in amber so the user can see what
    // they jumped to.
    let search_hit_idx = if app.search_query.is_some() && !app.search_matches.is_empty() {
        Some(app.search_matches[app.search_idx.min(app.search_matches.len() - 1)])
    } else {
        None
    };
    let highlight_q = app.search_query.as_deref();

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (i, m) in app.messages.iter().enumerate() {
        let hl = if Some(i) == search_hit_idx { highlight_q } else { None };
        lines.extend(render_message(m, nick, hl));
    }
    if app.streaming || !app.stream_buf.is_empty() {
        let content = if app.think_state == ThinkState::InThink {
            // model is inside a <think> block; show the animated indicator
            let frame = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
            format!("{} thinking deeply...", frame)
        } else if app.streaming && app.stream_buf.is_empty() {
            // request sent, first token not yet arrived
            let frame = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
            format!("{} awaiting response...", frame)
        } else {
            // active stream -- render whatever tokens have landed
            app.stream_buf.clone()
        };
        let bot_msg = ChatMessage {
            role: Role::Bot,
            content,
            time: crate::app::now(),
        };
        // the in-flight bot_msg doesn't correspond to a saved
        // message index, so we don't highlight it for now (the
        // completed message gets the highlight once it's pushed
        // into app.messages on Done).
        lines.extend(render_message(&bot_msg, nick, None));
    }

    // Apply follow_tail / scroll. `lines.len()` is the unwrapped line
    // count, so max_scroll is approximate when wrap kicks in for very
    // long lines; close enough for typical chat. The render clamps to
    // the visible area, so over-shooting just means we land on the
    // bottom which is exactly what follow_tail wants anyway.
    let max_scroll = lines.len().saturating_sub(inner.height as usize) as u16;
    let actual_scroll = if app.follow_tail {
        max_scroll
    } else {
        app.scroll.min(max_scroll)
    };
    // If we ended up at the bottom anyway, re-engage auto-follow so
    // the next streaming delta keeps us pinned there.
    if max_scroll > 0 && actual_scroll >= max_scroll {
        app.follow_tail = true;
    } else if actual_scroll < max_scroll {
        app.follow_tail = false;
    }
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((actual_scroll, 0));
    f.render_widget(p, inner);
}

fn draw_user_list(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE))
        .title(Span::styled(
            " Users ",
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(block, area);

    let mut items: Vec<ListItem> = Vec::new();
    let you_prefix = if app.streaming { "~" } else { "@" };
    items.push(ListItem::new(Line::from(vec![
        Span::styled(format!("{} ", you_prefix), Style::default().fg(SYSTEM).add_modifier(Modifier::BOLD)),
        Span::styled(app.cfg.nick.clone(), Style::default().fg(YOU).add_modifier(Modifier::BOLD)),
    ])));
    let bot_state: String = if app.streaming {
        if app.think_state == ThinkState::InThink {
            let frame = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
            format!(" {} thinking", frame)
        } else if app.stream_buf.is_empty() {
            let frame = SPINNER_FRAMES[app.spinner_frame % SPINNER_FRAMES.len()];
            format!(" {} awaiting", frame)
        } else {
            " (typing...)".to_string()
        }
    } else {
        String::new()
    };
    items.push(ListItem::new(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(BOT_NAME, Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::styled(bot_state, Style::default().fg(DIM)),
    ])));

    f.render_widget(List::new(items), inset(area, 1));
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let title = match app.screen {
        Screen::Chat => {
            if app.streaming {
                " Bot is typing… (Esc to cancel) "
            } else {
                " Input (Enter = send, Shift+Enter = newline) "
            }
        }
        Screen::Setup => " Setup ",
        Screen::ModelSelect => " Model Select ",
    };

    let (body, show_input_text) = match app.screen {
        Screen::Chat => {
            if app.streaming {
                (String::new(), false)
            } else {
                (format!("> {}", app.input), true)
            }
        }
        _ => (String::new(), false),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE))
        .title(Span::styled(
            title,
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ));

    // Text renders inside the block's borders. Inner text origin is
    // (area.x+1, area.y+1) and the wrap width is area.width-2.
    let text_x = area.x + 1;
    let text_y = area.y + 1;
    let text_w = area.width.saturating_sub(2);
    let text_h = area.height.saturating_sub(2);

    let p = Paragraph::new(body.clone())
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(INPUT).bg(BG));
    f.render_widget(p, area);

    if show_input_text {
        // Simulate the same wrap the Paragraph uses, so the caret lands
        // on the correct visual (line, col) -- not just on the last
        // logical line as before.
        let (cline, ccol) = wrap_pos(&body, text_w as usize);
        let max_y = text_y + text_h.saturating_sub(1);
        let max_x = text_x + text_w.saturating_sub(1);
        let y = text_y + (cline as u16).min(text_h.saturating_sub(1));
        let x = text_x + (ccol as u16).min(text_w.saturating_sub(1));
        // only set if it lands inside the box
        if x <= max_x && y <= max_y {
            f.set_cursor_position((x, y));
        }
    }
}

/// Simulates ratatui's `Wrap { trim: false }` to find the (line, col)
/// of the position just after the last character of `text` when wrapped
/// at `width` columns. Treats '\n' as a hard line break and wraps any
/// other character when the current column reaches `width`.
fn wrap_pos(text: &str, width: usize) -> (usize, usize) {
    if width == 0 {
        return (0, 0);
    }
    let mut line = 0usize;
    let mut col = 0usize;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            col = 0;
            continue;
        }
        if col >= width {
            line += 1;
            col = 0;
        }
        col += 1;
    }
    (line, col)
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Width of the message prefix column (timestamp + nick + space).
/// Used to align continuation lines under the content, not the start
/// of the line. Generous enough for nicks up to ~12 chars
/// (`<boblabs> `, `<setup_user> `, etc.).
const CONTENT_INDENT_COLS: usize = 20;

fn render_message(m: &ChatMessage, nick: &str, highlight: Option<&str>) -> Vec<Line<'static>> {
    let (nick_str, nick_color) = match m.role {
        Role::You => (format!("<{}>", nick), YOU),
        Role::Bot => (format!("<{}>", BOT_NAME), BOT),
        Role::System => ("***".to_string(), SYSTEM),
        Role::Error => ("!ERR".to_string(), ERROR),
    };
    let prefix = vec![
        Span::styled(format!("[{}] ", m.time), Style::default().fg(DIM)),
        Span::styled(
            nick_str,
            Style::default().fg(nick_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];

    let content_lines = if matches!(m.role, Role::You | Role::Bot) {
        render_markdown(&m.content, highlight)
    } else {
        // System / error messages: split on \n so each visual line
        // becomes its own Line object. Otherwise the chat scroll math
        // sees one Line but the Paragraph renders many visual rows,
        // and PageUp/PageDown look like no-ops when the system text
        // overflows the visible area.
        m.content
            .lines()
            .map(|line| {
                if let Some(q) = highlight {
                    highlight_spans(line, q, INPUT, None)
                } else {
                    Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(INPUT),
                    ))
                }
            })
            .collect()
    };

    if content_lines.is_empty() {
        return vec![Line::from(prefix)];
    }

    // Indent every continuation line so it sits under the content
    // column, not column 0. Same applies to word-wrap continuations
    // -- the indent pad goes BEFORE the content so the wrap aligns.
    let indent_pad: String = " ".repeat(CONTENT_INDENT_COLS);
    let pad_span = |spans: &mut Vec<Span<'static>>| {
        // Prepend a single space pad span to the line. We don't want
        // a styled span -- the pad is invisible.
        spans.insert(
            0,
            Span::raw(indent_pad.clone()),
        );
    };

    let mut result: Vec<Line<'static>> = Vec::with_capacity(content_lines.len());
    let mut iter = content_lines.into_iter();
    if let Some(first) = iter.next() {
        let mut first_spans = prefix;
        first_spans.extend(first.spans);
        result.push(Line::from(first_spans));
    }
    for mut line in iter {
        // blank lines (e.g. between markdown paragraphs) stay blank
        let is_blank = line.spans.iter().all(|s| s.content.trim().is_empty());
        if !is_blank {
            pad_span(&mut line.spans);
        }
        result.push(line);
    }
    result
}

/// Highlight every case-insensitive occurrence of `query` inside `text`
/// by splitting it into a Vec of Spans, each match wrapped in the
/// `match_style`. Non-matching runs keep `base_style`. `inline_code_bg`,
/// if Some, is used as the background of non-match runs (so the
/// highlights stand out inside a code block).
fn highlight_spans(
    text: &str,
    query: &str,
    base_color: Color,
    inline_code_bg: Option<Color>,
) -> Line<'static> {
    let match_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Rgb(0xFF, 0xD5, 0x4D)) // strong amber bg
        .add_modifier(Modifier::BOLD);
    let base_style = if let Some(bg) = inline_code_bg {
        Style::default().fg(base_color).bg(bg)
    } else {
        Style::default().fg(base_color)
    };

    if query.is_empty() {
        return Line::from(Span::styled(text.to_string(), base_style));
    }

    let text_lower = text.to_lowercase();
    let q_lower = query.to_lowercase();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut start = 0usize;
    while let Some(rel) = text_lower[start..].find(&q_lower) {
        let abs = start + rel;
        if abs > start {
            spans.push(Span::styled(text[start..abs].to_string(), base_style));
        }
        let end = abs + q_lower.len();
        spans.push(Span::styled(
            text[abs..end].to_string(),
            match_style,
        ));
        start = end;
    }
    if start < text.len() {
        spans.push(Span::styled(text[start..].to_string(), base_style));
    }
    if spans.is_empty() {
        spans.push(Span::styled(text.to_string(), base_style));
    }
    Line::from(spans)
}

fn flush_line(output: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>) {
    if !current.is_empty() {
        output.push(Line::from(std::mem::take(current)));
    }
}

/// Push a blank line if the last line in `output` is non-empty (i.e. not
/// already a separator). Safe on empty `output` -- first call always
/// no-ops so we never panic on a leading heading/list/code/quote.
fn push_blank_if_needed(output: &mut Vec<Line<'static>>) {
    match output.last() {
        Some(line) if line.spans.is_empty() => {} // already a blank
        Some(_) => output.push(Line::from("")),
        None => {} // empty output, nothing to separate from
    }
}

fn render_markdown(text: &str, highlight: Option<&str>) -> Vec<Line<'static>> {
    // Render the markdown. If anything panics inside (e.g. an unexpected
    // event sequence from a malformed partial stream), fall back to a
    // single plain line so the TUI keeps running instead of dying.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_markdown_inner(text, highlight)
    }));
    match result {
        Ok(lines) => lines,
        Err(_) => vec![Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(INPUT),
        ))],
    }
}

fn render_markdown_inner(text: &str, highlight: Option<&str>) -> Vec<Line<'static>> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, options);

    let mut output: Vec<Line<'static>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut in_code_block = false;
    let mut code_buf = String::new();
    let mut bold = false;
    let mut italic = false;

    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                flush_line(&mut output, &mut current);
                output.push(Line::from(""));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush_line(&mut output, &mut current);
                push_blank_if_needed(&mut output);
                let prefix = match level {
                    HeadingLevel::H1 => "# ",
                    HeadingLevel::H2 => "## ",
                    HeadingLevel::H3 => "### ",
                    HeadingLevel::H4 => "#### ",
                    HeadingLevel::H5 => "##### ",
                    HeadingLevel::H6 => "###### ",
                };
                current.push(Span::styled(
                    prefix.to_string(),
                    Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
                ));
            }
            Event::End(TagEnd::Heading(_)) => {
                flush_line(&mut output, &mut current);
                output.push(Line::from(""));
            }
            Event::Start(Tag::BlockQuote) => {
                flush_line(&mut output, &mut current);
                push_blank_if_needed(&mut output);
            }
            Event::End(TagEnd::BlockQuote) => {
                flush_line(&mut output, &mut current);
                output.push(Line::from(""));
            }
            Event::Start(Tag::CodeBlock(_)) => {
                flush_line(&mut output, &mut current);
                push_blank_if_needed(&mut output);
                in_code_block = true;
                code_buf.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                for line in code_buf.lines() {
                    output.push(Line::from(Span::styled(
                        format!(" {}", line),
                        Style::default().fg(INPUT).bg(BG_HI),
                    )));
                }
                output.push(Line::from(""));
                in_code_block = false;
                code_buf.clear();
            }
            Event::Code(code) => {
                if in_code_block {
                    code_buf.push_str(&code);
                    code_buf.push('\n');
                } else if let Some(q) = highlight {
                    // inline code with a highlighted match: same wrap as
                    // `format!(" {} ", code)` but the match span keeps
                    // the strong yellow bg, the rest stays BG_HI.
                    let mut line = highlight_spans(&code, q, INPUT, Some(BG_HI));
                    if let Some(first) = line.spans.first_mut() {
                        let mut s = std::mem::take(&mut first.content).into_owned();
                        s.insert(0, ' ');
                        first.content = s.into();
                    }
                    if let Some(last) = line.spans.last_mut() {
                        let mut s = std::mem::take(&mut last.content).into_owned();
                        s.push(' ');
                        last.content = s.into();
                    }
                    for span in line.spans {
                        current.push(span);
                    }
                } else {
                    current.push(Span::styled(
                        format!(" {} ", code),
                        Style::default().fg(INPUT).bg(BG_HI),
                    ));
                }
            }
            Event::Text(text) => {
                if in_code_block {
                    code_buf.push_str(&text);
                } else if let Some(q) = highlight {
                    // replace this single text span with the highlighted
                    // version (one or more spans with matches marked)
                    for span in highlight_spans(&text, q, INPUT, None).spans {
                        // re-apply bold/italic from the current context
                        let mut s = span;
                        s.style = s.style.patch(match (bold, italic) {
                            (true, true) => Style::default()
                                .add_modifier(Modifier::BOLD | Modifier::ITALIC),
                            (true, false) => Style::default()
                                .add_modifier(Modifier::BOLD),
                            (false, true) => Style::default()
                                .add_modifier(Modifier::ITALIC),
                            _ => Style::default(),
                        });
                        current.push(s);
                    }
                } else {
                    let style = match (bold, italic) {
                        (true, true) => Style::default()
                            .fg(INPUT)
                            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
                        (true, false) => {
                            Style::default().fg(INPUT).add_modifier(Modifier::BOLD)
                        }
                        (false, true) => {
                            Style::default().fg(INPUT).add_modifier(Modifier::ITALIC)
                        }
                        _ => Style::default().fg(INPUT),
                    };
                    current.push(Span::styled(text.to_string(), style));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                flush_line(&mut output, &mut current);
            }
            Event::Start(Tag::List(start)) => {
                flush_line(&mut output, &mut current);
                push_blank_if_needed(&mut output);
                list_stack.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                flush_line(&mut output, &mut current);
                output.push(Line::from(""));
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                flush_line(&mut output, &mut current);
                let depth = list_stack.len().saturating_sub(1);
                let indent = "  ".repeat(depth);
                let marker = match list_stack.last() {
                    Some(Some(n)) => format!("{}{}. ", indent, n),
                    _ => format!("{}• ", indent),
                };
                current.push(Span::styled(marker, Style::default().fg(TITLE)));
            }
            Event::End(TagEnd::Item) => {
                flush_line(&mut output, &mut current);
            }
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Rule => {
                flush_line(&mut output, &mut current);
                output.push(Line::from(Span::styled(
                    "─────────",
                    Style::default().fg(DIM),
                )));
                output.push(Line::from(""));
            }
            _ => {}
        }
    }

    flush_line(&mut output, &mut current);

    // trim trailing blank lines
    while output.last().map_or(false, |l| l.spans.is_empty()) {
        output.pop();
    }

    output
}

fn draw_menu(f: &mut Frame, app: &App, area: Rect) {
    let list = filtered_commands(&app.input);
    if list.is_empty() {
        return;
    }
    let count = list.len();
    let display_idx = app.menu_idx.min(count - 1);

    // Anchor the menu at the bottom-left of the chat area so it sits
    // right above the input box. Width capped at 48 cols.
    let inner_h = (count as u16 + 2).max(3).min(area.height);
    let menu_w = 48u16.min(area.width.saturating_sub(4));
    let menu_x = area.x + 2;
    let menu_y = area.y + area.height.saturating_sub(inner_h + 1);
    let menu_area = Rect {
        x: menu_x,
        y: menu_y,
        width: menu_w,
        height: inner_h,
    };

    f.render_widget(Clear, menu_area);

    let items: Vec<ListItem> = list
        .iter()
        .enumerate()
        .map(|(i, cmd)| {
            let desc = COMMANDS
                .iter()
                .find(|(c, _)| *c == cmd.as_str())
                .map(|(_, d)| *d)
                .unwrap_or("");
            let prefix = if i == display_idx { "▶ " } else { "  " };
            let cmd_style = if i == display_idx {
                Style::default()
                    .fg(INPUT)
                    .bg(BG_SEL)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(INPUT)
            };
            let line = Line::from(vec![
                Span::styled(format!("{}{}", prefix, cmd), cmd_style),
                Span::styled(format!("  {}", desc), Style::default().fg(DIM)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let title = format!(" Commands ({} match{}) ", count, if count == 1 { "" } else { "es" });
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE))
        .title(Span::styled(
            title,
            Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
        ));
    f.render_widget(List::new(items).block(block), menu_area);
}

fn input_box<'a>(value: &'a str, active: bool, multiline: bool) -> Paragraph<'a> {
    let style = if active {
        Style::default().fg(INPUT).bg(BG_HI)
    } else {
        Style::default().fg(DIM)
    };
    let s = if value.is_empty() { "(empty)".to_string() } else { value.to_string() };
    let mut p = Paragraph::new(s).style(style);
    if multiline {
        p = p.wrap(Wrap { trim: false });
    }
    p
}

fn input_box_masked<'a>(value: &'a str, active: bool) -> Paragraph<'a> {
    let masked: String = std::iter::repeat('•').take(value.chars().count()).collect();
    let style = if active {
        Style::default().fg(INPUT).bg(BG_HI)
    } else {
        Style::default().fg(DIM)
    };
    let s = if value.is_empty() { "(empty)".to_string() } else { masked };
    Paragraph::new(s).style(style)
}

fn inset(r: Rect, n: u16) -> Rect {
    Rect {
        x: r.x + n,
        y: r.y + 1,
        width: r.width.saturating_sub(2 * n),
        height: r.height.saturating_sub(2),
    }
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{}…", cut)
    }
}

/// Sanitise a hostname for status bar / empty-state display so the
/// actual endpoint does not leak in screenshots. Localhost and private
/// IPs are kept as-is; domains keep their TLD and show only the first
/// and last character of the leftmost label; IPs keep the last octet.
fn mask_host(host: &str) -> String {
    if host.is_empty() {
        return String::new();
    }
    if host == "localhost" || host == "0.0.0.0" {
        return host.to_string();
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        if let Some(idx) = host.rfind('.') {
            return format!("***.{}", &host[idx + 1..]);
        }
        return "***".to_string();
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 2 {
        return "***".to_string();
    }
    let first = parts[0];
    let rest = parts[1..].join(".");
    let chars: Vec<char> = first.chars().collect();
    let masked = if chars.len() <= 2 {
        "**".to_string()
    } else {
        format!("{}***{}", chars[0], chars[chars.len() - 1])
    };
    format!("{}.{}", masked, rest)
}

/// Sanitise a full base URL. Scheme + path preserved, host passed
/// through `mask_host`, optional `:<port>` retained.
fn mask_url(url: &str) -> String {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
        ("https://", r)
    } else if let Some(r) = url.strip_prefix("http://") {
        ("http://", r)
    } else {
        return url.to_string();
    };
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (host, port) = match host_port.rfind(':') {
        Some(idx) => {
            let p = &host_port[idx + 1..];
            if p.parse::<u16>().is_ok() {
                (&host_port[..idx], Some(p))
            } else {
                (host_port, None)
            }
        }
        None => (host_port, None),
    };
    let port_str = port.map(|p| format!(":{}", p)).unwrap_or_default();
    format!("{}{}{}{}", scheme, mask_host(host), port_str, path)
}

const BOBLABS_BANNER: &[&str] = &[
    "####  ####  ####  #     ####  ####  ####",
    "#  #  #  #  #  #  #     #  #  #  #  #   ",
    "####  #  # ####  #     ####  ####  ####",
    "#  #  #  #  #  #  #     #  #  #  #     #",
    "####  #### ####  ####  #  # ####  ####",
];

fn draw_empty_state(f: &mut Frame, app: &App, area: Rect) {
    let mut rows: Vec<(String, Style)> = Vec::new();
    let can_banner = area.width >= 42;
    if can_banner {
        for line in BOBLABS_BANNER {
            rows.push((
                (*line).to_string(),
                Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
            ));
        }
        rows.push((String::new(), Style::default()));
    }
    rows.push((
        "10XTHINK personal assistant".to_string(),
        Style::default().fg(SYSTEM).add_modifier(Modifier::BOLD),
    ));
    rows.push((
        "────────────────────────────".to_string(),
        Style::default().fg(DIM),
    ));
    rows.push((String::new(), Style::default()));
    if !app.cfg.base_url.is_empty() {
        rows.push((
            format!("connected to {}", mask_url(&app.cfg.base_url)),
            Style::default().fg(INPUT),
        ));
    }
    if !app.cfg.model.is_empty() {
        rows.push((
            format!("using {} · nick {}", app.cfg.model, app.cfg.nick),
            Style::default().fg(DIM),
        ));
    }
    rows.push((String::new(), Style::default()));
    rows.push((
        "type / to see commands".to_string(),
        Style::default().fg(DIM),
    ));
    rows.push((
        "Shift+Enter = newline · Tab completes · ↑/↓ navigates".to_string(),
        Style::default().fg(DIM),
    ));

    let total = rows.len() as u16;
    let y_start = area.y + area.height.saturating_sub(total) / 2;
    for (i, (text, style)) in rows.iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let w = text.chars().count() as u16;
        let x = area.x + area.width.saturating_sub(w) / 2;
        let y = y_start + i as u16;
        if y >= area.y && y < area.y + area.height {
            let p = Paragraph::new(text.clone()).style(*style);
            f.render_widget(p, Rect { x, y, width: w, height: 1 });
        }
    }
}
