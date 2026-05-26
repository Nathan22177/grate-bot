use super::canvas::CanvasPreset;
use rand::{seq::SliceRandom, thread_rng};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct State {
    pub(crate) games: Arc<RwLock<HashMap<GameKey, Game>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameKey {
    pub guild_id: u64,
    pub channel_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasConfig {
    pub preset: CanvasPreset,
    pub background_hex: String,
    pub require_canvas_size: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamePhase {
    Lobby,
    InProgress,
    Finished,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionKind {
    Prompt(String),
    Name(String),
    Drawing {
        attachment_url: String,
        filename: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameMode {
    Classic,
    Short {
        prompts: Vec<ShortPrompt>,
        drawings: Vec<ShortDrawing>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortPrompt {
    pub author_id: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortDrawing {
    pub author_id: u64,
    pub prompt_author_id: u64,
    pub prompt: String,
    pub attachment_url: String,
    pub filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortDrawingAssignment {
    pub player_id: u64,
    pub prompt_author_id: u64,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortShowcase {
    pub prompt_author_id: u64,
    pub prompt: String,
    pub drawing_author_id: u64,
    pub attachment_url: String,
    pub filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainEntry {
    pub author_id: u64,
    pub kind: SubmissionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    pub original_player_id: u64,
    pub entries: Vec<ChainEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundKind {
    Prompt,
    Drawing,
    Naming,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundAssignment {
    pub player_id: u64,
    pub chain_index: usize,
    pub previous_entry: Option<ChainEntry>,
    pub round_kind: RoundKind,
}

#[derive(Debug, Clone)]
pub struct Game {
    pub key: GameKey,
    pub host_id: u64,
    pub lobby_message_id: Option<u64>,
    pub canvas: CanvasConfig,
    pub phase: GamePhase,
    pub players: Vec<u64>,
    pub current_round: usize,
    pub chains: Vec<Chain>,
    pub mode: GameMode,
    unready_players: HashSet<u64>,
    submitted_this_round: HashSet<u64>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GameError {
    #[error("a game already exists in this server")]
    GameAlreadyExists,
    #[error("this command must be used in a server channel")]
    MissingGuild,
    #[error("there is no game in this server")]
    GameNotFound,
    #[error("only the host can do that")]
    NotHost,
    #[error("the host cannot leave the lobby; cancel the game instead")]
    HostCannotLeave,
    #[error("the game has already started")]
    AlreadyStarted,
    #[error("the game is not accepting submissions")]
    NotInProgress,
    #[error("you are already in this game")]
    AlreadyJoined,
    #[error("you are already in another active Grateic Phone game")]
    AlreadyInAnotherGame,
    #[error("you are not in this game")]
    NotAPlayer,
    #[error("the game is not in the lobby")]
    NotInLobby,
    #[error("at least two players are required")]
    NotEnoughPlayers,
    #[error("not every player is ready")]
    PlayersNotReady,
    #[error("you already submitted for this round")]
    AlreadySubmitted,
    #[error("this round expects text")]
    ExpectedText,
    #[error("this round expects an image attachment")]
    ExpectedDrawing,
    #[error("the round is not ready to advance")]
    RoundNotComplete,
    #[error("the requested round transition is stale")]
    StaleRoundTransition,
    #[error("text submissions must be 140 characters or fewer")]
    TextTooLong,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advance {
    Waiting,
    NextRound {
        next_round: usize,
        assignments: Vec<RoundAssignment>,
    },
    Finished {
        chains: Vec<Chain>,
    },
    ShortDrawingRound {
        assignments: Vec<ShortDrawingAssignment>,
    },
    ShortFinished {
        showcases: Vec<ShortShowcase>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartRound {
    Classic(Vec<RoundAssignment>),
    ShortPrompt,
}

impl Game {
    pub fn new(key: GameKey, host_id: u64, canvas: CanvasConfig) -> Self {
        Self::new_with_mode(key, host_id, canvas, GameMode::Classic)
    }

    pub fn new_short(key: GameKey, host_id: u64, canvas: CanvasConfig) -> Self {
        Self::new_with_mode(
            key,
            host_id,
            canvas,
            GameMode::Short {
                prompts: Vec::new(),
                drawings: Vec::new(),
            },
        )
    }

    fn new_with_mode(key: GameKey, host_id: u64, canvas: CanvasConfig, mode: GameMode) -> Self {
        Self {
            key,
            host_id,
            lobby_message_id: None,
            canvas,
            phase: GamePhase::Lobby,
            players: vec![host_id],
            current_round: 0,
            chains: Vec::new(),
            mode,
            unready_players: HashSet::new(),
            submitted_this_round: HashSet::new(),
        }
    }

    pub fn join(&mut self, player_id: u64) -> Result<(), GameError> {
        if self.phase != GamePhase::Lobby {
            return Err(GameError::AlreadyStarted);
        }

        if self.has_player(player_id) {
            return Err(GameError::AlreadyJoined);
        }

        self.players.push(player_id);
        self.mark_ready(player_id)?;
        Ok(())
    }

    pub fn set_lobby_message_id(&mut self, message_id: u64) {
        self.lobby_message_id = Some(message_id);
    }

    pub fn leave(&mut self, player_id: u64) -> Result<(), GameError> {
        if self.phase != GamePhase::Lobby {
            return Err(GameError::AlreadyStarted);
        }

        if player_id == self.host_id {
            return Err(GameError::HostCannotLeave);
        }

        let Some(player_index) = self.players.iter().position(|id| *id == player_id) else {
            return Err(GameError::NotAPlayer);
        };

        self.players.remove(player_index);
        self.unready_players.remove(&player_id);
        self.submitted_this_round.remove(&player_id);
        Ok(())
    }

    pub fn mark_ready(&mut self, player_id: u64) -> Result<(), GameError> {
        if self.phase != GamePhase::Lobby {
            return Err(GameError::NotInLobby);
        }

        if !self.has_player(player_id) {
            return Err(GameError::NotAPlayer);
        }

        self.unready_players.remove(&player_id);
        Ok(())
    }

    pub fn mark_not_ready(&mut self, player_id: u64) {
        self.unready_players.insert(player_id);
    }

    pub fn start(&mut self, requester_id: u64) -> Result<StartRound, GameError> {
        let mut rng = thread_rng();
        let players = shuffled_player_order(&self.players, &mut rng);
        self.start_with_player_order(requester_id, players)
    }

    fn start_with_player_order(
        &mut self,
        requester_id: u64,
        players: Vec<u64>,
    ) -> Result<StartRound, GameError> {
        if requester_id != self.host_id {
            return Err(GameError::NotHost);
        }

        if self.players.len() < 2 {
            return Err(GameError::NotEnoughPlayers);
        }

        if self.phase != GamePhase::Lobby {
            return Err(GameError::AlreadyStarted);
        }

        if !self.unready_players().is_empty() {
            return Err(GameError::PlayersNotReady);
        }

        self.phase = GamePhase::InProgress;
        self.players = players;
        self.current_round = 0;
        self.submitted_this_round.clear();

        match &mut self.mode {
            GameMode::Classic => {
                self.chains = self
                    .players
                    .iter()
                    .map(|player_id| Chain {
                        original_player_id: *player_id,
                        entries: Vec::new(),
                    })
                    .collect();

                Ok(StartRound::Classic(self.assignments_for_current_round()))
            }
            GameMode::Short { prompts, drawings } => {
                prompts.clear();
                drawings.clear();
                self.chains.clear();
                Ok(StartRound::ShortPrompt)
            }
        }
    }

    pub fn reset_to_lobby_after_failed_start(&mut self, unready_player_id: u64) {
        self.phase = GamePhase::Lobby;
        self.current_round = 0;
        self.reset_round_state();
        if let GameMode::Short { prompts, drawings } = &mut self.mode {
            prompts.clear();
            drawings.clear();
        }
        self.mark_not_ready(unready_player_id);
    }

    pub fn cancel(&mut self, requester_id: u64) -> Result<(), GameError> {
        if requester_id != self.host_id {
            return Err(GameError::NotHost);
        }

        if self.phase != GamePhase::Lobby {
            return Err(GameError::AlreadyStarted);
        }

        self.phase = GamePhase::Cancelled;
        Ok(())
    }

    pub fn force_cancel(&mut self, requester_id: u64) -> Result<(), GameError> {
        if requester_id != self.host_id {
            return Err(GameError::NotHost);
        }

        self.phase = GamePhase::Cancelled;
        Ok(())
    }

    pub fn submit_text(&mut self, player_id: u64, text: String) -> Result<Advance, GameError> {
        let text = validate_text_submission(text)?;

        if matches!(self.mode, GameMode::Short { .. }) {
            return self.submit_short_prompt(player_id, text);
        }

        if !matches!(self.round_kind(), RoundKind::Prompt | RoundKind::Naming) {
            return Err(GameError::ExpectedDrawing);
        }

        let kind = match self.round_kind() {
            RoundKind::Prompt => SubmissionKind::Prompt(text),
            RoundKind::Naming => SubmissionKind::Name(text),
            RoundKind::Drawing => return Err(GameError::ExpectedDrawing),
        };

        self.submit(player_id, kind)
    }

    pub fn submit_drawing(
        &mut self,
        player_id: u64,
        attachment_url: String,
        filename: String,
    ) -> Result<Advance, GameError> {
        if matches!(self.mode, GameMode::Short { .. }) {
            return self.submit_short_drawing(player_id, attachment_url, filename);
        }

        if self.round_kind() != RoundKind::Drawing {
            return Err(GameError::ExpectedText);
        }

        self.submit(
            player_id,
            SubmissionKind::Drawing {
                attachment_url,
                filename,
            },
        )
    }

    pub fn assignments_for_current_round(&self) -> Vec<RoundAssignment> {
        self.assignments_for_round(self.current_round)
    }

    pub fn pending_next_round(&self) -> Option<Advance> {
        if matches!(self.mode, GameMode::Short { .. }) {
            if self.phase == GamePhase::InProgress
                && self.current_round == 0
                && self.submitted_this_round.len() == self.players.len()
            {
                return Some(Advance::ShortDrawingRound {
                    assignments: self.short_drawing_assignments(),
                });
            }

            return None;
        }

        if self.phase != GamePhase::InProgress
            || self.submitted_this_round.len() != self.players.len()
            || self.current_round + 1 >= self.total_rounds()
        {
            return None;
        }

        let next_round = self.current_round + 1;
        Some(Advance::NextRound {
            next_round,
            assignments: self.assignments_for_round(next_round),
        })
    }

    pub fn commit_next_round(&mut self, next_round: usize) -> Result<(), GameError> {
        if matches!(self.mode, GameMode::Short { .. }) {
            if self.phase != GamePhase::InProgress {
                return Err(GameError::NotInProgress);
            }

            if self.current_round != 0 || next_round != 1 {
                return Err(GameError::StaleRoundTransition);
            }

            if self.submitted_this_round.len() != self.players.len() {
                return Err(GameError::RoundNotComplete);
            }

            self.current_round = 1;
            self.submitted_this_round.clear();
            return Ok(());
        }

        if self.phase != GamePhase::InProgress {
            return Err(GameError::NotInProgress);
        }

        if self.submitted_this_round.len() != self.players.len() {
            return Err(GameError::RoundNotComplete);
        }

        if next_round != self.current_round + 1 || next_round >= self.total_rounds() {
            return Err(GameError::StaleRoundTransition);
        }

        self.current_round = next_round;
        self.submitted_this_round.clear();
        Ok(())
    }

    fn assignments_for_round(&self, round: usize) -> Vec<RoundAssignment> {
        self.players
            .iter()
            .enumerate()
            .map(|(player_index, player_id)| {
                let chain_index = self.chain_index_for(player_index, round);
                RoundAssignment {
                    player_id: *player_id,
                    chain_index,
                    previous_entry: self.chains[chain_index].entries.last().cloned(),
                    round_kind: self.round_kind_for(round),
                }
            })
            .collect()
    }

    pub fn round_kind(&self) -> RoundKind {
        if matches!(self.mode, GameMode::Short { .. }) {
            return if self.current_round == 0 {
                RoundKind::Prompt
            } else {
                RoundKind::Drawing
            };
        }

        self.round_kind_for(self.current_round)
    }

    fn round_kind_for(&self, round: usize) -> RoundKind {
        match round {
            0 => RoundKind::Prompt,
            round if round + 1 == self.total_rounds() => RoundKind::Naming,
            round if round % 2 == 1 => RoundKind::Drawing,
            _ => RoundKind::Prompt,
        }
    }

    pub fn submitted_count(&self) -> usize {
        self.submitted_this_round.len()
    }

    pub fn has_submitted(&self, player_id: u64) -> bool {
        self.submitted_this_round.contains(&player_id)
    }

    pub fn ready_count(&self) -> usize {
        self.players.len() - self.unready_players.len()
    }

    pub fn unready_players(&self) -> Vec<u64> {
        self.players
            .iter()
            .copied()
            .filter(|player_id| self.unready_players.contains(player_id))
            .collect()
    }

    pub fn total_rounds(&self) -> usize {
        if matches!(self.mode, GameMode::Short { .. }) {
            return 2;
        }

        self.players.len() * 2 + 1
    }

    pub fn is_short(&self) -> bool {
        matches!(self.mode, GameMode::Short { .. })
    }

    pub fn mode_label(&self) -> &'static str {
        if self.is_short() { "short" } else { "classic" }
    }

    pub fn has_player(&self, player_id: u64) -> bool {
        self.players.contains(&player_id)
    }

    fn submit(&mut self, player_id: u64, kind: SubmissionKind) -> Result<Advance, GameError> {
        let player_index = self.ensure_can_submit(player_id)?;

        let chain_index = self.assigned_chain_index(player_index);
        self.chains[chain_index].entries.push(ChainEntry {
            author_id: player_id,
            kind,
        });
        self.submitted_this_round.insert(player_id);

        if self.submitted_this_round.len() < self.players.len() {
            return Ok(Advance::Waiting);
        }

        if self.current_round + 1 >= self.total_rounds() {
            self.phase = GamePhase::Finished;
            return Ok(Advance::Finished {
                chains: self.chains.clone(),
            });
        }

        let next_round = self.current_round + 1;

        Ok(Advance::NextRound {
            next_round,
            assignments: self.assignments_for_round(next_round),
        })
    }

    fn chain_index_for(&self, player_index: usize, round: usize) -> usize {
        let player_count = self.players.len();
        let author_offset = self.chain_author_offset_for_round(round);
        (player_index + player_count - author_offset) % player_count
    }

    fn chain_author_offset_for_round(&self, round: usize) -> usize {
        if round + 1 == self.total_rounds() {
            return 0;
        }

        if round % 2 == 0 {
            return round / 2;
        }

        let player_count = self.players.len();
        let drawing_shift = if player_count == 2 { 1 } else { 2 };
        ((round - 1) / 2 + drawing_shift) % player_count
    }

    fn assigned_chain_index(&self, player_index: usize) -> usize {
        self.chain_index_for(player_index, self.current_round)
    }

    fn submit_short_prompt(&mut self, player_id: u64, text: String) -> Result<Advance, GameError> {
        if self.phase != GamePhase::InProgress {
            return Err(GameError::NotInProgress);
        }

        if self.current_round != 0 {
            return Err(GameError::ExpectedDrawing);
        }

        self.ensure_can_submit(player_id)?;

        let GameMode::Short { prompts, .. } = &mut self.mode else {
            unreachable!("short prompt submissions only run in short mode");
        };

        prompts.push(ShortPrompt {
            author_id: player_id,
            text,
        });
        self.submitted_this_round.insert(player_id);

        if self.submitted_this_round.len() < self.players.len() {
            return Ok(Advance::Waiting);
        }

        Ok(Advance::ShortDrawingRound {
            assignments: self.short_drawing_assignments(),
        })
    }

    fn submit_short_drawing(
        &mut self,
        player_id: u64,
        attachment_url: String,
        filename: String,
    ) -> Result<Advance, GameError> {
        if self.phase != GamePhase::InProgress {
            return Err(GameError::NotInProgress);
        }

        if self.current_round != 1 {
            return Err(GameError::ExpectedText);
        }

        self.ensure_can_submit(player_id)?;

        let Some(assignment) = self
            .short_drawing_assignments()
            .into_iter()
            .find(|assignment| assignment.player_id == player_id)
        else {
            return Err(GameError::NotAPlayer);
        };

        let GameMode::Short { drawings, .. } = &mut self.mode else {
            unreachable!("short drawing submissions only run in short mode");
        };

        drawings.push(ShortDrawing {
            author_id: player_id,
            prompt_author_id: assignment.prompt_author_id,
            prompt: assignment.prompt,
            attachment_url,
            filename,
        });
        self.submitted_this_round.insert(player_id);

        if self.submitted_this_round.len() < self.players.len() {
            return Ok(Advance::Waiting);
        }

        self.phase = GamePhase::Finished;
        Ok(Advance::ShortFinished {
            showcases: self.short_showcases(),
        })
    }

    fn short_drawing_assignments(&self) -> Vec<ShortDrawingAssignment> {
        let GameMode::Short { prompts, .. } = &self.mode else {
            return Vec::new();
        };

        self.players
            .iter()
            .enumerate()
            .filter_map(|(player_index, player_id)| {
                let prompt_author_id =
                    self.players[(player_index + self.players.len() - 1) % self.players.len()];
                let prompt = prompts
                    .iter()
                    .find(|prompt| prompt.author_id == prompt_author_id)?;

                Some(ShortDrawingAssignment {
                    player_id: *player_id,
                    prompt_author_id,
                    prompt: prompt.text.clone(),
                })
            })
            .collect()
    }

    fn short_showcases(&self) -> Vec<ShortShowcase> {
        let GameMode::Short { drawings, .. } = &self.mode else {
            return Vec::new();
        };

        self.players
            .iter()
            .filter_map(|prompt_author_id| {
                let drawing = drawings
                    .iter()
                    .find(|drawing| drawing.prompt_author_id == *prompt_author_id)?;

                Some(ShortShowcase {
                    prompt_author_id: *prompt_author_id,
                    prompt: drawing.prompt.clone(),
                    drawing_author_id: drawing.author_id,
                    attachment_url: drawing.attachment_url.clone(),
                    filename: drawing.filename.clone(),
                })
            })
            .collect()
    }

    fn ensure_can_submit(&self, player_id: u64) -> Result<usize, GameError> {
        if self.phase != GamePhase::InProgress {
            return Err(GameError::NotInProgress);
        }

        let Some(player_index) = self.players.iter().position(|id| *id == player_id) else {
            return Err(GameError::NotAPlayer);
        };

        if self.submitted_this_round.contains(&player_id) {
            return Err(GameError::AlreadySubmitted);
        }

        Ok(player_index)
    }

    fn reset_round_state(&mut self) {
        self.chains.clear();
        self.submitted_this_round.clear();
    }
}

fn shuffled_player_order<R: rand::Rng + ?Sized>(players: &[u64], rng: &mut R) -> Vec<u64> {
    let mut shuffled = players.to_vec();
    shuffled.shuffle(rng);
    shuffled
}

fn validate_text_submission(text: String) -> Result<String, GameError> {
    let text = text.trim().to_owned();
    if text.chars().count() > 140 {
        return Err(GameError::TextTooLong);
    }

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    fn game_with_players(count: u64) -> Game {
        let mut game = Game::new(
            GameKey {
                guild_id: 1,
                channel_id: 10,
            },
            1,
            CanvasConfig {
                preset: CanvasPreset::Square,
                background_hex: "#ffffff".to_owned(),
                require_canvas_size: true,
            },
        );

        for player_id in 2..=count {
            game.join(player_id).unwrap();
        }

        game
    }

    fn short_game_with_players(count: u64) -> Game {
        let mut game = Game::new_short(
            GameKey {
                guild_id: 1,
                channel_id: 10,
            },
            1,
            CanvasConfig {
                preset: CanvasPreset::Square,
                background_hex: "#ffffff".to_owned(),
                require_canvas_size: true,
            },
        );

        for player_id in 2..=count {
            game.join(player_id).unwrap();
        }

        game
    }

    fn commit_if_next_round(game: &mut Game, advance: Advance) {
        if let Advance::NextRound { next_round, .. } = advance {
            game.commit_next_round(next_round).unwrap();
        }
    }

    fn start_in_join_order(game: &mut Game) -> StartRound {
        game.start_with_player_order(1, game.players.clone())
            .unwrap()
    }

    #[test]
    fn each_chain_alternates_text_and_drawing_until_author_names_final_image() {
        for count in [2, 3, 4, 5, 6] {
            let mut game = game_with_players(count);
            let player_order = match count {
                3 => vec![2, 3, 1],
                4 => vec![3, 1, 4, 2],
                5 => vec![4, 1, 5, 2, 3],
                6 => vec![6, 2, 4, 1, 5, 3],
                _ => game.players.clone(),
            };
            game.start_with_player_order(1, player_order.clone())
                .unwrap();

            for round in 0..game.total_rounds() {
                let assignments = game.assignments_for_current_round();
                let mut chain_indexes = assignments
                    .iter()
                    .map(|assignment| assignment.chain_index)
                    .collect::<Vec<_>>();
                chain_indexes.sort_unstable();

                assert_eq!(chain_indexes, (0..count as usize).collect::<Vec<_>>());

                for assignment in assignments {
                    let result = match game.round_kind() {
                        RoundKind::Prompt => {
                            game.submit_text(assignment.player_id, format!("prompt {round}"))
                        }
                        RoundKind::Drawing => game.submit_drawing(
                            assignment.player_id,
                            format!("https://cdn.example/{round}.png"),
                            "drawing.png".to_owned(),
                        ),
                        RoundKind::Naming => {
                            game.submit_text(assignment.player_id, format!("name {round}"))
                        }
                    };
                    commit_if_next_round(&mut game, result.unwrap());
                }
            }

            assert_eq!(game.phase, GamePhase::Finished);
            for (chain_index, chain) in game.chains.iter().enumerate() {
                assert_eq!(chain.entries.len(), game.total_rounds());
                assert_eq!(chain.original_player_id, player_order[chain_index]);
                assert_eq!(
                    chain.entries.first().unwrap().author_id,
                    chain.original_player_id
                );
                assert!(matches!(
                    chain.entries.first().unwrap().kind,
                    SubmissionKind::Prompt(_)
                ));
                assert_eq!(
                    chain.entries.last().unwrap().author_id,
                    chain.original_player_id
                );
                assert!(matches!(
                    chain.entries.last().unwrap().kind,
                    SubmissionKind::Name(_)
                ));

                let mut prompt_authors = Vec::new();
                let mut drawing_authors = Vec::new();
                for (entry_index, entry) in chain.entries.iter().enumerate() {
                    match entry_index {
                        0 => {
                            assert!(matches!(entry.kind, SubmissionKind::Prompt(_)));
                            prompt_authors.push(entry.author_id);
                        }
                        index if index + 1 == count as usize * 2 + 1 => {
                            assert!(matches!(entry.kind, SubmissionKind::Name(_)))
                        }
                        index if index % 2 == 1 => {
                            assert!(matches!(entry.kind, SubmissionKind::Drawing { .. }));
                            drawing_authors.push(entry.author_id);
                        }
                        _ => {
                            assert!(matches!(entry.kind, SubmissionKind::Prompt(_)));
                            prompt_authors.push(entry.author_id);
                        }
                    }
                }

                let mut sorted_prompt_authors = prompt_authors;
                sorted_prompt_authors.sort_unstable();
                assert_eq!(sorted_prompt_authors, (1..=count).collect::<Vec<_>>());

                let mut sorted_drawing_authors = drawing_authors;
                sorted_drawing_authors.sort_unstable();
                assert_eq!(sorted_drawing_authors, (1..=count).collect::<Vec<_>>());
            }
        }
    }

    #[test]
    fn shuffled_player_order_preserves_players_and_can_change_order() {
        let players = vec![1, 2, 3, 4, 5, 6];
        let mut rng = StdRng::seed_from_u64(7);
        let shuffled = shuffled_player_order(&players, &mut rng);

        assert_ne!(shuffled, players);

        let mut sorted_shuffled = shuffled;
        sorted_shuffled.sort_unstable();
        assert_eq!(sorted_shuffled, players);
    }

    #[test]
    fn detects_completion_after_final_round() {
        let mut game = game_with_players(3);
        game.start(1).unwrap();

        assert_eq!(
            game.submit_text(1, "a".to_owned()).unwrap(),
            Advance::Waiting
        );
        assert_eq!(
            game.submit_text(2, "b".to_owned()).unwrap(),
            Advance::Waiting
        );
        let advance = game.submit_text(3, "c".to_owned()).unwrap();
        assert!(matches!(advance, Advance::NextRound { .. }));
        commit_if_next_round(&mut game, advance);

        while game.round_kind() != RoundKind::Naming {
            let assignments = game.assignments_for_current_round();
            for assignment in assignments {
                let advance = match game.round_kind() {
                    RoundKind::Prompt => {
                        game.submit_text(assignment.player_id, "next prompt".to_owned())
                    }
                    RoundKind::Drawing => game.submit_drawing(
                        assignment.player_id,
                        "https://cdn.example/image.png".to_owned(),
                        "drawing.png".to_owned(),
                    ),
                    RoundKind::Naming => unreachable!(),
                }
                .unwrap();

                commit_if_next_round(&mut game, advance);
            }
        }

        assert_eq!(game.round_kind(), RoundKind::Naming);
        assert_eq!(game.assignments_for_current_round()[0].chain_index, 0);
        assert_eq!(game.assignments_for_current_round()[1].chain_index, 1);
        assert_eq!(game.assignments_for_current_round()[2].chain_index, 2);

        let naming_assignments = game.assignments_for_current_round();
        let naming_cases = [
            (
                naming_assignments[0].player_id,
                naming_assignments[0].chain_index,
                "noodle ruins",
            ),
            (
                naming_assignments[1].player_id,
                naming_assignments[1].chain_index,
                "robot fog",
            ),
            (
                naming_assignments[2].player_id,
                naming_assignments[2].chain_index,
                "dragon office",
            ),
        ];

        assert_eq!(
            game.submit_text(naming_cases[0].0, naming_cases[0].2.to_owned())
                .unwrap(),
            Advance::Waiting
        );
        assert_eq!(
            game.chains[naming_cases[0].1].entries.last().unwrap().kind,
            SubmissionKind::Name(naming_cases[0].2.to_owned())
        );

        assert_eq!(
            game.submit_text(naming_cases[1].0, naming_cases[1].2.to_owned())
                .unwrap(),
            Advance::Waiting
        );
        assert_eq!(
            game.chains[naming_cases[1].1].entries.last().unwrap().kind,
            SubmissionKind::Name(naming_cases[1].2.to_owned())
        );

        assert!(matches!(
            game.submit_text(naming_cases[2].0, naming_cases[2].2.to_owned())
                .unwrap(),
            Advance::Finished { .. }
        ));
        assert_eq!(
            game.chains[naming_cases[2].1].entries.last().unwrap().kind,
            SubmissionKind::Name(naming_cases[2].2.to_owned())
        );
        assert_eq!(game.phase, GamePhase::Finished);

        for chain in &game.chains {
            assert!(matches!(
                chain.entries.last().unwrap().kind,
                SubmissionKind::Name(_)
            ));
            assert_eq!(
                chain.entries.last().unwrap().author_id,
                chain.original_player_id
            );
        }
    }

    #[test]
    fn next_round_does_not_commit_until_delivery_succeeds() {
        let mut game = game_with_players(3);
        game.start(1).unwrap();

        game.submit_text(1, "a".to_owned()).unwrap();
        game.submit_text(2, "b".to_owned()).unwrap();
        let advance = game.submit_text(3, "c".to_owned()).unwrap();

        let Advance::NextRound {
            next_round,
            assignments,
        } = advance
        else {
            panic!("expected next round assignments");
        };

        assert_eq!(next_round, 1);
        assert_eq!(assignments.len(), 3);
        assert_eq!(game.current_round, 0);
        assert_eq!(game.submitted_count(), 3);
        assert!(matches!(
            game.pending_next_round(),
            Some(Advance::NextRound { next_round: 1, .. })
        ));

        game.commit_next_round(next_round).unwrap();
        assert_eq!(game.current_round, 1);
        assert_eq!(game.submitted_count(), 0);
        assert_eq!(game.round_kind(), RoundKind::Drawing);
    }

    #[test]
    fn rejects_invalid_actions() {
        let mut game = game_with_players(2);

        assert_eq!(game.start(2), Err(GameError::NotHost));
        assert_eq!(
            game.submit_text(1, "too early".to_owned()),
            Err(GameError::NotInProgress)
        );

        game.start(1).unwrap();
        assert_eq!(game.join(99), Err(GameError::AlreadyStarted));
        assert_eq!(
            game.submit_drawing(1, "url".to_owned(), "x.png".to_owned()),
            Err(GameError::ExpectedText)
        );
        assert_eq!(
            game.submit_text(99, "oops".to_owned()),
            Err(GameError::NotAPlayer)
        );

        game.submit_text(1, "ok".to_owned()).unwrap();
        assert_eq!(
            game.submit_text(1, "again".to_owned()),
            Err(GameError::AlreadySubmitted)
        );
    }

    #[test]
    fn players_are_ready_by_default_and_failed_dm_marks_unready() {
        let mut game = game_with_players(3);

        assert!(game.unready_players().is_empty());
        assert_eq!(game.ready_count(), 3);
        assert!(game.start(1).is_ok());

        let mut game = game_with_players(3);
        game.mark_not_ready(2);
        assert_eq!(game.start(1), Err(GameError::PlayersNotReady));
        assert_eq!(game.unready_players(), vec![2]);

        game.mark_ready(2).unwrap();
        assert!(game.unready_players().is_empty());
        assert!(game.start(1).is_ok());
    }

    #[test]
    fn non_host_can_leave_lobby() {
        let mut game = game_with_players(3);

        game.leave(2).unwrap();

        assert_eq!(game.players, vec![1, 3]);
        assert!(!game.has_player(2));
    }

    #[test]
    fn host_cannot_leave_lobby() {
        let mut game = game_with_players(2);

        assert_eq!(game.leave(1), Err(GameError::HostCannotLeave));
        assert_eq!(game.players, vec![1, 2]);
    }

    #[test]
    fn leave_is_rejected_after_start() {
        let mut game = game_with_players(2);
        start_in_join_order(&mut game);

        assert_eq!(game.leave(2), Err(GameError::AlreadyStarted));
    }

    #[test]
    fn normal_cancel_is_lobby_only_but_force_cancel_can_end_started_game() {
        let mut game = game_with_players(2);

        game.cancel(1).unwrap();
        assert_eq!(game.phase, GamePhase::Cancelled);

        let mut game = game_with_players(2);
        start_in_join_order(&mut game);

        assert_eq!(game.cancel(1), Err(GameError::AlreadyStarted));
        game.force_cancel(1).unwrap();
        assert_eq!(game.phase, GamePhase::Cancelled);
    }

    #[test]
    fn leave_removes_readiness_tracking() {
        let mut game = game_with_players(3);
        game.mark_not_ready(2);

        game.leave(2).unwrap();

        assert_eq!(game.players, vec![1, 3]);
        assert_eq!(game.unready_players(), Vec::<u64>::new());
        assert_eq!(game.ready_count(), 2);
    }

    #[test]
    fn text_submissions_are_trimmed_and_capped_at_140_characters() {
        let mut game = game_with_players(2);
        start_in_join_order(&mut game);

        let accepted = format!("  {}  ", "a".repeat(140));
        assert_eq!(game.submit_text(1, accepted).unwrap(), Advance::Waiting);
        assert_eq!(
            game.chains[0].entries[0].kind,
            SubmissionKind::Prompt("a".repeat(140))
        );

        assert_eq!(
            game.submit_text(2, "b".repeat(141)),
            Err(GameError::TextTooLong)
        );
    }

    #[test]
    fn short_game_collects_prompts_then_assigns_each_to_another_player() {
        let mut game = short_game_with_players(3);

        assert_eq!(
            game.start_with_player_order(1, vec![2, 3, 1]).unwrap(),
            StartRound::ShortPrompt
        );
        assert_eq!(game.total_rounds(), 2);
        assert_eq!(game.round_kind(), RoundKind::Prompt);

        assert_eq!(
            game.submit_text(1, "sun whale".to_owned()).unwrap(),
            Advance::Waiting
        );
        assert_eq!(
            game.submit_text(2, "moon castle".to_owned()).unwrap(),
            Advance::Waiting
        );

        let advance = game.submit_text(3, "tiny train".to_owned()).unwrap();
        let Advance::ShortDrawingRound { assignments } = advance else {
            panic!("expected short drawing assignments");
        };

        assert_eq!(assignments.len(), 3);
        for assignment in &assignments {
            assert_ne!(assignment.player_id, assignment.prompt_author_id);
        }

        assert_eq!(assignments[0].player_id, 2);
        assert_eq!(assignments[0].prompt_author_id, 1);
        assert_eq!(assignments[0].prompt, "sun whale");

        game.commit_next_round(1).unwrap();
        assert_eq!(game.round_kind(), RoundKind::Drawing);
        assert_eq!(game.submitted_count(), 0);
    }

    #[test]
    fn short_game_drawing_assignments_follow_shuffled_order_not_submission_order() {
        fn assignments_for_submission_order(order: &[u64]) -> Vec<(u64, u64, String)> {
            let mut game = short_game_with_players(3);
            game.start_with_player_order(1, vec![2, 3, 1]).unwrap();

            let mut advance = Advance::Waiting;
            for player_id in order {
                advance = game
                    .submit_text(*player_id, format!("prompt from {player_id}"))
                    .unwrap();
            }

            let Advance::ShortDrawingRound { assignments } = advance else {
                panic!("expected short drawing assignments");
            };

            assignments
                .into_iter()
                .map(|assignment| {
                    (
                        assignment.player_id,
                        assignment.prompt_author_id,
                        assignment.prompt,
                    )
                })
                .collect()
        }

        let expected = vec![
            (2, 1, "prompt from 1".to_owned()),
            (3, 2, "prompt from 2".to_owned()),
            (1, 3, "prompt from 3".to_owned()),
        ];

        assert_eq!(assignments_for_submission_order(&[1, 2, 3]), expected);
        assert_eq!(assignments_for_submission_order(&[3, 1, 2]), expected);
    }

    #[test]
    fn short_game_finishes_with_showcases_after_one_drawing_round() {
        let mut game = short_game_with_players(2);
        game.start(1).unwrap();

        game.submit_text(1, "red door".to_owned()).unwrap();
        let advance = game.submit_text(2, "blue key".to_owned()).unwrap();
        let Advance::ShortDrawingRound { assignments } = advance else {
            panic!("expected short drawing assignments");
        };
        game.commit_next_round(1).unwrap();

        assert_eq!(game.round_kind(), RoundKind::Drawing);
        assert_eq!(
            game.submit_drawing(
                assignments[0].player_id,
                "https://cdn.example/one.png".to_owned(),
                "one.png".to_owned(),
            )
            .unwrap(),
            Advance::Waiting
        );

        let advance = game
            .submit_drawing(
                assignments[1].player_id,
                "https://cdn.example/two.png".to_owned(),
                "two.png".to_owned(),
            )
            .unwrap();
        let Advance::ShortFinished { showcases } = advance else {
            panic!("expected short showcases");
        };

        assert_eq!(game.phase, GamePhase::Finished);
        assert_eq!(showcases.len(), 2);
        assert!(showcases.iter().any(|showcase| {
            showcase.prompt_author_id == 1
                && showcase.prompt == "red door"
                && showcase.drawing_author_id == 2
        }));
        assert!(showcases.iter().any(|showcase| {
            showcase.prompt_author_id == 2
                && showcase.prompt == "blue key"
                && showcase.drawing_author_id == 1
        }));
    }
}
