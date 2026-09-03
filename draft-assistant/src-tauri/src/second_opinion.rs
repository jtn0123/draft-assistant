//! A second opinion on the board: an imported projections CSV, matched to the
//! league's own players by name and position.
//!
//! The CSV is whatever `scripts/projections/fetch_2026_projections.py` writes — ESPN's Mike Clay season projections
//! with FantasyPros ADP bolted on. Its points are half-PPR and this league is
//! full PPR with six-point passing touchdowns and yardage bonuses, so the
//! **points do not compare and are deliberately not imported**. Ranks do:
//! "Clay has him WR9, this board has him WR21" is a real disagreement whatever
//! the scoring, and that is the whole of what this module carries across.
//!
//! Positional rank is recomputed here rather than read from the file, because
//! the file has no positional rank column — only an overall `rank` that
//! already mixes every position together.
//!
//! Not every row in the file is worth ranking against. The script labels each
//! one with how it came to exist, in two optional columns:
//!
//! * `projection_method` — `published` for a number a provider published,
//!   `adp_estimate` for one the script *invented* from a hand-tuned linear
//!   curve because no provider had the player.
//! * `ranking_basis` — `season_projection`, or `week_1_matchup_projection`
//!   for the team-defence table, which is a week-one matchup page standing in
//!   for a season ranking nobody publishes free.
//!
//! Both of those kinds are dropped at parse time rather than ranked: the
//! positional ranks this module computes come from the file's overall rank,
//! so one invented row shifts every real row behind it. The count and the
//! reason travel back to the toast so the user knows what was left out.
//! A file written before those columns existed has no labels, nothing to
//! drop, and imports exactly as it always did.
//!
//! A row with no ADP is *not* dropped and *not* given a made-up ADP: the ADP
//! column is not read here at all. The board's own ADP is what the rec card
//! counts market lag against, and a source row with no market price simply
//! contributes its ranks like any other.

use crate::board::BoardPlayer;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[path = "second_opinion_provenance.rs"]
mod provenance;
pub use provenance::Excluded;
use provenance::{unrankable, Cause};

/// The imported file, copied into the app data dir so it survives a restart.
pub const SECOND_OPINION_FILE: &str = "second_opinion.csv";

/// How far apart two positional ranks must be before the board calls it a
/// disagreement worth pointing at. Eight is roughly "a different tier".
pub const DISAGREEMENT: i64 = 8;

/// What one imported source says about one player. Ranks only — see the
/// module note on why the points are left behind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecondOpinion {
    /// Rank among this source's players at the same position: 9 for WR9.
    pub positional_rank: u32,
    /// The source's own overall rank column.
    pub overall_rank: u32,
    /// Short label for whoever published it — "Clay", "FantasyPros".
    pub source: String,
}

/// One row of the file, after parsing.
#[derive(Debug, Clone)]
struct Row {
    name: String,
    position: String,
    team: Option<String>,
    overall_rank: u32,
    points: f64,
    positional_rank: u32,
}

/// A parsed file, indexed three ways so a match can fall back from strict to
/// forgiving without re-scanning.
#[derive(Debug, Clone)]
pub struct SecondOpinionTable {
    pub source: String,
    /// When the file was imported, epoch seconds.
    pub loaded_at: u64,
    rows: Vec<Row>,
    /// (normalised name, position, team) -> row index.
    by_team: HashMap<(String, String, String), usize>,
    /// (normalised name, position) -> row index. The team-agnostic fallback:
    /// a player traded since the file was published still matches.
    by_name: HashMap<(String, String), usize>,
    /// The same, with the spaces taken out, so "Amon-Ra" and "Amon Ra" — or
    /// "D.J." and "DJ" — land on each other.
    by_squash: HashMap<(String, String), usize>,
    /// What the file offered and this table would not rank.
    excluded: Excluded,
}

