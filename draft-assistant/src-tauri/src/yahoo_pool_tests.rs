use super::*;

fn player(id: u32) -> YahooPlayer {
    YahooPlayer {
        player_key: format!("449.p.{id}"),
        player_id: id.to_string(),
        full_name: format!("Player {id}"),
        ..YahooPlayer::default()
    }
}

#[test]
fn an_empty_pool_starts_at_page_zero_and_is_not_complete() {
    let pool = PlayerPool::empty();
    assert_eq!(pool.next_start, 0);
    assert!(!pool.complete);
    assert!(pool.players.is_empty());
}

#[test]
fn a_partial_pool_round_trips_through_the_cache_shape() {
    // The failure this prevents: a partial pool written to disk and read back
    // as a whole one, which would leave the board missing every page the
    // throttle interrupted with nothing saying so.
    let partial = PlayerPool {
        players: vec![player(1), player(2)],
        next_start: 75,
        complete: false,
    };
    let text = serde_json::to_string(&partial).expect("serialize");
    let back: PlayerPool = serde_json::from_str(&text).expect("deserialize");
    assert_eq!(back, partial);
    assert!(!back.complete);
    assert_eq!(back.next_start, 75);
}

#[test]
fn a_pool_cached_as_a_bare_list_is_not_mistaken_for_a_partial_one() {
    // Older builds cached `Vec<YahooPlayer>` under the same file name. That
    // shape has to fail to deserialize rather than land as an empty pool, or
    // the first load after an upgrade would show a board with no players.
    let older = serde_json::to_string(&vec![player(1)]).expect("serialize");
    assert!(serde_json::from_str::<PlayerPool>(&older).is_err());
}
