"""Column flattening, cell splitting, and the cross-provider name key."""

import re
from typing import Dict, List

import pandas as pd

from .sources import NAME_SUFFIX_PATTERN, PLAYER_NAME_ALIASES


def flatten_columns(df: pd.DataFrame) -> pd.DataFrame:
    """Flatten pandas MultiIndex columns from FantasyPros tables."""
    if isinstance(df.columns, pd.MultiIndex):
        flattened: List[str] = []
        seen: Dict[str, int] = {}
        for parts in df.columns:
            clean_parts = [str(part).strip() for part in parts if "Unnamed:" not in str(part)]
            name = "_".join(clean_parts) if clean_parts else str(parts[-1]).strip()
            name = re.sub(r"\s+", "_", name).upper()
            seen[name] = seen.get(name, 0) + 1
            if seen[name] > 1:
                name = f"{name}_{seen[name]}"
            flattened.append(name)
        df = df.copy()
        df.columns = flattened
    else:
        df = df.copy()
        df.columns = [re.sub(r"\s+", "_", str(col).strip()).upper() for col in df.columns]
    return df


def split_player_team(value: object) -> tuple[str, str]:
    """Split FantasyPros player cells such as 'Josh Allen BUF' into name/team."""
    text = str(value).strip()
    match = re.match(r"^(?P<name>.+?)\s+(?P<team>[A-Z]{2,3})$", text)
    if not match:
        return text, ""
    return match.group("name").strip(), match.group("team").strip()


def parse_position_rank(value: object) -> str:
    match = re.match(r"([A-Z]+)", str(value).strip())
    return match.group(1) if match else ""


def parse_position_rank_number(value: object) -> int:
    match = re.search(r"(\d+)", str(value).strip())
    return int(match.group(1)) if match else 999


def normalize_player_name(value: object) -> str:
    """Create a conservative cross-provider identity key."""
    text = re.sub(r"\s+", " ", str(value).strip())
    text = NAME_SUFFIX_PATTERN.sub("", text)
    normalized = text.replace("’", "'").replace(".", "").casefold()
    return PLAYER_NAME_ALIASES.get(normalized, normalized)