/// How an import went, in the numbers the toast reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MatchReport {
    /// Rows in the file that found a player on this league's board.
    pub matched: usize,
    /// Rows the file offered, after the unrankable ones were dropped.
    pub total: usize,
    /// What was dropped before any of that, and why.
    pub excluded: Excluded,
}

impl MatchReport {
    pub fn message(&self) -> String {
        format!(
            "Second opinion loaded: {} of {} players matched",
            self.matched, self.total
        )
    }
}

/// Name parts that are decoration, not identity.
const SUFFIXES: &[&str] = &["jr", "sr", "ii", "iii", "iv", "v", "dst", "def"];

/// Lowercase, drop the punctuation, drop the suffixes, collapse the spaces.
///
/// Punctuation is removed without leaving a gap behind, so "D.J." becomes
/// "dj" and "Ja'Marr" becomes "jamarr" — which is what Sleeper writes for the
/// second of those and what a hand-typed sheet writes for the first.
pub fn normalize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c.is_whitespace() || c == '-' {
                ' '
            } else {
                '\0'
            }
        })
        .filter(|&c| c != '\0')
        .collect();
    let mut parts: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|part| !SUFFIXES.contains(part))
        .collect();
    // A name that is nothing but a suffix ("V" as a whole name) keeps itself
    // rather than normalising to the empty string, which would match anyone.
    if parts.is_empty() {
        parts = cleaned.split_whitespace().collect();
    }
    parts.join(" ")
}

/// The normalised name with the spaces taken out — the loosest key used.
fn squash(name: &str) -> String {
    normalize_name(name).replace(' ', "")
}

/// Sleeper calls a team defence `DEF`; projection exports call it `DST` or
/// `D/ST`. Everything else passes through uppercased.
pub fn normalize_position(position: &str) -> String {
    match position.trim().to_ascii_uppercase().as_str() {
        "DST" | "D/ST" | "DEFENSE" => "DEF".to_string(),
        other => other.to_string(),
    }
}

/// A short label for whoever published the file, read off the `source` column.
fn source_label(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("clayprojections") {
        "Clay".to_string()
    } else if lower.contains("fantasypros") {
        "FantasyPros".to_string()
    } else {
        "Imported".to_string()
    }
}

fn column(headers: &csv::StringRecord, wanted: &str) -> Option<usize> {
    headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case(wanted))
}

fn required(headers: &csv::StringRecord, wanted: &str) -> Result<usize, String> {
    column(headers, wanted).ok_or_else(|| {
        format!(
            "that file has no \"{wanted}\" column, so it is not a projections export — \
             re-run the projections script and import the CSV it writes"
        )
    })
}

