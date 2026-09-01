//! Last season's final table, read from the previous league in the chain.
//!
//! Its own file so `season_engine` stays about this season. Nothing here ever
//! changes once a season is over, which is why the cache TTL is a month.

use crate::engine::Engine;
use crate::season::LastSeasonRow;
use crate::season_api::{Roster, SeasonEndpoints};
use crate::season_engine::LAST_SEASON_TTL_SECS;
use crate::sleeper::League;

impl Engine {
    /// Last season's final table, from the previous league in the chain.
    pub(crate) async fn last_season(
        &self,
        league: &League,
        my_user_id: Option<&str>,
        force: bool,
    ) -> Vec<LastSeasonRow> {
        let Some(previous_id) = league.previous_league_id.as_deref() else {
            return Vec::new();
        };
        if previous_id.is_empty() || previous_id == "0" {
            return Vec::new();
        }
        let name = Self::season_cache_name(previous_id, "final");
        if !force {
            if let Some((_, rows)) =
                self.read_cache::<Vec<LastSeasonRow>>(&name, LAST_SEASON_TTL_SECS)
            {
                return rows;
            }
        }
        let (rosters, users, bracket) = tokio::join!(
            self.client.rosters(previous_id),
            self.client.league_users(previous_id),
            self.client.winners_bracket(previous_id)
        );
        let Ok(rosters) = rosters else {
            return Vec::new();
        };
        let names = crate::sleeper::label_map(&users.unwrap_or_default());
        // The game that decides first place names the champion.
        let champion =
            bracket.unwrap_or_default().iter().find_map(
                |m| {
                    if m.p == Some(1) {
                        m.w
                    } else {
                        None
                    }
                },
            );
        let most_points = rosters
            .iter()
            .max_by(|a, b| a.settings.points_for().total_cmp(&b.settings.points_for()))
            .map(|r| r.roster_id);

        let mut ordered: Vec<&Roster> = rosters.iter().collect();
        // Champion first — they finished first overall whatever the regular
        // season said — then everyone else by record and points.
        ordered.sort_by(|a, b| {
            let champ = |r: &Roster| champion == Some(r.roster_id);
            champ(b)
                .cmp(&champ(a))
                .then_with(|| b.settings.wins.cmp(&a.settings.wins))
                .then_with(|| b.settings.points_for().total_cmp(&a.settings.points_for()))
        });
        let rows: Vec<LastSeasonRow> = ordered
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let is_champ = champion == Some(r.roster_id);
                LastSeasonRow {
                    place: i as u32 + 1,
                    name: r
                        .owner_id
                        .as_ref()
                        .and_then(|o| names.get(o).cloned())
                        .unwrap_or_else(|| format!("Team {}", r.roster_id)),
                    record: if r.settings.ties > 0 {
                        format!(
                            "{}\u{2013}{}\u{2013}{}",
                            r.settings.wins, r.settings.losses, r.settings.ties
                        )
                    } else {
                        format!("{}\u{2013}{}", r.settings.wins, r.settings.losses)
                    },
                    points: r.settings.points_for(),
                    tag: if is_champ {
                        Some("Champ".into())
                    } else if most_points == Some(r.roster_id) {
                        Some("Most pts".into())
                    } else {
                        None
                    },
                    is_mine: my_user_id.is_some() && r.owner_id.as_deref() == my_user_id,
                }
            })
            .collect();
        self.write_cache(&name, &rows);
        rows
    }
}
