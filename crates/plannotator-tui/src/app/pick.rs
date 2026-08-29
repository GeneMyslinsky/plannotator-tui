//! The message picker: choose one or more of the agent's recent messages, newest first. The
//! newest assistant (or newest candidate when none exists) is already open behind it.

use anyhow::Result;
use plannotator_tui_hosts::{Message, Role};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr as _;

use super::{App, Mode, Open};
use crate::last::{message_source, messages_source};

const PICK_MAX_WIDTH: u16 = 90;
const PICK_EMPTY_STATUS: &str = "select at least one message before opening";

impl App {
    /// Open with `messages` (newest first) as candidates; the picker shows when there is a
    /// choice to make.
    pub(crate) fn open_message(
        host: &str,
        transcript: &str,
        messages: Vec<Message>,
        width: usize,
        delivery: Box<dyn crate::delivery::Delivery>,
    ) -> Result<Self> {
        let Some(newest) =
            messages.iter().find(|message| message.role == Role::Assistant).or(messages.first())
        else {
            anyhow::bail!("no message to open");
        };
        let mut app = Self::open(message_source(host, transcript, newest), width, delivery)?;
        host.clone_into(&mut app.message_host);
        transcript.clone_into(&mut app.message_transcript);
        app.pick_selected = vec![false; messages.len()];
        app.candidates = messages;
        if app.candidates.len() > 1 {
            app.mode = Mode::Pick;
        }
        Ok(app)
    }

    /// Swap the open document for the message or messages selected in the picker.
    fn open_selected(&mut self) -> Result<()> {
        let selected_count = self.pick_selected.iter().filter(|selected| **selected).count();
        if selected_count == 0 {
            self.status = Some(PICK_EMPTY_STATUS.into());
            return Ok(());
        }

        let (source, status) = if selected_count == 1 {
            let Some(index) = self.pick_selected.iter().position(|selected| *selected) else { return Ok(()) };
            let Some(message) = self.candidates.get(index) else { return Ok(()) };
            (
                message_source(&self.message_host, &self.message_transcript, message),
                format!("message {} of {}", index + 1, self.candidates.len()),
            )
        } else {
            let messages: Vec<Message> = self
                .candidates
                .iter()
                .zip(&self.pick_selected)
                .filter_map(|(message, selected)| (*selected).then(|| message.clone()))
                .collect();
            (
                messages_source(&self.message_host, &self.message_transcript, &messages),
                format!("{selected_count} messages selected"),
            )
        };
        self.open = Open::new(source, self.open.layout.width, &self.data_dir, &self.project)?;
        self.scroll = 0;
        self.selected = 0;
        self.cursor = (0, 0);
        self.rail_cursor = 0;
        self.clear_selection();
        self.derive_send_state();
        self.mode = Mode::Browse;
        self.status = Some(status);
        Ok(())
    }

    fn toggle_pick(&mut self, index: usize) {
        if let Some(selected) = self.pick_selected.get_mut(index) {
            *selected = !*selected;
            self.status = None;
        }
    }

    pub(super) fn reopen_picker(&mut self) {
        if self.candidates.len() > 1 {
            self.mode = Mode::Pick;
        }
    }

    pub(super) fn pick_key(&mut self, key: KeyEvent) -> Result<()> {
        let last = self.candidates.len().saturating_sub(1);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.pick_cursor = (self.pick_cursor + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up => self.pick_cursor = self.pick_cursor.saturating_sub(1),
            KeyCode::Char(' ') => self.toggle_pick(self.pick_cursor),
            KeyCode::Enter => return self.open_selected(),
            KeyCode::Esc => {
                self.status = None;
                self.mode = Mode::Browse;
            }
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
        Ok(())
    }

    pub(super) fn pick_mouse(&mut self, mouse: MouseEvent) -> Result<()> {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return Ok(());
        }
        let hit = self
            .geometry
            .pick_rows
            .iter()
            .find(|(rect, _)| mouse.row == rect.y && mouse.column >= rect.x && mouse.column < rect.right())
            .map(|(_, index)| *index);
        if let Some(index) = hit {
            self.pick_cursor = index;
            self.toggle_pick(index);
        }
        Ok(())
    }

    pub(super) fn draw_pick(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let width = PICK_MAX_WIDTH.min(area.width.saturating_sub(4)).max(20);
        let rows = self.candidates.len() as u16;
        let height = (rows + 2).min(area.height.saturating_sub(2)).max(3);
        let rect = Rect { x: (area.width - width) / 2, y: (area.height - height) / 2, width, height };
        frame.render_widget(Clear, rect);
        let boxed = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::Cyan))
            .title(Span::styled(
                if self.status.as_deref() == Some(PICK_EMPTY_STATUS) {
                    PICK_EMPTY_STATUS
                } else {
                    " select one or more messages "
                },
                Style::new().dim(),
            ))
            .title_bottom(Span::styled(
                format!(
                    " space toggle · enter review · esc newest asst · q quit · {} selected ",
                    self.pick_selected.iter().filter(|selected| **selected).count()
                ),
                Style::new().dim(),
            ));
        let inner = boxed.inner(rect);
        frame.render_widget(boxed, rect);
        let mut pick_rows = Vec::new();
        let lines: Vec<Line<'static>> = self
            .candidates
            .iter()
            .enumerate()
            .take(usize::from(inner.height))
            .map(|(index, message)| {
                let row = Rect { x: inner.x, y: inner.y + index as u16, width: inner.width, height: 1 };
                pick_rows.push((row, index));
                let selected = self.pick_selected.get(index).copied().unwrap_or(false);
                let checkbox = if selected { "[x]" } else { "[ ]" };
                let (role, role_style) = match message.role {
                    Role::Human => ("You", Style::new().fg(Color::Yellow)),
                    Role::Assistant => ("Assistant", Style::new().fg(Color::Cyan)),
                };
                let style = if index == self.pick_cursor { Style::new().reversed() } else { Style::new() };
                let role_style = if index == self.pick_cursor { role_style.reversed() } else { role_style };
                let width = usize::from(inner.width).saturating_sub(6 + role.width());
                let text = fit(&pick_label(message), width);
                Line::from(vec![
                    Span::styled(format!(" {checkbox} "), style),
                    Span::styled(role, role_style),
                    Span::styled(format!(" {text}"), style),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        self.geometry.pick_rows = pick_rows;
    }
}

/// `HH:MM  first line of the message`.
fn pick_label(message: &Message) -> String {
    let time = message.at.as_deref().and_then(clock).unwrap_or_else(|| "     ".to_owned());
    let first = message.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    format!("{time}  {first}")
}

/// `HH:MM` out of an RFC 3339 timestamp; anything else is left blank.
fn clock(at: &str) -> Option<String> {
    let time = at.get(11..16)?;
    (time.len() == 5 && time.as_bytes().get(2) == Some(&b':')).then(|| time.to_owned())
}

fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    for ch in text.chars() {
        if out.width() + 1 >= width {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}
