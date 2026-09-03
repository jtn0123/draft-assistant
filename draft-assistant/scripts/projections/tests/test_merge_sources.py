"""What the pipeline promises about provenance, tested without the network.

`merge_sources` takes the three feeds as frames, so every claim the module
docstring makes about labelling can be pinned with a handful of fake rows.
"""

import csv
import json
import tempfile
import unittest
from pathlib import Path

import pandas as pd

from ..build import build_metadata, merge_sources, write_projection_artifacts
from ..sources import (
    PROJECTION_METHOD_ADP_ESTIMATE,
    PROJECTION_METHOD_PUBLISHED,
    RANKING_BASIS_SEASON,
    RANKING_BASIS_WEEK_1,
)

CLAY = "https://g.espncdn.com/clay.pdf"
DST_URL = "https://www.fantasypros.com/nfl/projections/dst.php?week=1"


def projections() -> pd.DataFrame:
    """Two published season rows, one of them never drafted in any mock."""
    return pd.DataFrame(
        [
            {
                "name": "Jahmyr Gibbs",
                "position": "RB",
                "team": "DET",
                "projected_fantasy_points": 331.0,
                "projection_rank": 1,
                "projection_method": PROJECTION_METHOD_PUBLISHED,
                "ranking_basis": RANKING_BASIS_SEASON,
                "projection_source": CLAY,
            },
            {
                "name": "Deep Sleeper",
                "position": "WR",
                "team": "SEA",
                "projected_fantasy_points": 88.0,
                "projection_rank": 90,
                "projection_method": PROJECTION_METHOD_PUBLISHED,
                "ranking_basis": RANKING_BASIS_SEASON,
                "projection_source": CLAY,
            },
        ]
    )


def special_teams() -> pd.DataFrame:
    return pd.DataFrame(
        [
            {
                "name": "Denver Broncos D/ST",
                "position": "DST",
                "team": "DEN",
                "projected_fantasy_points": 8.3,
                "projection_rank": 1,
                "projection_method": PROJECTION_METHOD_PUBLISHED,
                "projection_source": DST_URL,
                "ranking_basis": RANKING_BASIS_WEEK_1,
            }
        ]
    )


def adp() -> pd.DataFrame:
    """Gibbs, the defence, and one player nobody published a projection for."""
    return pd.DataFrame(
        [
            {
                "rank": 1.0,
                "name": "Jahmyr Gibbs",
                "position_rank": "RB1",
                "position": "RB",
                "team": "DET",
                "bye_week": "6",
                "adp": 1.42,
            },
            {
                "rank": 168.0,
                "name": "Denver Broncos D/ST",
                "position_rank": "DST1",
                "position": "DST",
                "team": "DEN",
                "bye_week": "10",
                "adp": 170.0,
            },
            {
                "rank": 200.0,
                "name": "Unprojected Rookie",
                "position_rank": "WR80",
                "position": "WR",
                "team": "NYJ",
                "bye_week": "9",
                "adp": 201.5,
            },
        ]
    )


def output() -> pd.DataFrame:
    return merge_sources(projections(), special_teams(), adp())


def row(frame: pd.DataFrame, name: str) -> pd.Series:
    matches = frame[frame["name"] == name]
    assert len(matches) == 1, f"{name} appears {len(matches)} times"
    return matches.iloc[0]


class ProvenanceLabels(unittest.TestCase):
    def test_every_row_carries_both_provenance_columns(self):
        frame = output()
        self.assertFalse(frame["projection_method"].isna().any())
        self.assertFalse(frame["ranking_basis"].isna().any())

    def test_a_published_season_row_says_so(self):
        gibbs = row(output(), "Jahmyr Gibbs")
        self.assertEqual(gibbs["projection_method"], PROJECTION_METHOD_PUBLISHED)
        self.assertEqual(gibbs["ranking_basis"], RANKING_BASIS_SEASON)
        self.assertEqual(gibbs["projected_fantasy_points"], 331.0)

    def test_a_row_whose_points_were_invented_is_labelled_adp_estimate(self):
        rookie = row(output(), "Unprojected Rookie")
        self.assertEqual(rookie["projection_method"], PROJECTION_METHOD_ADP_ESTIMATE)
        # The curve produced *something*, which is exactly why the label has
        # to travel with it — the number looks like every other number.
        self.assertGreater(rookie["projected_fantasy_points"], 0)

    def test_the_defence_keeps_its_week_one_basis(self):
        denver = row(output(), "Denver Broncos D/ST")
        self.assertEqual(denver["ranking_basis"], RANKING_BASIS_WEEK_1)
        self.assertEqual(denver["projection_method"], PROJECTION_METHOD_PUBLISHED)


class MissingAdp(unittest.TestCase):
    def test_a_player_with_no_adp_keeps_an_empty_cell(self):
        sleeper = row(output(), "Deep Sleeper")
        self.assertTrue(pd.isna(sleeper["adp"]), sleeper["adp"])
        # His points are the published ones; only the *rank* was guessed.
        self.assertEqual(sleeper["projected_fantasy_points"], 88.0)
        self.assertEqual(sleeper["projection_method"], PROJECTION_METHOD_PUBLISHED)
        self.assertTrue(sleeper["estimated_rank"])

    def test_an_adp_less_row_reaches_the_csv_with_the_cell_left_blank(self):
        with tempfile.TemporaryDirectory() as directory:
            csv_path = Path(directory) / "players.csv"
            meta_path = Path(directory) / "meta.json"
            write_projection_artifacts(output(), 2026, csv_path, meta_path)
            with csv_path.open(encoding="utf-8") as handle:
                rows = list(csv.DictReader(handle))
        sleeper = next(r for r in rows if r["name"] == "Deep Sleeper")
        self.assertEqual(sleeper["adp"], "")
        # The working column stays out of the published schema.
        self.assertNotIn("estimated_rank", rows[0])
        self.assertIn("projection_method", rows[0])
        self.assertIn("ranking_basis", rows[0])


class Manifest(unittest.TestCase):
    def test_the_manifest_counts_what_the_importer_will_drop(self):
        metadata = build_metadata(output(), 2026, Path("players.csv"))
        self.assertEqual(metadata["projection_method_counts"][PROJECTION_METHOD_ADP_ESTIMATE], 1)
        self.assertEqual(metadata["ranking_basis_counts"][RANKING_BASIS_WEEK_1], 1)
        # One estimated row plus one week-1 defence row.
        self.assertEqual(metadata["excluded_by_importer_count"], 2)
        self.assertEqual(metadata["missing_adp_count"], 1)
        self.assertEqual(metadata["estimated_rank_count"], 1)

    def test_the_manifest_is_json_serialisable(self):
        with tempfile.TemporaryDirectory() as directory:
            meta_path = Path(directory) / "meta.json"
            write_projection_artifacts(
                output(), 2026, Path(directory) / "players.csv", meta_path
            )
            metadata = json.loads(meta_path.read_text())
        self.assertEqual(metadata["schema_version"], "1.1")
        self.assertEqual(metadata["row_count"], 4)


if __name__ == "__main__":
    unittest.main()
