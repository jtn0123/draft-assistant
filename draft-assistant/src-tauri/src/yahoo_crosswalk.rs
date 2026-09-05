//! Yahoo players onto Sleeper player ids.
//!
//! The board, the projections, the ADP column and the headshots are all keyed
//! by Sleeper's player id. Yahoo has its own ids and no projections at all, so
//! a Yahoo league is only worth loading if each of its players can be pointed
//! at the Sleeper row that carries those numbers. That is this module: a
//! name/team/position match against the players dictionary the engine already
//! caches, using the same normalisers the imported-projections CSV uses so
//! both sides are spelled the same way.
//!
//! Three things it deliberately does:
//!
//! - **Defences by team.** Sleeper writes a defence's name as the city and
//!   the mascot in two fields; Yahoo writes "Baltimore". Neither normalises
//!   to the other, and neither has to: there is exactly one `DEF` per team.
//!   The abbreviations still have to be spelled the same way, and they are
//!   not — `crate::yahoo_crosswalk_teams` folds JAC onto JAX and WSH onto
//!   WAS, and puts the city and mascot words behind that as a last try.
//! - **A team-blind second try.** A player traded since the dictionary was
//!   written has the right name and the wrong team on one side. Matching him
//!   to the wrong player is impossible — the fallback is only consulted when
//!   the name and position pick out exactly one Sleeper row.
//! - **Keeps the unmatched.** A player with no Sleeper row stays on the board
//!   under his Yahoo id with no projection, so a pick of him is still a pick
//!   and still leaves the board. He is counted, and the count is a warning.

use crate::second_opinion::{normalize_name, normalize_position};
use crate::sleeper::PlayerMeta;
use crate::yahoo_crosswalk_teams::{canonical_team, team_words};
use crate::yahoo_map::MappedPlayer;
use std::collections::{HashMap, HashSet};

/// What one league's player pool became.
#[derive(Debug, Default)]
pub struct Crosswalk {
    /// Yahoo player key (`449.p.30977`) -> the id the board and picks use:
    /// a Sleeper id where one was found, `yahoo:30977` where none was.
    pub ids: HashMap<String, String>,
    /// The rows to build the board from, keyed by that same id.
    pub player_meta: HashMap<String, PlayerMeta>,
    /// How many Yahoo players found no Sleeper row.
    pub unmatched: usize,
}

impl Crosswalk {
    /// The app id for one Yahoo player key, if the pool held it.
    pub fn id_for(&self, player_key: &str) -> Option<&str> {
        self.ids.get(player_key).map(String::as_str)
    }

    /// The one line the user is shown about players that went unmatched.
    pub fn warning(&self) -> Option<String> {
        (self.unmatched > 0).then(|| {
            format!(
                "{} Yahoo players had no Sleeper match — they are on the board \
                 with no projection",
                self.unmatched
            )
        })
    }
}

/// The full name a Sleeper row goes by, whichever fields it filled in.
fn sleeper_name(meta: &PlayerMeta) -> String {
    if let Some(full) = meta.full_name.as_deref().filter(|n| !n.trim().is_empty()) {
        return full.to_string();
    }
    let first = meta.first_name.clone().unwrap_or_default();
    let last = meta.last_name.clone().unwrap_or_default();
    format!("{first} {last}").trim().to_string()
}

/// The match key for a Sleeper row: normalised name, upper-case team,
/// normalised position — the same triple [`MappedPlayer::crosswalk_key`]
/// builds from the Yahoo side.
fn sleeper_key(meta: &PlayerMeta) -> Option<(String, String, String)> {
    let position = normalize_position(meta.position.as_deref()?);
    let name = normalize_name(&sleeper_name(meta));
    if name.is_empty() {
        return None;
    }
    Some((
        name,
        canonical_team(meta.team.as_deref().unwrap_or_default()),
        position,
    ))
}

