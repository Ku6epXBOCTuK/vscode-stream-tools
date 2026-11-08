use std::sync::Arc;
use std::sync::nonpoison::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::chat::{ChatMessage, KIND_CHAT};
use crate::config::{Commands, TrackCommand};

pub type NowPlayingSource = Arc<dyn Fn() -> Option<String> + Send + Sync>;

pub struct Dispatcher {
    track: TrackCommand,
    now_playing: NowPlayingSource,
    last_reply: Mutex<Option<Instant>>,
}

impl Dispatcher {
    pub fn new(commands: &Commands, now_playing: NowPlayingSource) -> Self {
        Self {
            track: commands.track.clone(),
            now_playing,
            last_reply: Mutex::new(None),
        }
    }

    pub fn handle(&self, message: &ChatMessage, me: &str) -> Option<String> {
        if !self.track.enabled || message.kind != KIND_CHAT {
            return None;
        }
        if !message.channel.eq_ignore_ascii_case(me) {
            log::debug!(
                "track command from {} ignored: #{} is not the signed-in channel",
                message.user.login,
                message.channel
            );
            return None;
        }
        if message.user.login.eq_ignore_ascii_case(me) {
            log::debug!("track command from the broadcaster ignored");
            return None;
        }

        let text = message.text.trim();
        let is_alias = self
            .track
            .aliases
            .iter()
            .any(|alias| !alias.trim().is_empty() && text.eq_ignore_ascii_case(alias.trim()));
        if !is_alias {
            return None;
        }

        if let Some(left) = self.cooldown_left() {
            log::info!(
                "track command from {} refused: {}s of cooldown left",
                message.user.login,
                left.as_secs()
            );
            return None;
        }

        let Some(track) = (self.now_playing)().filter(|track| !track.trim().is_empty()) else {
            log::info!(
                "track command from {} refused: nothing is playing",
                message.user.login
            );
            return None;
        };

        *self.lock_replies() = Some(Instant::now());
        let template = if self.track.template.trim().is_empty() {
            "{track}"
        } else {
            &self.track.template
        };
        let reply = template.replace("{track}", track.trim());
        log::info!(
            "track command from {} answered: {reply}",
            message.user.login
        );
        Some(reply)
    }

    fn cooldown_left(&self) -> Option<Duration> {
        let cooldown = Duration::from_secs(self.track.cooldown_seconds);
        let last = *self.lock_replies();
        last.and_then(|last| cooldown.checked_sub(last.elapsed()))
    }

    fn lock_replies(&self) -> MutexGuard<'_, Option<Instant>> {
        self.last_reply.lock()
    }
}

pub(crate) async fn reply(
    state: &std::sync::Arc<crate::ChatState>,
    text: String,
    reply_to: Option<String>,
) {
    match crate::api::send_text(state, &text, reply_to.as_deref()).await {
        Ok(()) => log::info!("track command reply sent"),
        Err(err) => log::warn!("track command reply failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{ChatMessage, User};

    fn message(channel: &str, login: &str, text: &str) -> ChatMessage {
        ChatMessage::new(
            channel,
            KIND_CHAT,
            User {
                login: login.to_string(),
                name: login.to_string(),
                ..User::default()
            },
            text.to_string(),
        )
    }

    fn dispatcher(commands: TrackCommand, track: Option<&'static str>) -> Dispatcher {
        Dispatcher::new(
            &Commands { track: commands },
            Arc::new(move || track.map(str::to_string)),
        )
    }

    #[test]
    fn an_alias_gets_the_track_in_the_template() {
        let dispatcher = dispatcher(TrackCommand::default(), Some("Kavinsky - Nightcall"));
        let reply = dispatcher.handle(&message("streamer", "viewer", "!трек"), "streamer");
        assert_eq!(
            reply.as_deref(),
            Some("Сейчас играет: Kavinsky - Nightcall")
        );
    }

    #[test]
    fn the_alias_is_case_insensitive_and_trimmed() {
        let dispatcher = dispatcher(TrackCommand::default(), Some("Song"));
        assert_eq!(
            dispatcher
                .handle(&message("streamer", "viewer", "  !TRACK "), "streamer")
                .as_deref(),
            Some("Сейчас играет: Song")
        );
    }

    #[test]
    fn a_second_command_waits_for_the_cooldown() {
        let commands = TrackCommand {
            cooldown_seconds: 60,
            ..TrackCommand::default()
        };
        let dispatcher = dispatcher(commands, Some("Song"));
        assert!(
            dispatcher
                .handle(&message("streamer", "viewer", "!трек"), "streamer")
                .is_some()
        );
        assert!(
            dispatcher
                .handle(&message("streamer", "viewer", "!трек"), "streamer")
                .is_none()
        );
    }

    #[test]
    fn a_zero_cooldown_answers_every_time() {
        let commands = TrackCommand {
            cooldown_seconds: 0,
            ..TrackCommand::default()
        };
        let dispatcher = dispatcher(commands, Some("Song"));
        for _ in 0..3 {
            assert!(
                dispatcher
                    .handle(&message("streamer", "viewer", "!трек"), "streamer")
                    .is_some()
            );
        }
    }

    #[test]
    fn the_broadcaster_is_not_answered() {
        let dispatcher = dispatcher(TrackCommand::default(), Some("Song"));
        assert!(
            dispatcher
                .handle(&message("streamer", "streamer", "!трек"), "streamer")
                .is_none()
        );
    }

    #[test]
    fn another_channel_is_not_answered() {
        let dispatcher = dispatcher(TrackCommand::default(), Some("Song"));
        assert!(
            dispatcher
                .handle(&message("someone_else", "viewer", "!трек"), "streamer")
                .is_none()
        );
    }

    #[test]
    fn nothing_playing_means_no_reply() {
        let dispatcher = dispatcher(TrackCommand::default(), None);
        assert!(
            dispatcher
                .handle(&message("streamer", "viewer", "!трек"), "streamer")
                .is_none()
        );
    }

    #[test]
    fn unknown_text_is_ignored() {
        let dispatcher = dispatcher(TrackCommand::default(), Some("Song"));
        assert!(
            dispatcher
                .handle(&message("streamer", "viewer", "!трек please"), "streamer")
                .is_none()
        );
    }

    #[test]
    fn a_disabled_command_says_nothing() {
        let commands = TrackCommand {
            enabled: false,
            ..TrackCommand::default()
        };
        let dispatcher = dispatcher(commands, Some("Song"));
        assert!(
            dispatcher
                .handle(&message("streamer", "viewer", "!трек"), "streamer")
                .is_none()
        );
    }

    #[test]
    fn custom_aliases_and_template_are_used() {
        let commands = TrackCommand {
            aliases: vec!["!now".to_string()],
            template: "{track} is live".to_string(),
            ..TrackCommand::default()
        };
        let dispatcher = dispatcher(commands, Some("Song"));
        assert!(
            dispatcher
                .handle(&message("streamer", "viewer", "!трек"), "streamer")
                .is_none()
        );
        assert_eq!(
            dispatcher
                .handle(&message("streamer", "viewer", "!now"), "streamer")
                .as_deref(),
            Some("Song is live")
        );
    }
}
