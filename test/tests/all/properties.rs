//! Properties that must hold for every seed (proptest finds counterexamples and shrinks them).

use proptest::prelude::*;
use simtest::{Scenario, boot};

fn scenario(game: &str, seed: u64) -> Scenario {
    Scenario {
        name: "property".into(),
        game: game.into(),
        seed: Some(seed),
        params: Default::default(),
        switches: Default::default(),
        expect_error: None,
        steps: Vec::new(),
    }
}

fn hashes(game: &str, seed: u64, ticks: usize) -> Vec<u64> {
    let mut e = boot(&scenario(game, seed)).expect("boots");
    (0..ticks).map(|_| e.tick().hash).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    /// Same seed, same world, tick by tick: for any seed.
    #[test]
    fn every_seed_is_deterministic(seed in 0u64..1_000_000) {
        prop_assert_eq!(hashes("games/wolf_sheep", seed, 40), hashes("games/wolf_sheep", seed, 40));
    }

    /// A save at any moment restores the exact future: for any seed and any moment.
    #[test]
    fn a_save_at_any_tick_restores_the_future(seed in 0u64..1_000_000, at in 0u64..120) {
        let mut a = boot(&scenario("games/colony3d", seed)).expect("boots");
        for _ in 0..at { a.tick(); }
        let snap = a.snapshot();
        let future: Vec<u64> = (0..30).map(|_| a.tick().hash).collect();
        let mut b = boot(&scenario("games/colony3d", seed)).expect("boots");
        b.restore(snap).expect("restores");
        prop_assert_eq!(future, (0..30).map(|_| b.tick().hash).collect::<Vec<_>>());
    }

    /// Nobody ever holds negative food or stock, whatever the seed (Need guards double spending).
    #[test]
    fn market_stock_never_goes_negative(seed in 0u64..1_000_000) {
        let mut e = boot(&scenario("games/market", seed)).expect("boots");
        for _ in 0..150 {
            e.tick();
            for x in e.world().entities().values() {
                for (k, v) in &x.props {
                    prop_assert!(*v >= 0 || k == "owner", "{} {} = {} at tick {}", x.kind, k, v, e.world().tick);
                }
            }
        }
    }
}
