"""Projections fetcher: the CSV the app's "Import projections CSV…" reads.

What this is
------------
A small pipeline that assembles one season projections CSV out of three
public feeds and writes a provenance manifest next to it:

* **ESPN Mike Clay draft-kit PDF** (default provider) — season projections in
  PPR, converted to the requested scoring using the PDF's own receptions
  column.  `espn_pdf.py`.
* **FantasyPros** — kicker season projections, team-defence rankings, and the
  per-position projection tables used by the `--provider fantasypros` path.
  `feeds.py`.
* **DraftWizard ADP** — average draft position.  `feeds.py`.

Layout
------
``sources.py``   feed URLs and the lookup tables (team codes, name aliases).
``names.py``     column flattening and the cross-provider name key.
``estimates.py`` the two hand-tuned curves that *invent* numbers — read the
                 warning there before trusting anything they touch.
``feeds.py``     the network calls, one function per feed.
``espn_pdf.py``  the Clay PDF adapter.
``build.py``     the merge, the provenance labelling, the metadata manifest.
``fetch_2026_projections.py``  the command line entry point.

Running it
----------
From ``draft-assistant/scripts``::

    python -m projections.fetch_2026_projections --season 2026 \
        --output /tmp/players_2026.csv --metadata-output /tmp/meta_2026.json

Dependencies are in ``requirements.txt`` (pandas, requests, pypdf, lxml).
The CSV it writes is user data and is deliberately **not** committed.

Provenance: why two extra columns exist
---------------------------------------
Not every row in the output is a published projection, and the app cannot
tell them apart by looking.  Every row therefore carries:

``projection_method``
    ``published`` for a number a provider actually published;
    ``adp_estimate`` for one this pipeline *fabricated* from a hand-tuned
    linear curve because no provider had the player (see ``estimates.py``).
``ranking_basis``
    ``season_projection`` for a ranking that is about the whole season;
    ``week_1_matchup_projection`` for the FantasyPros team-defence table,
    which is a week-1 matchup page being used as a season ranking.

The Rust importer (`src-tauri/src/second_opinion.rs`) drops both of the
non-``season_projection``/non-``published`` kinds rather than ranking players
against invented numbers, and tells the user how many rows it dropped.

Two things the pipeline does not invent
---------------------------------------
* **ADP.**  A player the ADP feed never listed keeps an empty ``adp`` cell.
  It is never back-filled from a curve; the app reads the column as optional
  and simply has no market price for that player.
* **Anything for a player nobody projected and nobody drafted.**  Such a row
  never enters the merge at all.

One thing it does invent, and cannot label away
-----------------------------------------------
``estimates.estimate_overall_rank`` fills the ``rank`` column for rows the ADP
feed did not carry, so that the file stays sortable.  Those rows keep
``ranking_basis == season_projection`` because the *basis* is still the
season — but their overall rank is this pipeline's guess, not a market
consensus.  ``estimated_rank_count`` in the metadata manifest counts them.
"""

from .build import build_metadata, build_projection_file, merge_sources, write_projection_artifacts

__all__ = [
    "build_metadata",
    "build_projection_file",
    "merge_sources",
    "write_projection_artifacts",
]
