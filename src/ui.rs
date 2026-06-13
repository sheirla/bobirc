use crate::app::{App, ChatMessage, Role, Screen, SetupField};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

const TITLE: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;
const YOU: Color = Color::Green;
const BOT: Color = Color::Cyan;
const SYSTEM: Color = Color::Yellow;
const ERROR: Color = Color::Red;
const SEL: Color = Color::Magenta;
const INPUT: Color = Color::White;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // root vertical: top status (1) | middle (flex) | bottom input area (3)
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    draw_status(f, app, v[0]);

    match app.screen {
        Screen::Setup => draw_setup(f, app, v[1]),
        Screen::ModelSelect => draw_model_select(f, app, v[1]),
        Screen::Chat => draw_chat(f, app, v[1]),
    }

    draw_input(f, app, v[2]);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let screen = match app.screen {
        Screen::Setup => "SETUP",
        Screen::ModelSelect => "MODEL-SELECT",
        Screen::Chat => "CHAT",
    };
    let left = format!(
        " bobric v0.1 │ screen:{} │ server:{} │ nick:{} ",
        screen,
        short(&app.cfg.base_url, 32),
        app.cfg.nick,
    );
    let right = if !app.cfg.model.is_empty() {
        format!(" model:{} ", app.cfg.model)
    } else {
        " no model selected ".to_string()
    };
    let p = Paragraph::new(Line::from(vec![
        Span::styled(left, Style::default().fg(TITLE).add_modifier(Modifier::BOLD)),
        Span::styled("│", Style::default().fg(DIM)),
        Span::styled(right, Style::default().fg(BOT)),
    ]))
    .style(Style::default().bg(Color::Rgb(20, 20, 30)));
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

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(1),
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
        input_box(&app.setup_base, app.setup_field == SetupField::BaseUrl),
        rows[1],
    );

    f.render_widget(Paragraph::new(label("API Key", app.setup_field == SetupField::ApiKey)), rows[2]);
    f.render_widget(
        input_box_masked(&app.setup_key, app.setup_field == SetupField::ApiKey),
        rows[3],
    );

    f.render_widget(Paragraph::new(label("Nick", app.setup_field == SetupField::Nick)), rows[4]);
    f.render_widget(
        input_box(&app.setup_nick, app.setup_field == SetupField::Nick),
        rows[5],
    );

    let hint = Line::from(vec![
        Span::styled("Tab", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::raw(" switch field  "),
        Span::styled("Enter", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::raw(" save & continue  "),
        Span::styled("Esc", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::raw(" quit"),
    ]);
    f.render_widget(Paragraph::new(hint), rows[7]);

    if let Some(s) = &app.setup_status {
        let style = if s.starts_with("OK") {
            Style::default().fg(SYSTEM)
        } else {
            Style::default().fg(ERROR)
        };
        f.render_widget(Paragraph::new(Span::styled(s, style)), rows[6]);
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
        Span::styled("[T]", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::raw(" Test  "),
        Span::styled("[F]", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::raw(" Fetch  "),
        Span::styled("[Enter]", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::raw(" Select  "),
        Span::styled("[Esc]", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
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
                    Style::default().fg(INPUT).bg(Color::Rgb(40, 40, 60)).add_modifier(Modifier::BOLD)
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
        Span::styled("↑/↓", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::raw(" navigate  "),
        Span::styled("PgUp/PgDn", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::raw(" page"),
    ]);
    f.render_widget(Paragraph::new(help), rows[2]);
}

fn draw_chat(f: &mut Frame, app: &App, area: Rect) {
    // split: chat (left) | user list (right 18 cols)
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(18)])
        .split(area);

    draw_chat_main(f, app, h[0]);
    draw_user_list(f, app, h[1]);
}

fn draw_chat_main(f: &mut Frame, app: &App, area: Rect) {
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

    let mut lines: Vec<Line<'static>> = Vec::new();
    for m in &app.messages {
        lines.push(styled_line(m, nick));
    }
    if app.streaming || !app.stream_buf.is_empty() {
        let content = if !app.streaming && app.stream_buf.is_empty() {
            "…".to_string()
        } else {
            app.stream_buf.clone()
        };
        let bot_msg = ChatMessage {
            role: Role::Bot,
            content,
            time: crate::app::now(),
        };
        lines.push(styled_line(&bot_msg, nick));
    }

    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll, 0));
    f.render_widget(p, inner);
}

fn styled_line(m: &ChatMessage, nick: &str) -> Line<'static> {
    let (nick_str, nick_color) = match m.role {
        Role::You => (format!("<{}>", nick), YOU),
        Role::Bot => ("<Bot>".to_string(), BOT),
        Role::System => ("***".to_string(), SYSTEM),
        Role::Error => ("!ERR".to_string(), ERROR),
    };
    Line::from(vec![
        Span::styled(format!("[{}] ", m.time), Style::default().fg(DIM)),
        Span::styled(
            nick_str,
            Style::default().fg(nick_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(m.content.clone(), Style::default().fg(INPUT)),
    ])
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
    let bot_state = if app.streaming { " (typing...)" } else { "" };
    items.push(ListItem::new(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("Bot", Style::default().fg(BOT).add_modifier(Modifier::BOLD)),
        Span::styled(bot_state, Style::default().fg(DIM)),
    ])));

    f.render_widget(List::new(items), inset(area, 1));
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(TITLE));

    let (title, body) = match app.screen {
        Screen::Chat => {
            if app.streaming {
                (" Bot is typing… ", String::new())
            } else {
                let prompt = format!("> {}", app.input);
                (" Input ", prompt)
            }
        }
        Screen::Setup => (" Setup (Tab to switch, Enter to save) ", String::new()),
        Screen::ModelSelect => (" Model Select (F fetch, T test, Enter select) ", String::new()),
    };

    let p = Paragraph::new(body)
        .block(
            block.title(Span::styled(
                title,
                Style::default().fg(TITLE).add_modifier(Modifier::BOLD),
            )),
        )
        .style(Style::default().fg(INPUT));
    f.render_widget(p, inset(area, 0));

    if !matches!(app.screen, Screen::Chat) {
        // hide the global prompt
    }

    // caret
    if app.screen == Screen::Chat && !app.streaming {
        let inner = inset(area, 1);
        let x = inner.x + 2 + app.input.chars().count() as u16;
        let y = inner.y;
        if x < inner.x + inner.width {
            f.set_cursor_position((x, y));
        }
    }
}

fn input_box<'a>(value: &'a str, active: bool) -> Paragraph<'a> {
    let style = if active {
        Style::default().fg(INPUT).bg(Color::Rgb(30, 30, 50))
    } else {
        Style::default().fg(DIM)
    };
    let s = if value.is_empty() { "(empty)".to_string() } else { value.to_string() };
    Paragraph::new(s).style(style)
}

fn input_box_masked<'a>(value: &'a str, active: bool) -> Paragraph<'a> {
    let masked: String = std::iter::repeat('•').take(value.chars().count()).collect();
    let style = if active {
        Style::default().fg(INPUT).bg(Color::Rgb(30, 30, 50))
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

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_w = r.width * percent_x / 100;
    let popup_h = r.height * percent_y / 100;
    let x = r.x + (r.width - popup_w) / 2;
    let y = r.y + (r.height - popup_h) / 2;
    Rect { x, y, width: popup_w, height: popup_h }
}

#[allow(dead_code)]
fn _render_loading(f: &mut Frame, msg: &str) {
    let area = centered_rect(40, 20, f.area());
    f.render_widget(Clear, area);
    let p = Paragraph::new(msg)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(TITLE))
                .title(Span::styled(" bobric ", Style::default().fg(TITLE))),
        )
        .style(Style::default().fg(INPUT));
    f.render_widget(p, area);
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{}…", cut)
    }
}
