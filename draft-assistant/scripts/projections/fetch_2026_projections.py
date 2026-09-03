"""Command line entry point for the projections pipeline.

See ``projections/__init__.py`` for what the pipeline does, which feeds it
reads, and what the two provenance columns in the CSV mean.

Run it from ``draft-assistant/scripts``::

    python -m projections.fetch_2026_projections --season 2026 \
        --output /tmp/players_2026.csv --metadata-output /tmp/meta_2026.json

The CSV it writes is user data: import it from the app's Settings menu, and
do not commit it.
"""

import argparse
import logging
import sys
from pathlib import Path

if __package__ in (None, ""):  # `python fetch_2026_projections.py`
    sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
    __package__ = "projections"

from .build import build_projection_file, write_projection_artifacts  # noqa: E402
from .sources import DEFAULT_SEASON  # noqa: E402

logging.basicConfig(level=logging.INFO, format="%(asctime)s - %(levelname)s - %(message)s")
logger = logging.getLogger(__name__)


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        description="Fetch full-season projections and DraftWizard ADP"
    )
    parser.add_argument("--season", type=int, default=DEFAULT_SEASON)
    parser.add_argument("--scoring", choices=["standard", "half_ppr", "ppr"], default="half_ppr")
    parser.add_argument("--provider", choices=["espn", "fantasypros"], default="espn")
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--metadata-output", type=Path, default=None)
    args = parser.parse_args(argv)

    output = build_projection_file(season=args.season, scoring=args.scoring, provider=args.provider)
    output_file, metadata_file = write_projection_artifacts(
        output, args.season, args.output, args.metadata_output
    )
    logger.info("Wrote %s projection rows to %s", len(output), output_file)
    logger.info("Wrote projection metadata to %s", metadata_file)


if __name__ == "__main__":
    main()