/// The three lookups a match is tried against, built once per load.
struct Index<'a> {
    /// (name, team, position) -> Sleeper id.
    exact: HashMap<(String, String, String), &'a str>,
    /// (name, position) -> Sleeper id, dropped as soon as it is ambiguous.
    nameless_team: HashMap<(String, String), &'a str>,
    ambiguous: HashSet<(String, String)>,
    /// team -> the Sleeper id of that team's defence, keyed by the canonical
    /// abbreviation so JAC and JAX are one team.
    defences: HashMap<String, &'a str>,
    /// A word out of a defence's name ("jacksonville", "jaguars") -> that
    /// defence, for the rows Yahoo sends with no team abbreviation at all.
    /// Dropped as soon as two franchises share a word.
    defence_words: HashMap<String, &'a str>,
    ambiguous_words: HashSet<String>,
}

impl<'a> Index<'a> {
    fn build(sleeper: &'a HashMap<String, PlayerMeta>) -> Self {
        let mut index = Index {
            exact: HashMap::new(),
            nameless_team: HashMap::new(),
            ambiguous: HashSet::new(),
            defences: HashMap::new(),
            defence_words: HashMap::new(),
            ambiguous_words: HashSet::new(),
        };
        // Sorted so that two rows competing for one key resolve the same way
        // on every run; a HashMap's order would make the board move about.
        let mut rows: Vec<(&String, &PlayerMeta)> = sleeper.iter().collect();
        rows.sort_by(|a, b| a.0.cmp(b.0));
        for (id, meta) in rows {
            let Some((name, team, position)) = sleeper_key(meta) else {
                continue;
            };
            if position == "DEF" {
                if !team.is_empty() {
                    index.defences.entry(team.clone()).or_insert(id);
                }
                index.add_defence_words(meta, id);
                continue;
            }
            index
                .exact
                .entry((name.clone(), team, position.clone()))
                .or_insert(id);
            let loose = (name, position);
            if index.ambiguous.contains(&loose) {
                continue;
            }
            // A second row under the same name and position: the loose key can
            // no longer identify anyone, so it identifies nobody.
            if index.nameless_team.insert(loose.clone(), id).is_some() {
                index.nameless_team.remove(&loose);
                index.ambiguous.insert(loose);
            }
        }
        index
    }

    /// File one defence under each word of its name, and un-file any word two
    /// defences claim: a word that names two teams names neither.
    fn add_defence_words(&mut self, meta: &PlayerMeta, id: &'a str) {
        let name = format!(
            "{} {}",
            sleeper_name(meta),
            meta.last_name.clone().unwrap_or_default()
        );
        for word in team_words(&name) {
            if self.ambiguous_words.contains(&word) {
                continue;
            }
            match self.defence_words.insert(word.clone(), id) {
                Some(other) if other != id => {
                    self.defence_words.remove(&word);
                    self.ambiguous_words.insert(word);
                }
                _ => {}
            }
        }
    }

    fn find(&self, player: &MappedPlayer) -> Option<&'a str> {
        let (name, team, position) = player.crosswalk_key();
        if position == "DEF" {
            // The abbreviation first, then the name. Yahoo leaves the
            // abbreviation off the odd defence row, and a defence with no
            // Sleeper row is a starting lineup slot with no projection behind
            // it.
            return self.defences.get(&team).copied().or_else(|| {
                team_words(&player.meta.full_name.clone().unwrap_or_default())
                    .into_iter()
                    .find_map(|word| self.defence_words.get(&word).copied())
            });
        }
        if let Some(id) = self.exact.get(&(name.clone(), team, position.clone())) {
            return Some(id);
        }
        self.nameless_team.get(&(name, position)).copied()
    }
}

/// Match a Yahoo player pool against the Sleeper players dictionary.
pub fn build(pool: &[MappedPlayer], sleeper: &HashMap<String, PlayerMeta>) -> Crosswalk {
    let index = Index::build(sleeper);
    let mut crosswalk = Crosswalk::default();
    for player in pool {
        match index.find(player) {
            Some(id) => {
                crosswalk
                    .ids
                    .insert(player.player_key.clone(), id.to_string());
                if let Some(meta) = sleeper.get(id) {
                    crosswalk.player_meta.insert(id.to_string(), meta.clone());
                }
            }
            None => {
                crosswalk.unmatched += 1;
                crosswalk
                    .ids
                    .insert(player.player_key.clone(), player.id.clone());
                crosswalk
                    .player_meta
                    .insert(player.id.clone(), player.meta.clone());
            }
        }
    }
    crosswalk
}

#[cfg(test)]
#[path = "yahoo_crosswalk_tests.rs"]
mod tests;
