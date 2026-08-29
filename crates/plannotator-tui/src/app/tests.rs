//! Behaviour of the header's Send button and the quit confirmation, drawn into a
//! `TestBackend` the way the `--snapshot` CLI does.

#![allow(clippy::expect_used, clippy::indexing_slicing, reason = "tests assert by panicking")]

use std::path::PathBuf;

use plannotator_tui_hosts::{Message, Role};
use plannotator_tui_schema::{DocumentSource, Kind, Provenance};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::style::{Color, Modifier};

use super::send::SendState;
use super::{App, Mode};
use crate::delivery::{Delivery, Discard, HerdrAgent};

/// A transient source: the app runs exactly as it does on a file, but nothing is written
/// to the Plannotator data directory.
fn app(delivery: Box<dyn Delivery>) -> App {
    let source =
        DocumentSource::new("# Plan\n\nfirst thing\n".to_owned(), "plan.md", true, Provenance::Stdin);
    App::open(source, 60, delivery).expect("app opens")
}

fn agent() -> Box<dyn Delivery> {
    Box::new(HerdrAgent::new(PathBuf::from("/nonexistent/herdr"), "w1:p1".into(), Some("claude".into())))
}

/// One frame drawn into the same in-memory terminal the `--snapshot` CLI uses.
fn draw_buffer(app: &mut App) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");
    terminal.draw(|frame| app.draw(frame)).expect("draw");
    terminal.backend().buffer().clone()
}

/// One frame, as one string per screen row.
fn draw(app: &mut App) -> Vec<String> {
    let buffer = draw_buffer(app);
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .filter_map(|x| buffer.cell((x, y)))
                .map(|cell| cell.symbol().to_owned())
                .collect()
        })
        .collect()
}

fn cell_for_text<'a>(buffer: &'a Buffer, text: &str) -> Option<&'a ratatui::buffer::Cell> {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let rendered: String = (x..buffer.area.width)
                .filter_map(|column| buffer.cell((column, y)))
                .map(|cell| cell.symbol())
                .collect();
            if rendered.starts_with(text) {
                return buffer.cell((x, y));
            }
        }
    }
    None
}

fn row(rows: &[String], index: usize) -> &str {
    rows.get(index).map_or("", String::as_str)
}

#[test]
fn the_header_draws_the_send_button_and_records_where_it_is() {
    let mut app = app(agent());
    app.add_block_annotation(0, Kind::Comment, "x".to_owned()).expect("annotation");
    let rows = draw(&mut app);
    let header = row(&rows, 0);
    assert!(header.contains("Send 1 to claude in w1:p1 ▸"), "header was {header:?}");
    let rect = app.geometry.send_button.expect("button rect recorded");
    assert_eq!(rect.y, 0);
    assert_eq!(rect.right(), 80, "the button sits on the right edge");
}

#[test]
fn clicking_the_send_button_sends() {
    let mut app = app(Box::new(Discard));
    app.add_block_annotation(0, Kind::Comment, "x".to_owned()).expect("annotation");
    draw(&mut app);
    let rect = app.geometry.send_button.expect("button rect recorded");
    let click = Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x + rect.width / 2,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_event(&click).expect("click");
    assert_eq!(app.send_state, SendState::Sent);
}

#[test]
fn quitting_with_unsent_feedback_asks_before_it_quits() {
    let mut app = app(agent());
    app.add_block_annotation(0, Kind::Comment, "x".to_owned()).expect("annotation");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char('q')))).expect("q");
    assert_eq!(app.mode, Mode::ConfirmQuit);
    assert!(!app.quit, "the question is asked instead of quitting");
    let rows = draw(&mut app);
    let footer = row(&rows, 19);
    assert!(footer.contains("before quitting? y send · n quit · esc cancel"), "footer was {footer:?}");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char('n')))).expect("n");
    assert!(app.quit, "n quits without sending");
    assert_eq!(app.send_state, SendState::Ready, "nothing was sent");
}

fn candidates() -> Vec<Message> {
    let message = |id: &str, role: Role, text: &str, at: &str| Message {
        id: id.to_owned(),
        role,
        text: text.to_owned(),
        at: Some(at.to_owned()),
    };
    vec![
        message(
            "m4",
            Role::Human,
            "Newest request\n\nPlease review this change.\n",
            "2026-08-28T12:41:00.000Z",
        ),
        message(
            "m3",
            Role::Assistant,
            "# Current reply\n\nHere is the latest response.\n",
            "2026-08-28T12:40:00.000Z",
        ),
        message(
            "m2",
            Role::Human,
            "Earlier request\n\nPlease keep the context.\n",
            "2026-08-28T12:38:00.000Z",
        ),
        message("m1", Role::Assistant, "# Earlier reply\n\nAn older response.\n", "2026-08-28T12:30:00.000Z"),
    ]
}

