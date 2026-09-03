"""The two hand-tuned curves that invent numbers.

Nothing in here is a projection or a consensus. Both functions are linear
fits somebody eyeballed once, and every row either of them touches is a row
the app must be able to recognise and refuse:

* :func:`estimate_points_from_adp` stamps ``projection_method=adp_estimate``
  on the row it fills, and the importer drops those rows outright.
* :func:`estimate_overall_rank` fills the ``rank`` column for players the ADP
  feed never listed, so the file stays sortable. It cannot be labelled the
  same way without throwing away a third of the file, so it is counted
  instead — ``estimated_rank_count`` in the metadata manifest.
"""

import pandas as pd

from .names import parse_position_rank_number


def estimate_points_from_adp(row: pd.Series) -> float:
    """Estimate fantasy points for ADP-only rows when projections are paginated/truncated."""
    position = str(row.get("position", ""))
    pos_rank = parse_position_rank_number(row.get("position_rank", ""))
    curves = {
        "QB": (335, 7.0, 90),
        "RB": (275, 4.0, 35),
        "WR": (230, 2.4, 35),
        "TE": (170, 3.2, 25),
    }
    if position not in curves:
        return 0.0
    start, slope, floor = curves[position]
    return max(floor, start - ((pos_rank - 1) * slope))


def estimate_overall_rank(row: pd.Series) -> float:
    """Estimate overall rank for projection-only players without ADP."""
    position = str(row.get("position", ""))
    pos_rank = pd.to_numeric(row.get("projection_rank", None), errors="coerce")
    if pd.isna(pos_rank):
        pos_rank = parse_position_rank_number(row.get("position_rank", ""))
    curves = {
        "RB": (0, 3.0),
        "WR": (0, 2.6),
        "TE": (12, 6.0),
        "QB": (18, 6.5),
        "DST": (165, 3.0),
        "K": (180, 3.0),
    }
    offset, multiplier = curves.get(position, (100, 4.0))
    return offset + (float(pos_rank) * multiplier)
