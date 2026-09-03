"""The merge, the provenance labelling, and the metadata manifest.

:func:`merge_sources` is deliberately pure — DataFrames in, the output frame
out — so the labelling can be tested without touching the network.
"""

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, Optional, Tuple

import pandas as pd

from .espn_pdf import fetch_espn_projection_rows, projection_url
from .estimates import estimate_overall_rank, estimate_points_from_adp
from .feeds import (
    fetch_adp_rows,
    fetch_projection_rows,
    fetch_special_teams_rows,
    match_key,
)
from .sources import (
    DEFAULT_SEASON,
    FANTASYPROS_ADP_URL,
    FANTASYPROS_PROJECTION_URLS,
    PLAYABLE_POSITIONS,
    PROJECTION_METHOD_ADP_ESTIMATE,
    RANKING_BASIS_SEASON,
    RANKING_BASIS_WEEK_1,
    SPECIAL_TEAMS_PROJECTION_URLS,
)

logger = logging.getLogger(__name__)

OUTPUT_COLUMNS = [
    "rank",
    "name",
    "position",
    "team",
    "bye_week",
    "projected_fantasy_points",
    "tier",
    "adp",
    "projection_method",
    "team_conflict",
    "source",
    "ranking_basis",
]


def assign_tiers(df: pd.DataFrame) -> pd.Series:
    fallback_rank = pd.Series(df.index + 1, index=df.index)
    rank = pd.to_numeric(df["rank"], errors="coerce").fillna(fallback_rank)
    return pd.cut(
        rank,
        bins=[0, 24, 60, 100, 160, float("inf")],
        labels=[1, 2, 3, 4, 5],
        include_lowest=True,
    ).astype(int)


def fill_bye_weeks_from_team(df: pd.DataFrame) -> pd.DataFrame:
    """Fill missing bye weeks from teammates in the provider data.

    Bye weeks belong to teams, but the ADP feed does not contain every player
    returned by the projection feed.  Propagating the provider's team-level
    value keeps projection-only players from losing their bye during the outer
    merge without maintaining a second schedule by hand.
    """
    result = df.copy()
    result["bye_week"] = result["bye_week"].astype("object")
    bye_text = result["bye_week"].fillna("").astype(str).str.strip()
    known = ~bye_text.isin(["", "N/A", "nan"])
    team_byes = (
        result.loc[known & result["team"].fillna("").ne(""), ["team", "bye_week"]]
        .drop_duplicates("team")
        .set_index("team")["bye_week"]
    )
    missing = ~known
    result.loc[missing, "bye_week"] = result.loc[missing, "team"].map(team_byes).fillna("N/A")
    return result


def merge_sources(
    projections: pd.DataFrame,
    special_teams: pd.DataFrame,
    adp: pd.DataFrame,
) -> pd.DataFrame:
    """Join the feeds and stamp every row with how it came to exist.

    Two columns carry that: ``projection_method`` (``published`` for a number
    a provider published, ``adp_estimate`` for one invented by
    :func:`~projections.estimates.estimate_points_from_adp`) and
    ``ranking_basis`` (``season_projection``, or
    ``week_1_matchup_projection`` for the defence table).  Neither is ever
    left blank: a row with no label of its own is a published season row,
    because every other kind is stamped at the feed that produced it.

    ADP is *not* invented.  Rows the ADP feed never carried keep an empty
    ``adp`` and are counted in the manifest instead.
    """
    projections = pd.concat([projections, special_teams], ignore_index=True, sort=False).copy()
    adp = adp.copy()
    for column in ("projection_method", "ranking_basis", "projection_source"):
        if column not in projections.columns:
            projections[column] = pd.NA
    projections["match_name"] = match_key(projections)
    adp["match_name"] = match_key(adp)

    merged = adp.merge(
        projections,
        on=["match_name", "position"],
        how="outer",
        suffixes=("_adp", "_proj"),
    )
    merged["name"] = merged["name_adp"].fillna(merged["name_proj"])
    merged["team"] = merged["team_adp"].fillna("").where(
        merged["team_adp"].fillna("") != "", merged["team_proj"].fillna("")
    )
    merged["team_conflict"] = (
        merged["team_adp"].fillna("").ne("")
        & merged["team_proj"].fillna("").ne("")
        & merged["team_adp"].fillna("").ne(merged["team_proj"].fillna(""))
    )
    merged["rank"] = pd.to_numeric(merged["rank"], errors="coerce")
    missing_rank = merged["rank"].isna()
    # The overall rank of a player the ADP feed never listed is this
    # pipeline's guess. Counted as estimated_rank_count in the manifest.
    merged["estimated_rank"] = missing_rank
    merged.loc[missing_rank, "rank"] = merged[missing_rank].apply(estimate_overall_rank, axis=1)
    merged["adp"] = pd.to_numeric(merged["adp"], errors="coerce")
    merged["bye_week"] = merged["bye_week"].fillna("N/A")
    merged = fill_bye_weeks_from_team(merged)
    merged["projected_fantasy_points"] = pd.to_numeric(
        merged["projected_fantasy_points"], errors="coerce"
    )
    missing_projection = merged["projected_fantasy_points"].isna() | (
        merged["projected_fantasy_points"] <= 0
    )
    merged["projection_method"] = merged["projection_method"].fillna(
        PROJECTION_METHOD_ADP_ESTIMATE
    )
    merged.loc[missing_projection, "projection_method"] = PROJECTION_METHOD_ADP_ESTIMATE
    merged.loc[missing_projection, "projected_fantasy_points"] = merged[missing_projection].apply(
        estimate_points_from_adp, axis=1
    )
    merged["tier"] = assign_tiers(merged)
    merged["source"] = merged["projection_source"].fillna("Local ADP estimate")
    merged["ranking_basis"] = merged["ranking_basis"].fillna(RANKING_BASIS_SEASON)

    output = merged[OUTPUT_COLUMNS + ["estimated_rank"]].copy()
    output = output[output["position"].isin(PLAYABLE_POSITIONS)]
    output = output[
        pd.to_numeric(output["projected_fantasy_points"], errors="coerce").fillna(0) > 0
    ]
    output = output.sort_values(["rank", "projected_fantasy_points"], ascending=[True, False])
    output["rank"] = output["rank"].round(0).astype(int)
    output["projected_fantasy_points"] = output["projected_fantasy_points"].round(1)
    output["adp"] = output["adp"].round(2)
    return output


