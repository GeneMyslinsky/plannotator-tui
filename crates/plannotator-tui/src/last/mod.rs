//! `plannotator-tui last`: review an agent's recent conversation (docs/spec-last-message.md).
//!
//! `locate` is the one impure step (home directory, process table, transcript files); the
//! rest is the ordinary app over a transient document that is never written to disk.

#![allow(clippy::print_stdout, clippy::print_stderr, reason = "`--print` is a stdout contract")]

mod locate;

use std::io::Read as _;
use std::path::PathBuf;

use anyhow::{Context, Result};
use plannotator_tui_hosts::{Message, Role};
use plannotator_tui_schema::{DocumentSource, Provenance};

use crate::app::App;
use crate::cli;

/// What `last` was asked to open. Every field optional; detection fills the gaps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LastOptions {
    /// A supported host label, else detected from the environment.
    pub(crate) host: Option<String>,
    /// The agent process to start the transcript search from.
    pub(crate) pid: Option<u32>,
    /// An explicit transcript; skips detection.
    pub(crate) session: Option<PathBuf>,
    /// Read the document from stdin instead of a transcript.
    pub(crate) stdin: bool,
    /// Print the newest message and exit instead of opening the UI.
    pub(crate) print: bool,
    /// How many recent messages the picker offers.
    pub(crate) pick: usize,
}

pub(crate) fn run(options: &LastOptions) -> Result<()> {
    if options.stdin {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text).context("reading stdin")?;
        if options.print {
            print!("{text}");
            return Ok(());
        }
        let source = DocumentSource::new(text, "stdin · message", true, Provenance::Stdin);
        return cli::run_ui(|width| App::open(source, width, cli::delivery(true)));
    }
    let located = match locate::locate(options) {
        Ok(located) => located,
        // The delivery contract: a script or hook must never be aborted by us.
        Err(err) if options.print => {
            eprintln!("plannotator-tui last: {err:#}");
            return Ok(());
        }
        // Inside Herdr we can still show what the agent printed, whatever it is.
        Err(err) => {
            let Some(screen) = locate::screen_fallback(&crate::herdr::context::HerdrEnv::from_env()) else {
                return Err(err);
            };
            let note = format!("{err:#} — showing the pane's recent output instead");
            return cli::run_ui(|width| {
                let mut app = App::open(screen, width, cli::delivery(true))?;
                app.set_status(note.clone());
                Ok(app)
            });
        }
    };
    if options.print {
        let Some(newest) = newest_assistant(&located.messages) else {
            eprintln!(
                "plannotator-tui last: transcript {} has no assistant messages yet",
                located.transcript.display()
            );
            return Ok(());
        };
        println!("{}", newest.text);
        return Ok(());
    }
    let label = located.host.label();
    let transcript = located.transcript.display().to_string();
    let messages = located.messages;
    cli::run_ui(|width| App::open_message(label, &transcript, messages, width, cli::delivery(true)))
}

/// The newest assistant candidate, for the `--print` compatibility contract.
fn newest_assistant(messages: &[Message]) -> Option<&Message> {
    messages.iter().find(|message| message.role == Role::Assistant)
}

fn message_heading(role: Role) -> &'static str {
    match role {
        Role::Human => "## You\n\n",
        Role::Assistant => "## Assistant\n\n",
    }
}

/// Selected conversation messages as one transient review document.
///
/// `messages` are candidates in newest-first order; the document restores chronological order.
pub(crate) fn messages_source(host: &str, transcript: &str, messages: &[Message]) -> DocumentSource {
    let capacity = messages
        .iter()
        .map(|message| message_heading(message.role).len() + message.text.len())
        .sum::<usize>()
        + messages.len().saturating_sub(1).saturating_mul(2);
    let mut content = String::with_capacity(capacity);
    for message in messages.iter().rev() {
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(message_heading(message.role));
        content.push_str(&message.text);
    }
    DocumentSource::new(
        content,
        format!("{host} · {} messages", messages.len()),
        true,
        Provenance::AgentMessage {
            host: host.to_owned(),
            session: Some(transcript.to_owned()),
            message_id: None,
        },
    )
}

/// A message as a document: transient, provenance names the host, transcript and message.
pub(crate) fn message_source(host: &str, transcript: &str, message: &Message) -> DocumentSource {
    DocumentSource::new(
        message.text.clone(),
        format!("{host} · last message"),
        true,
        Provenance::AgentMessage {
            host: host.to_owned(),
            session: Some(transcript.to_owned()),
            message_id: Some(message.id.clone()),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{messages_source, newest_assistant};
    use plannotator_tui_hosts::{Message, Role};
    use plannotator_tui_schema::Provenance;

    fn message(id: &str, role: Role, text: &str) -> Message {
        Message { id: id.to_owned(), role, text: text.to_owned(), at: None }
    }

    #[test]
    fn newest_assistant_skips_newer_human_candidates() {
        let messages = [
            message("u2", Role::Human, "newer prompt"),
            message("a1", Role::Assistant, "newest assistant reply"),
        ];

        assert_eq!(
            newest_assistant(&messages).map(|message| message.text.as_str()),
            Some("newest assistant reply")
        );
    }

    #[test]
    fn messages_source_reverses_selected_candidates_into_a_chronological_review() {
        let messages = [
            message("a2", Role::Assistant, "latest reply"),
            message("u2", Role::Human, "follow-up"),
            message("a1", Role::Assistant, "initial reply"),
            message("u1", Role::Human, "initial prompt"),
        ];

        let source = messages_source("claude", "/tmp/transcript.jsonl", &messages);

        assert_eq!(
            source.content,
            "## You\n\ninitial prompt\n\n## Assistant\n\ninitial reply\n\n## You\n\nfollow-up\n\n## Assistant\n\nlatest reply"
        );
        assert_eq!(source.name, "claude · 4 messages");
        assert!(source.transient);
        assert_eq!(
            source.provenance,
            Provenance::AgentMessage {
                host: "claude".to_owned(),
                session: Some("/tmp/transcript.jsonl".to_owned()),
                message_id: None,
            }
        );
    }
}
