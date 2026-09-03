"""Feed URLs and the lookup tables the pipeline reads them with."""

import re
from datetime import datetime
from typing import Dict

FANTASYPROS_PROJECTION_URLS: Dict[str, str] = {
    "QB": "https://www.fantasypros.com/nfl/projections/qb.php?week=draft",
    "RB": "https://www.fantasypros.com/nfl/projections/rb.php?week=draft",
    "WR": "https://www.fantasypros.com/nfl/projections/wr.php?week=draft",
    "TE": "https://www.fantasypros.com/nfl/projections/te.php?week=draft",
}
# The kicker table is a season projection. The defence table is not: it is a
# week-1 matchup page, and the only free full-season defence ranking there is.
# Rows built from it are labelled RANKING_BASIS_WEEK_1 so the app can refuse
# to treat a matchup ranking as a season one.
SPECIAL_TEAMS_PROJECTION_URLS: Dict[str, str] = {
    "K": "https://www.fantasypros.com/nfl/projections/k.php?week=draft",
    "DST": "https://www.fantasypros.com/nfl/projections/dst.php?week=1",
}
FANTASYPROS_ADP_URL = "https://draftwizard.fantasypros.com/football/adp/mock-drafts/"

DEFAULT_SEASON = datetime.now().year

# --- the provenance vocabulary the CSV's two extra columns are written in ---

#: A number a provider published.
PROJECTION_METHOD_PUBLISHED = "published"
#: A number this pipeline invented from a curve. See estimates.py.
PROJECTION_METHOD_ADP_ESTIMATE = "adp_estimate"
#: A ranking that is about the whole season.
RANKING_BASIS_SEASON = "season_projection"
#: A ranking that is about week one only, used for want of a season one.
RANKING_BASIS_WEEK_1 = "week_1_matchup_projection"

TEAM_NORMALIZATION = {
    "JAC": "JAX",
    "LA": "LAR",
    "ARZ": "ARI",
    "BLT": "BAL",
    "CLV": "CLE",
    "HST": "HOU",
}
NAME_SUFFIX_PATTERN = re.compile(r"\s+(?:JR\.?|SR\.?|II|III|IV|V)$", re.IGNORECASE)
PLAYER_NAME_ALIASES = {
    "kenny gainwell": "kenneth gainwell",
    "chig okonkwo": "chigoziem okonkwo",
    "ken walker": "kenneth walker",
}
DEFENSE_TEAM_CODES = {
    "Arizona Cardinals": "ARI",
    "Atlanta Falcons": "ATL",
    "Baltimore Ravens": "BAL",
    "Buffalo Bills": "BUF",
    "Carolina Panthers": "CAR",
    "Chicago Bears": "CHI",
    "Cincinnati Bengals": "CIN",
    "Cleveland Browns": "CLE",
    "Dallas Cowboys": "DAL",
    "Denver Broncos": "DEN",
    "Detroit Lions": "DET",
    "Green Bay Packers": "GB",
    "Houston Texans": "HOU",
    "Indianapolis Colts": "IND",
    "Jacksonville Jaguars": "JAX",
    "Kansas City Chiefs": "KC",
    "Las Vegas Raiders": "LV",
    "Los Angeles Chargers": "LAC",
    "Los Angeles Rams": "LAR",
    "Miami Dolphins": "MIA",
    "Minnesota Vikings": "MIN",
    "New England Patriots": "NE",
    "New Orleans Saints": "NO",
    "New York Giants": "NYG",
    "New York Jets": "NYJ",
    "Philadelphia Eagles": "PHI",
    "Pittsburgh Steelers": "PIT",
    "San Francisco 49ers": "SF",
    "Seattle Seahawks": "SEA",
    "Tampa Bay Buccaneers": "TB",
    "Tennessee Titans": "TEN",
    "Washington Commanders": "WAS",
}
PLAYABLE_POSITIONS = ["QB", "RB", "WR", "TE", "K", "DST"]
