"""The network calls: one function per feed, each returning a DataFrame.

Every row a feed produces is stamped with its provenance here, at the point
where the pipeline still knows where the number came from.
"""

import logging

import pandas as pd

from .names import (
    flatten_columns,
    normalize_player_name,
    parse_position_rank,
    split_player_team,
)
from .sources import (
    DEFENSE_TEAM_CODES,
    FANTASYPROS_ADP_URL,
    FANTASYPROS_PROJECTION_URLS,
    PROJECTION_METHOD_PUBLISHED,
    RANKING_BASIS_SEASON,
    RANKING_BASIS_WEEK_1,
    SPECIAL_TEAMS_PROJECTION_URLS,
    TEAM_NORMALIZATION,
)

logger = logging.getLogger(__name__)


def fetch_projection_rows() -> pd.DataFrame:
    """FantasyPros season projections, one table per offensive position."""
    rows = []
    for position, url in FANTASYPROS_PROJECTION_URLS.items():
        logger.info("Fetching %s projections from %s", position, url)
        table = flatten_columns(pd.read_html(url)[0])
        for idx, row in table.iterrows():
            player_name, team = split_player_team(row.get("PLAYER", ""))
            if not player_name:
                continue
            rows.append(
                {
                    "name": player_name,
                    "position": position,
                    "team": team,
                    "projected_fantasy_points": row.get("MISC_FPTS", row.get("FPTS", 0)),
                    "projection_rank": idx + 1,
                    "projection_method": PROJECTION_METHOD_PUBLISHED,
                    "ranking_basis": RANKING_BASIS_SEASON,
                    "projection_source": url,
                }
            )
    projections = pd.DataFrame(rows)
    projections["projected_fantasy_points"] = pd.to_numeric(
        projections["projected_fantasy_points"], errors="coerce"
    ).fillna(0)
    return projections


def fetch_special_teams_rows() -> pd.DataFrame:
    """Season K projections, and the week-1 D/ST table used as a fallback.

    The defence rows are labelled ``week_1_matchup_projection``: FantasyPros
    publishes no free season defence ranking, so a matchup page stands in for
    one, and the label is what lets the app refuse it.
    """
    rows = []
    for position, url in SPECIAL_TEAMS_PROJECTION_URLS.items():
        logger.info("Fetching %s projections from %s", position, url)
        table = flatten_columns(pd.read_html(url)[0])
        for idx, row in table.iterrows():
            raw_name = str(row.get("PLAYER", "")).strip()
            if position == "K":
                name, team = split_player_team(raw_name)
                ranking_basis = RANKING_BASIS_SEASON
            else:
                team = DEFENSE_TEAM_CODES.get(raw_name, "")
                name = "{} D/ST".format(raw_name)
                ranking_basis = RANKING_BASIS_WEEK_1
            points = pd.to_numeric(row.get("FPTS", row.get("MISC_FPTS", 0)), errors="coerce")
            if not name or not team or pd.isna(points) or float(points) <= 0:
                continue
            rows.append(
                {
                    "name": name,
                    "position": position,
                    "team": team,
                    "projected_fantasy_points": float(points),
                    "projection_rank": idx + 1,
                    "projection_method": PROJECTION_METHOD_PUBLISHED,
                    "projection_source": url,
                    "ranking_basis": ranking_basis,
                }
            )
    return pd.DataFrame(rows)


def fetch_adp_rows() -> pd.DataFrame:
    """DraftWizard average draft position.

    A player this feed does not carry keeps an empty ``adp`` cell downstream;
    nothing in the pipeline fills one in.
    """
    logger.info("Fetching ADP from %s", FANTASYPROS_ADP_URL)
    adp = pd.read_html(FANTASYPROS_ADP_URL)[0]
    adp = adp.rename(
        columns={
            "Position": "position_rank",
            "Overall": "rank",
            "Player": "name",
            "Team (Bye)": "team_bye",
            "Avg Pick": "adp",
        }
    )
    parsed_team_bye = adp["team_bye"].astype(str).str.extract(
        r"(?P<team>[A-Z]{2,3})\s+\((?P<bye_week>[^)]+)\)"
    )
    adp["team"] = parsed_team_bye["team"].fillna("")
    adp["team"] = adp["team"].replace(TEAM_NORMALIZATION)
    adp["bye_week"] = parsed_team_bye["bye_week"].fillna("N/A")
    adp["position"] = adp["position_rank"].apply(parse_position_rank)
    adp["adp"] = pd.to_numeric(adp["adp"], errors="coerce")
    adp["rank"] = pd.to_numeric(adp["rank"], errors="coerce")
    return adp[["rank", "name", "position_rank", "position", "team", "bye_week", "adp"]]


def match_key(frame: pd.DataFrame) -> pd.Series:
    """The name column as a cross-provider join key."""
    return frame["name"].apply(normalize_player_name)