/// Parse a projections CSV. Never panics: a header-less file, a file with the
/// wrong columns, and a file with no rows all come back as plain-words errors.
pub fn parse(text: &str, loaded_at: u64) -> Result<SecondOpinionTable, String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| format!("that file could not be read as a CSV: {e}"))?
        .clone();
    let name_at = required(&headers, "name")?;
    let position_at = required(&headers, "position")?;
    let rank_at = required(&headers, "rank")?;
    let points_at = required(&headers, "projected_fantasy_points")?;
    let team_at = column(&headers, "team");
    let source_at = column(&headers, "source");
    // Both optional: a file written before the script labelled its rows has
    // neither, and every row of it counts.
    let method_at = column(&headers, "projection_method");
    let basis_at = column(&headers, "ranking_basis");

    let mut rows: Vec<Row> = Vec::new();
    let mut excluded = Excluded::default();
    let mut source = String::new();
    for record in reader.records() {
        let record = match record {
            Ok(record) => record,
            // One malformed line is not worth losing the other four hundred.
            Err(_) => continue,
        };
        let name = record.get(name_at).unwrap_or_default().trim().to_string();
        let position = normalize_position(record.get(position_at).unwrap_or_default());
        if name.is_empty() || position.is_empty() {
            continue;
        }
        // Dropped before the rank is even read: a row whose points were
        // invented, or whose ranking is a week-one matchup, would otherwise
        // push every real row at its position one place down.
        match unrankable(
            method_at.and_then(|at| record.get(at)),
            basis_at.and_then(|at| record.get(at)),
        ) {
            Some(Cause::AdpEstimate) => {
                excluded.adp_estimate += 1;
                continue;
            }
            Some(Cause::Week1Ranking) => {
                excluded.week_1_ranking += 1;
                continue;
            }
            None => {}
        }
        let Ok(overall_rank) = record
            .get(rank_at)
            .unwrap_or_default()
            .trim()
            .parse::<f64>()
        else {
            continue;
        };
        let points = record
            .get(points_at)
            .unwrap_or_default()
            .trim()
            .parse::<f64>()
            .unwrap_or(0.0);
        let team = team_at
            .and_then(|at| record.get(at))
            .map(|t| t.trim().to_ascii_uppercase())
            .filter(|t| !t.is_empty());
        if source.is_empty() {
            if let Some(raw) = source_at.and_then(|at| record.get(at)) {
                if !raw.trim().is_empty() {
                    source = source_label(raw);
                }
            }
        }
        rows.push(Row {
            name,
            position,
            team,
            overall_rank: overall_rank.round().max(0.0) as u32,
            points,
            positional_rank: 0,
        });
    }
    if rows.is_empty() {
        if let Some(reason) = excluded.reason() {
            return Err(format!(
                "every row in that file is one the app cannot rank ({reason}) — \
                 re-run the projections script against live sources"
            ));
        }
        return Err(
            "that file has a header but no player rows in it — nothing to import".to_string(),
        );
    }
    if source.is_empty() {
        source = "Imported".to_string();
    }
    rank_within_positions(&mut rows);
    Ok(index(rows, source, loaded_at, excluded))
}

