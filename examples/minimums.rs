//! Integration tests for algorithms on larger datasets
use std::{
    env, fs,
    path::Path,
    time::{Duration, Instant},
};

use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use zuddy::{ZddHolder, algorithms::subset_cover, serialize::MultipleOwnedZdd};

#[derive(Eq, Clone, Copy, PartialOrd, Ord, PartialEq, Debug, Hash, Deserialize, Serialize)]
struct Id(usize);

#[derive(Eq, Clone, Copy, PartialEq, PartialOrd, Ord, Debug, Hash, Deserialize, Serialize)]
struct WeightedId {
    id: Id,
    weight: u8,
}

fn main() -> anyhow::Result<()> {
    let base = env!("CARGO_MANIFEST_DIR");
    let mut rng = SmallRng::seed_from_u64(32);

    let mut times = vec![];
    for file in ["more_difficult"] {
        //, "test", "complicated_trees", "ten_big"] {
        println!("Doing {file}");
        let big_zdds = Path::new(base).join(format!("examples/resources/{file}.ron"));
        let holder = ZddHolder::new().with_temp_cache_size(2_500_000_000);
        let zdd: MultipleOwnedZdd<WeightedId> =
            ron::from_str(fs::read_to_string(big_zdds)?.as_str())?;
        let mut zdd = zdd.to_set_families(&holder).into_iter().collect::<Vec<_>>();
        zdd.sort_by_key(|x| x.0);
        let mut zdd = zdd.into_iter().map(|(_, v)| v).collect::<Vec<_>>();
        let start = Instant::now();
        zdd.shuffle(&mut rng);
        let all_grammar = subset_cover(&zdd, |x| usize::from(x.weight), None).unwrap();
        let time = start.elapsed();
        let size = all_grammar.min_weight(|x| usize::from(x.weight));

        println!(
            "Done minimising {file} with {} solutions w/ size {size} in {:.3} seconds!",
            all_grammar.size().unwrap(),
            time.as_secs_f64()
        );
        times.push(time);
    }

    let avg = times.iter().sum::<Duration>() / u32::try_from(times.len()).unwrap();
    println!("Average time  = {:.3} seconds", avg.as_secs_f64());
    Ok(())
}