def build_projection_file(
    season: int = DEFAULT_SEASON,
    scoring: str = "half_ppr",
    provider: str = "espn",
) -> pd.DataFrame:
    """Fetch every feed and merge them into the output frame."""
    if provider == "espn":
        logger.info("Fetching ESPN Mike Clay %s projections for %s", scoring, season)
        projections = fetch_espn_projection_rows(season, scoring=scoring)
        projection_sources = [projection_url(season)]
    elif provider == "fantasypros":
        projections = fetch_projection_rows()
        projection_sources = list(FANTASYPROS_PROJECTION_URLS.values())
    else:
        raise ValueError("provider must be espn or fantasypros")
    projection_sources.extend(SPECIAL_TEAMS_PROJECTION_URLS.values())
    output = merge_sources(projections, fetch_special_teams_rows(), fetch_adp_rows())
    output.attrs["provider"] = provider
    output.attrs["scoring"] = scoring
    output.attrs["projection_sources"] = projection_sources
    return output


def build_metadata(output: pd.DataFrame, season: int, output_file: Path) -> Dict[str, object]:
    """Build a source and coverage manifest alongside the generated CSV."""
    position_counts = output["position"].value_counts().to_dict()
    method_counts = output["projection_method"].value_counts().to_dict()
    basis_counts = output["ranking_basis"].value_counts().to_dict()
    return {
        "schema_version": "1.1",
        "season": season,
        "retrieved_at": datetime.now(timezone.utc).isoformat(),
        "output_file": str(output_file),
        "sources": {
            "projections": output.attrs.get(
                "projection_sources", sorted(output["source"].unique().tolist())
            ),
            "adp": FANTASYPROS_ADP_URL,
        },
        "provider": output.attrs.get("provider", "unknown"),
        "scoring": output.attrs.get("scoring", "unknown"),
        "row_count": len(output),
        "position_counts": {str(key): int(value) for key, value in position_counts.items()},
        "projection_method_counts": {str(key): int(value) for key, value in method_counts.items()},
        "ranking_basis_counts": {str(key): int(value) for key, value in basis_counts.items()},
        "estimated_rank_count": int(output.get("estimated_rank", pd.Series(dtype=bool)).sum()),
        "missing_team_count": int(output["team"].fillna("").eq("").sum()),
        "missing_bye_week_count": int(
            output["bye_week"].fillna("N/A").astype(str).isin(["", "N/A", "nan"]).sum()
        ),
        "missing_adp_count": int(output["adp"].isna().sum()),
        "excluded_by_importer_count": int(
            (
                output["projection_method"].eq(PROJECTION_METHOD_ADP_ESTIMATE)
                | output["ranking_basis"].eq(RANKING_BASIS_WEEK_1)
            ).sum()
        ),
        "duplicate_player_position_count": int(output.duplicated(["name", "position"]).sum()),
        "team_conflict_count": int(output["team_conflict"].fillna(False).astype(bool).sum()),
        "team_conflict_examples": output.loc[
            output["team_conflict"].fillna(False).astype(bool), ["name", "position", "team"]
        ]
        .head(20)
        .to_dict("records"),
    }


def write_projection_artifacts(
    output: pd.DataFrame,
    season: int,
    output_file: Optional[Path] = None,
    metadata_file: Optional[Path] = None,
) -> Tuple[Path, Path]:
    """Write the CSV and its manifest, and say where they went."""
    output_file = Path(output_file or Path("data") / f"players_{season}_positions_bye.csv")
    metadata_file = Path(metadata_file or Path("data") / f"projection_metadata_{season}.json")
    output_file.parent.mkdir(parents=True, exist_ok=True)
    metadata_file.parent.mkdir(parents=True, exist_ok=True)
    metadata = build_metadata(output, season, output_file)
    # estimated_rank is a working column, not part of the published schema.
    output[OUTPUT_COLUMNS].to_csv(output_file, index=False)
    with metadata_file.open("w", encoding="utf-8") as handle:
        json.dump(metadata, handle, indent=2)
        handle.write("\n")
    return output_file, metadata_file