/// Number each position 1..n by the file's own overall rank, best first, with
/// projected points breaking a tie. The file has no positional rank of its
/// own, and its overall rank is the only ordering it commits to.
fn rank_within_positions(rows: &mut [Row]) {
    let mut by_position: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        by_position.entry(row.position.clone()).or_default().push(i);
    }
    for indexes in by_position.values_mut() {
        indexes.sort_by(|&a, &b| {
            rows[a]
                .overall_rank
                .cmp(&rows[b].overall_rank)
                .then_with(|| {
                    rows[b]
                        .points
                        .partial_cmp(&rows[a].points)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        for (rank, &i) in indexes.iter().enumerate() {
            rows[i].positional_rank = rank as u32 + 1;
        }
    }
}

fn index(rows: Vec<Row>, source: String, loaded_at: u64, excluded: Excluded) -> SecondOpinionTable {
    let mut by_team = HashMap::new();
    let mut by_name = HashMap::new();
    let mut by_squash = HashMap::new();
    for (i, row) in rows.iter().enumerate() {
        let name = normalize_name(&row.name);
        let squashed = squash(&row.name);
        if let Some(team) = &row.team {
            by_team
                .entry((name.clone(), row.position.clone(), team.clone()))
                .or_insert(i);
        }
        // A duplicated name keeps the higher-ranked row: the file is sorted
        // best-first, so the first one seen is already the one to keep.
        by_name.entry((name, row.position.clone())).or_insert(i);
        by_squash
            .entry((squashed, row.position.clone()))
            .or_insert(i);
    }
    SecondOpinionTable {
        source,
        loaded_at,
        rows,
        by_team,
        by_name,
        by_squash,
        excluded,
    }
}

impl SecondOpinionTable {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// What the file carried that this table would not rank.
    pub fn excluded(&self) -> Excluded {
        self.excluded
    }

    /// The row for one player, strictest key first: name + position + team,
    /// then name + position, then the spaceless name + position.
    fn find(&self, name: &str, position: &str, team: Option<&str>) -> Option<usize> {
        let normalized = normalize_name(name);
        let position = normalize_position(position);
        if let Some(team) = team {
            let key = (
                normalized.clone(),
                position.clone(),
                team.to_ascii_uppercase(),
            );
            if let Some(&i) = self.by_team.get(&key) {
                return Some(i);
            }
        }
        if let Some(&i) = self.by_name.get(&(normalized, position.clone())) {
            return Some(i);
        }
        self.by_squash.get(&(squash(name), position)).copied()
    }

    fn opinion(&self, i: usize) -> SecondOpinion {
        SecondOpinion {
            positional_rank: self.rows[i].positional_rank,
            overall_rank: self.rows[i].overall_rank,
            source: self.source.clone(),
        }
    }
}

/// Stamp every board player this table knows about, and report how much of the
/// file found a home. Players it has nothing to say about are cleared, so a
/// re-import replaces the last one rather than layering on top of it.
pub fn apply(table: &SecondOpinionTable, board: &mut [BoardPlayer]) -> MatchReport {
    let mut used = vec![false; table.len()];
    for player in board.iter_mut() {
        match table.find(&player.name, &player.position, player.team.as_deref()) {
            Some(i) => {
                used[i] = true;
                player.second_opinion = Some(table.opinion(i));
            }
            None => player.second_opinion = None,
        }
    }
    MatchReport {
        matched: used.iter().filter(|&&hit| hit).count(),
        total: table.len(),
        excluded: table.excluded,
    }
}

/// Where the imported copy lives.
pub fn stored_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SECOND_OPINION_FILE)
}

/// Read the copy kept in the app data dir, if there is one. A file that no
/// longer parses is reported rather than silently ignored — the user imported
/// it on purpose and deserves to know it stopped working.
pub fn load(data_dir: &Path) -> Result<Option<SecondOpinionTable>, String> {
    let path = stored_path(data_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let loaded_at = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    parse(&text, loaded_at).map(Some)
}

/// What an imported second opinion is worth on the rec card: a score
/// adjustment and the line that explains it.
///
/// Symmetric, because a source that likes a player *less* than this board is
/// exactly as informative as one that likes him more — the old one-directional
/// version quietly bumped every disagreement upward and never once warned the
/// user off. Proportional, because WR9-vs-WR21 and WR9-vs-WR60 are not the
/// same disagreement: a quarter of a point per position of gap, capped either
/// way so an unmatched-looking row cannot swamp the VORP the score is built on.
pub fn rec_adjustment(player: &BoardPlayer, teams: u32) -> Option<(f64, String)> {
    let opinion = player.second_opinion.as_ref()?;
    let gap = player.position_rank as i64 - opinion.positional_rank as i64;
    if gap.abs() < DISAGREEMENT {
        return None; // a few places apart is noise, in either direction
    }
    let delta = (gap as f64 / 4.0).clamp(-8.0, 8.0);
    let headline = format!(
        "{} has him {}{}",
        opinion.source, player.position, opinion.positional_rank
    );
    if gap < 0 {
        return Some((
            delta,
            format!(
                "{headline}, well behind this board's {}{}",
                player.position, player.position_rank
            ),
        ));
    }
    // How far behind the source's own overall rank the market is still
    // drafting him, in rounds of this league.
    if let Some(adp) = player.adp {
        let rounds = (adp - opinion.overall_rank as f64) / teams.max(1) as f64;
        if rounds >= 1.0 {
            let rounds = rounds.round() as u32;
            let plural = if rounds == 1 { "" } else { "s" };
            return Some((
                delta,
                format!("{headline} — market is {rounds} round{plural} late"),
            ));
        }
    }
    Some((
        delta,
        format!(
            "{headline}; this board has him {}{}",
            player.position, player.position_rank
        ),
    ))
}

#[cfg(test)]
#[path = "second_opinion_tests.rs"]
mod tests;
