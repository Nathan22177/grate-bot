use super::canvas::CanvasPreset;
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
    pub canvas: CanvasConfig,
    pub phase: GamePhase,
    pub players: Vec<u64>,
    pub current_round: usize,
    pub chains: Vec<Chain>,
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
    #[error("the game has already started")]
    AlreadyStarted,
    #[error("the game is not accepting submissions")]
    NotInProgress,
    #[error("you are already in this game")]
    AlreadyJoined,
    #[error("you are already in another active Grateic game")]
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
}

impl Game {
    pub fn new(key: GameKey, host_id: u64, canvas: CanvasConfig) -> Self {
        Self {
            key,
            host_id,
            canvas,
            phase: GamePhase::Lobby,
            players: vec![host_id],
            current_round: 0,
            chains: Vec::new(),
            unready_players: HashSet::new(),
            submitted_this_round: HashSet::new(),
        }
    }

    pub fn join(&mut self, player_id: u64) -> Result<(), GameError> {
        if self.phase != GamePhase::Lobby {
            return Err(GameError::AlreadyStarted);
        }

        if self.players.contains(&player_id) {
            return Err(GameError::AlreadyJoined);
        }

        self.players.push(player_id);
        self.mark_ready(player_id)?;
        Ok(())
    }

    pub fn mark_ready(&mut self, player_id: u64) -> Result<(), GameError> {
        if self.phase != GamePhase::Lobby {
            return Err(GameError::NotInLobby);
        }

        if !self.players.contains(&player_id) {
            return Err(GameError::NotAPlayer);
        }

        self.unready_players.remove(&player_id);
        Ok(())
    }

    pub fn mark_not_ready(&mut self, player_id: u64) {
        self.unready_players.insert(player_id);
    }

    pub fn start(&mut self, requester_id: u64) -> Result<Vec<RoundAssignment>, GameError> {
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
        self.current_round = 0;
        self.submitted_this_round.clear();
        self.chains = self
            .players
            .iter()
            .map(|player_id| Chain {
                original_player_id: *player_id,
                entries: Vec::new(),
            })
            .collect();

        Ok(self.assignments_for_current_round())
    }

    pub fn reset_to_lobby_after_failed_start(&mut self, unready_player_id: u64) {
        self.phase = GamePhase::Lobby;
        self.current_round = 0;
        self.chains.clear();
        self.submitted_this_round.clear();
        self.mark_not_ready(unready_player_id);
    }

    pub fn cancel(&mut self, requester_id: u64) -> Result<(), GameError> {
        if requester_id != self.host_id {
            return Err(GameError::NotHost);
        }

        self.phase = GamePhase::Cancelled;
        Ok(())
    }

    pub fn submit_text(&mut self, player_id: u64, text: String) -> Result<Advance, GameError> {
        if !matches!(self.round_kind(), RoundKind::Prompt | RoundKind::Naming) {
            return Err(GameError::ExpectedDrawing);
        }

        let kind = match self.round_kind() {
            RoundKind::Prompt => SubmissionKind::Prompt(text.trim().to_owned()),
            RoundKind::Naming => SubmissionKind::Name(text.trim().to_owned()),
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
        self.players.len() * 2 + 1
    }

    fn submit(&mut self, player_id: u64, kind: SubmissionKind) -> Result<Advance, GameError> {
        if self.phase != GamePhase::InProgress {
            return Err(GameError::NotInProgress);
        }

        let Some(player_index) = self.players.iter().position(|id| *id == player_id) else {
            return Err(GameError::NotAPlayer);
        };

        if self.submitted_this_round.contains(&player_id) {
            return Err(GameError::AlreadySubmitted);
        }

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
        (player_index + self.players.len() - (round % self.players.len())) % self.players.len()
    }

    fn assigned_chain_index(&self, player_index: usize) -> usize {
        self.chain_index_for(player_index, self.current_round)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn each_chain_alternates_text_and_drawing_until_author_names_final_image() {
        for count in [2, 3, 5] {
            let mut game = game_with_players(count);
            game.start(1).unwrap();

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
            for chain in &game.chains {
                assert_eq!(chain.entries.len(), game.total_rounds());
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

                for (entry_index, entry) in chain.entries.iter().enumerate() {
                    assert_eq!(
                        entry.author_id,
                        ((chain.original_player_id - 1 + entry_index as u64) % count) + 1
                    );

                    match entry_index {
                        0 => assert!(matches!(entry.kind, SubmissionKind::Prompt(_))),
                        index if index + 1 == count as usize * 2 + 1 => {
                            assert!(matches!(entry.kind, SubmissionKind::Name(_)))
                        }
                        index if index % 2 == 1 => {
                            assert!(matches!(entry.kind, SubmissionKind::Drawing { .. }))
                        }
                        _ => assert!(matches!(entry.kind, SubmissionKind::Prompt(_))),
                    }
                }

                let drawing_authors = chain
                    .entries
                    .iter()
                    .filter_map(|entry| match entry.kind {
                        SubmissionKind::Drawing { .. } => Some(entry.author_id),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(drawing_authors.len(), count as usize);
            }
        }
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
}