#[test]
fn the_picker_renders_roles_and_multiselect_controls() {
    let mut app = App::open_message("claude", "/tmp/transcript.jsonl", candidates(), 60, Box::new(Discard))
        .expect("opens");
    assert_eq!(app.mode, Mode::Pick, "more than one candidate asks which");
    assert_eq!(app.open.doc.source, "# Current reply\n\nHere is the latest response.\n");
    assert_eq!(app.pick_selected, vec![false; 4]);

    let rows = draw(&mut app);
    let listed: Vec<&str> = rows.iter().map(String::as_str).filter(|row| row.contains("[ ]")).collect();
    assert_eq!(listed.len(), 4, "{rows:?}");
    assert!(listed[0].contains("[ ]"), "{:?}", listed[0]);
    assert!(listed[0].contains("You"), "{:?}", listed[0]);
    assert!(listed[0].contains("12:41"), "{:?}", listed[0]);
    assert!(listed[0].contains("Newest request"), "{:?}", listed[0]);
    assert!(listed[1].contains("[ ]"), "{:?}", listed[1]);
    assert!(listed[1].contains("Assistant"), "{:?}", listed[1]);
    assert!(listed[1].contains("12:40"), "{:?}", listed[1]);
    assert!(listed[1].contains("# Current reply"), "{:?}", listed[1]);
    assert!(rows.iter().any(|row| row.contains("space toggle") && row.contains("0 selected")), "{rows:?}");

    let buffer = draw_buffer(&mut app);
    let you = cell_for_text(&buffer, "You").expect("You label").style();
    let assistant = cell_for_text(&buffer, "Assistant").expect("Assistant label").style();
    assert_eq!(you.fg, Some(Color::Yellow));
    assert_eq!(assistant.fg, Some(Color::Cyan));
    assert!(you.add_modifier.contains(Modifier::REVERSED), "the picker cursor stays reversed");

    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char(' ')))).expect("space");
    assert_eq!(app.pick_selected, vec![true, false, false, false]);
    let rows = draw(&mut app);
    assert!(rows.iter().any(|row| row.contains("[x] You")), "{rows:?}");
    assert!(rows.iter().any(|row| row.contains("1 selected")), "{rows:?}");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char(' ')))).expect("space toggles off");
    assert_eq!(app.pick_selected, vec![false; 4]);

    draw(&mut app);
    let clicked = app.geometry.pick_rows[1].0;
    app.handle_event(&Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: clicked.x,
        row: clicked.y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("click");
    assert_eq!(app.pick_cursor, 1);
    assert_eq!(app.pick_selected, vec![false, true, false, false]);
    let buffer = draw_buffer(&mut app);
    assert!(
        cell_for_text(&buffer, "Assistant")
            .expect("Assistant label")
            .style()
            .add_modifier
            .contains(Modifier::REVERSED),
        "clicking moves the reversed cursor"
    );
}

#[test]
fn entering_without_a_selection_stays_in_the_picker() {
    let mut app = App::open_message("claude", "/tmp/transcript.jsonl", candidates(), 60, Box::new(Discard))
        .expect("opens");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Enter))).expect("enter");
    assert_eq!(app.mode, Mode::Pick);
    assert_eq!(app.status.as_deref(), Some("select at least one message before opening"));
    let rows = draw(&mut app);
    assert!(rows.iter().any(|row| row.contains("select at least one message before opening")), "{rows:?}");
}

#[test]
fn selecting_multiple_messages_opens_chronological_transient_conversation() {
    let mut app = App::open_message("claude", "/tmp/transcript.jsonl", candidates(), 60, Box::new(Discard))
        .expect("opens");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char(' ')))).expect("select newest");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char('j')))).expect("next");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char(' ')))).expect("select assistant");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Enter))).expect("enter");

    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.open.source.name, "claude · 2 messages");
    assert!(app.open.store.is_transient(), "a composite message is never written to disk");
    let assistant = app.open.doc.source.find("## Assistant\n\n# Current reply").expect("assistant heading");
    let human = app.open.doc.source.find("## You\n\nNewest request").expect("human heading");
    assert!(assistant < human, "selected candidates are restored chronologically");
    match &app.open.source.provenance {
        Provenance::AgentMessage { host, session, message_id } => {
            assert_eq!(host, "claude");
            assert_eq!(session.as_deref(), Some("/tmp/transcript.jsonl"));
            assert!(message_id.is_none());
        }
        provenance => panic!("unexpected provenance: {provenance:?}"),
    }
}

#[test]
fn selecting_one_message_opens_its_raw_transient_source() {
    let mut app = App::open_message("claude", "/tmp/transcript.jsonl", candidates(), 60, Box::new(Discard))
        .expect("opens");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char(' ')))).expect("select human");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Enter))).expect("enter");

    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.open.source.name, "claude · last message");
    assert_eq!(app.open.doc.source, "Newest request\n\nPlease review this change.\n");
    assert!(app.open.store.is_transient(), "a message is never written to disk");
    match &app.open.source.provenance {
        Provenance::AgentMessage { message_id, .. } => assert_eq!(message_id.as_deref(), Some("m4")),
        provenance => panic!("unexpected provenance: {provenance:?}"),
    }
}

#[test]
fn escaping_the_picker_keeps_the_newest_assistant_message() {
    let mut app = App::open_message("claude", "/tmp/transcript.jsonl", candidates(), 60, Box::new(Discard))
        .expect("opens");
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Esc))).expect("esc");
    assert_eq!(app.mode, Mode::Browse);
    assert_eq!(app.open.doc.source, "# Current reply\n\nHere is the latest response.\n");
    assert_eq!(app.open.source.name, "claude · last message");
    assert!(app.open.store.is_transient());
    app.handle_event(&Event::Key(KeyEvent::from(KeyCode::Char('p')))).expect("p");
    assert_eq!(app.mode, Mode::Pick, "p reopens the picker");
}
