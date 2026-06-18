use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use std::{
    cell::UnsafeCell,
    hash::{Hash, Hasher},
    ops::Range,
    sync::{
        Arc, Mutex,
        atomic::{
            AtomicBool, AtomicU64,
            Ordering::{self, Relaxed},
        },
    },
};
use thiserror::Error;

use ahash::HashSet;
use serde::{Deserialize, Serialize};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned, transmute};

use crate::{ZddHolder, manager::ZddIndex};

const N_BYTES_PER_HASH: usize = 3;
const N_BYTES_PER_INDEX: usize = 5;

#[derive(Copy, Clone, IntoBytes, FromBytes, Immutable, Unaligned, PartialEq, Eq, KnownLayout)]
#[repr(C)]
struct HashEntry {
    hash: [u8; N_BYTES_PER_HASH],
    index: [u8; N_BYTES_PER_INDEX],
}

impl std::fmt::Debug for HashEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HashEntry")
            .field("hash", &self.hash())
            .field("index", &self.index())
            .finish()
    }
}

const EMPTY: HashEntry = HashEntry {
    hash: [0; N_BYTES_PER_HASH],
    index: [0; N_BYTES_PER_INDEX],
};

impl HashEntry {
    #[expect(clippy::many_single_char_names)]
    fn new(index: usize, hash: u32) -> Self {
        debug_assert!(hash < 1_u32 << (N_BYTES_PER_HASH * 8));
        let [_, a, b, c] = u32::to_be_bytes(hash);
        let [_, _, _, d, e, f, g, h] = u64::to_be_bytes(index as u64);

        HashEntry {
            hash: [a, b, c],
            index: [d, e, f, g, h],
        }
    }

    fn index(self) -> usize {
        usize::try_from(u64::from_be_bytes([
            0,
            0,
            0,
            self.index[0],
            self.index[1],
            self.index[2],
            self.index[3],
            self.index[4],
        ]))
        .unwrap()
    }

    fn hash(self) -> u32 {
        u32::from_be_bytes([0, self.hash[0], self.hash[1], self.hash[2]])
    }

    fn is_empty(self) -> bool {
        self == EMPTY
    }

    fn to_u64(self) -> u64 {
        transmute!(self)
    }

    fn from_u64(x: u64) -> Self {
        transmute!(x)
    }

    fn from_atomic_u64(x: &AtomicU64) -> Self {
        transmute!(x.load(Relaxed))
    }
}

#[derive(Debug)]
pub(super) struct HashTable<V> {
    pools: ThreadPool,
    hashes: Vec<AtomicU64>,
    databits: Vec<AtomicBool>,
    regionbits: Vec<AtomicBool>,
    data: Vec<UnsafeCell<Option<V>>>,
    current_region: Vec<AtomicU64>,
    counts: Vec<AtomicU64>,
}

unsafe impl<V> Sync for HashTable<V> {}

fn zeroed_atomic(n: usize) -> Vec<AtomicU64> {
    vec![0u64; n].into_iter().map(AtomicU64::new).collect()
}

fn zeroed_atomic_bool(n: usize) -> Vec<AtomicBool> {
    vec![false; n].into_iter().map(AtomicBool::new).collect()
}

fn noned_unsafe_cell<V: Clone>(n: usize) -> Vec<UnsafeCell<Option<V>>> {
    vec![None; n].into_iter().map(UnsafeCell::new).collect()
}

impl<V: Hash + Eq> ZddHolder<V> {
    pub(super) fn inc_count(&self, i: usize) {
        self.uniq_table.inc_count(i);
    }

    pub(super) fn dec_count(&self, i: usize) {
        self.uniq_table.dec_count(i);
    }

    pub(super) fn n_pools(&self) -> usize {
        self.uniq_table.pools.current_num_threads()
    }

    pub(crate) fn pools(&self) -> &ThreadPool {
        &self.uniq_table.pools
    }
}

impl<V: Hash + Eq + Send + Sync> ZddHolder<V> {
    pub(super) fn used_variables(&self) -> impl ParallelIterator<Item = ZddIndex<V>> {
        self.uniq_table
            .counts
            .par_iter()
            .enumerate()
            .filter_map(|(i, x)| {
                if x.load(Relaxed) != 0 {
                    Some(ZddIndex::from(i))
                } else {
                    None
                }
            })
    }
}
impl<V: Hash + Eq> HashTable<V> {
    fn inc_count(&self, i: usize) {
        self.counts[i].fetch_add(1, Relaxed);
    }

    fn dec_count(&self, i: usize) {
        self.counts[i].fetch_sub(1, Relaxed);
    }
}

#[derive(Debug, Error)]
#[error("The table is full!")]
pub(crate) struct FullTable;

const REGION_SIZE: usize = 512;

impl<V: Clone + Hash + Eq> HashTable<V> {
    pub fn new(size: usize, n_pools: usize) -> Self {
        let pools = ThreadPoolBuilder::new()
            .num_threads(n_pools)
            .build()
            .unwrap();

        let n_region_bits = size * n_pools;
        let size = size * REGION_SIZE * n_pools;
        let databits = zeroed_atomic_bool(size);
        databits[0].store(true, Ordering::SeqCst);
        databits[1].store(true, Ordering::SeqCst);
        let gap = n_region_bits / n_pools;
        let current_region = (0..n_pools)
            .map(|x| AtomicU64::from(u64::try_from(x * gap).unwrap()))
            .collect();

        HashTable {
            pools,
            hashes: zeroed_atomic(size),
            databits,
            counts: zeroed_atomic(size),
            regionbits: zeroed_atomic_bool(n_region_bits),
            data: noned_unsafe_cell(size),
            current_region,
        }
    }

    fn capacity(&self) -> u64 {
        u64::try_from(self.data.len()).unwrap()
    }

    fn hash(&self, data: &V) -> u32 {
        let mut h = ahash::AHasher::default();
        data.hash(&mut h);
        let hash = h.finish().saturating_add(2);
        let h = (hash % (1_u64 << (N_BYTES_PER_HASH * 8))) % self.capacity();
        h as u32
    }

    fn probe(&self, h: u32) -> impl Iterator<Item = (usize, HashEntry)> {
        self.hashes
            .iter()
            .enumerate()
            .skip(usize::try_from(h).unwrap())
            .map(|(i, h)| (i, HashEntry::from_atomic_u64(h)))
    }

    fn equal_at_index(&self, i: usize, data: &V) -> bool {
        unsafe { (&*self.data[i].get()).as_ref() == Some(data) }
    }

    unsafe fn write_at_index(&self, i: usize, data: V) {
        unsafe {
            let h = &mut *self.data[i].get();
            *h = Some(data);
        }
    }

    fn set_region_id(&self, x: u64) {
        let r = &self.current_region[self.thread_id()];
        r.store(x, Relaxed);
    }

    fn thread_id(&self) -> usize {
        self.pools.current_thread_index().unwrap_or_else(|| {
            rayon::current_thread_index().unwrap_or(0) % self.current_region.len()
        })
    }

    fn region_id(&self) -> usize {
        let r = &self.current_region[self.thread_id()];
        usize::try_from(r.load(Relaxed)).unwrap()
    }

    fn region(&self) -> Range<usize> {
        let region = self.region_id();
        (REGION_SIZE * region)..(REGION_SIZE * region + REGION_SIZE - 1)
    }

    ///Tries to claim a new region for the current thread id and returns whether it could.
    fn claim_region(&self) -> Result<(), FullTable> {
        let old_region = self.region_id();
        let mut new_region = (old_region + 1) % (self.data.len() / REGION_SIZE);

        while old_region != new_region {
            if !self.regionbits[new_region].load(Relaxed)
                && let Ok(_) =
                    self.regionbits[new_region].compare_exchange(false, true, Relaxed, Relaxed)
            {
                self.set_region_id(u64::try_from(new_region).unwrap());
                return Ok(());
            }
            new_region = (new_region + 1) % (self.data.len() / REGION_SIZE);
        }
        Err(FullTable)
    }

    fn reserve_data_bucket(&self) -> Result<usize, FullTable> {
        loop {
            if let Some(index) = self.region().find(|i| !self.databits[*i].load(Relaxed)) {
                self.databits[index].store(true, Relaxed);
                return Ok(index);
            }

            self.claim_region()?;
        }
    }

    pub(crate) fn get(&self, i: usize) -> Option<V> {
        unsafe { (&*self.data[i].get()).clone() }
    }

    pub(crate) fn find_or_insert(&self, data: V) -> Result<usize, FullTable> {
        let h = self.hash(&data);
        let mut index = 0;
        for (s, mut v) in self.probe(h) {
            if v.is_empty() {
                if index == 0 {
                    //will only happen once so maybe there's a way to avoid the clone here
                    index = self.reserve_data_bucket()?;
                    unsafe {
                        self.write_at_index(index, data.clone());
                    }
                }
                match self.hashes[s].compare_exchange(
                    0,
                    HashEntry::new(index, h).to_u64(),
                    Relaxed,
                    Relaxed,
                ) {
                    Ok(_) => {
                        return Ok(index);
                    }
                    Err(new_v) => v = HashEntry::from_u64(new_v),
                }
            }

            if v.hash() == h && self.equal_at_index(v.index(), &data) {
                if index != 0 {
                    self.databits[index].store(false, Relaxed);
                }
                return Ok(v.index());
            }
        }
        Err(FullTable)
    }
}

#[cfg(test)]
mod test {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use rayon::ThreadPoolBuilder;

    #[test]
    fn hash_table_with_other_pool() -> Result<(), FullTable> {
        let hash_table = HashTable::<char>::new(100, 8);
        let used_chars = MOBY.chars().collect::<HashSet<_>>();

        let vals = MOBY
            .par_chars()
            .map(|k| hash_table.find_or_insert(k).map(|v| (k, v)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut map = HashMap::new();
        for (k, v) in vals {
            let old_v = *map.entry(k).or_insert(v);
            assert!(old_v == v, "{k} cannot be at {old_v} and {v}");
        }
        assert_eq!(map.len(), used_chars.len());
        Ok(())
    }

    #[test]
    fn hash_table() -> Result<(), FullTable> {
        assert_eq!(size_of::<HashEntry>(), size_of::<usize>());
        let pools = ThreadPoolBuilder::new().num_threads(8).build().unwrap();
        let hash_table = HashTable::<char>::new(100, 8);
        let vals = pools.install(|| {
            "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG"
                .par_chars()
                .map(|k| hash_table.find_or_insert(k).map(|v| (k, v)))
                .collect::<Result<Vec<_>, _>>()
        })?;
        println!("{vals:?}");
        let mut map = HashMap::new();
        for (k, v) in vals {
            assert_eq!(*map.entry(k).or_insert(v), v);
        }
        assert_eq!(map.len(), 27);

        let hash_table = HashTable::<char>::new(8, 8);
        let used_chars = MOBY.chars().collect::<HashSet<_>>();

        let vals = pools.install(|| {
            MOBY.par_chars()
                .map(|k| hash_table.find_or_insert(k).map(|v| (k, v)))
                .collect::<Result<Vec<_>, _>>()
        })?;
        let mut map = HashMap::new();
        for (k, v) in vals {
            let old_v = *map.entry(k).or_insert(v);
            assert!(old_v == v, "{k} cannot be at {old_v} and {v}");
        }
        assert_eq!(map.len(), used_chars.len());
        Ok(())
    }

    const MOBY: &str = "Call me Ishmael. Some years ago—never mind how long precisely—having little or no money in my purse, and nothing particular to interest me on shore, I thought I would sail about a little and see the watery part of the world. It is a way I have of driving off the spleen and regulating the circulation. Whenever I find myself growing grim about the mouth; whenever it is a damp, drizzly November in my soul; whenever I find myself involuntarily pausing before coffin warehouses, and bringing up the rear of every funeral I meet; and especially whenever my hypos get such an upper hand of me, that it requires a strong moral principle to prevent me from deliberately stepping into the street, and methodically knocking people’s hats off—then, I account it high time to get to sea as soon as I can. This is my substitute for pistol and ball. With a philosophical flourish Cato throws himself upon his sword; I quietly take to the ship. There is nothing surprising in this. If they but knew it, almost all men in their degree, some time or other, cherish very nearly the same feelings towards the ocean with me.

There now is your insular city of the Manhattoes, belted round by wharves as Indian isles by coral reefs—commerce surrounds it with her surf. Right and left, the streets take you waterward. Its extreme downtown is the battery, where that noble mole is washed by waves, and cooled by breezes, which a few hours previous were out of sight of land. Look at the crowds of water-gazers there.

Circumambulate the city of a dreamy Sabbath afternoon. Go from Corlears Hook to Coenties Slip, and from thence, by Whitehall, northward. What do you see?—Posted like silent sentinels all around the town, stand thousands upon thousands of mortal men fixed in ocean reveries. Some leaning against the spiles; some seated upon the pier-heads; some looking over the bulwarks of ships from China; some high aloft in the rigging, as if striving to get a still better seaward peep. But these are all landsmen; of week days pent up in lath and plaster—tied to counters, nailed to benches, clinched to desks. How then is this? Are the green fields gone? What do they here?

But look! here come more crowds, pacing straight for the water, and seemingly bound for a dive. Strange! Nothing will content them but the extremest limit of the land; loitering under the shady lee of yonder warehouses will not suffice. No. They must get just as nigh the water as they possibly can without falling in. And there they stand—miles of them—leagues. Inlanders all, they come from lanes and alleys, streets and avenues—north, east, south, and west. Yet here they all unite. Tell me, does the magnetic virtue of the needles of the compasses of all those ships attract them thither?

Once more. Say you are in the country; in some high land of lakes. Take almost any path you please, and ten to one it carries you down in a dale, and leaves you there by a pool in the stream. There is magic in it. Let the most absent-minded of men be plunged in his deepest reveries—stand that man on his legs, set his feet a-going, and he will infallibly lead you to water, if water there be in all that region. Should you ever be athirst in the great American desert, try this experiment, if your caravan happen to be supplied with a metaphysical professor. Yes, as every one knows, meditation and water are wedded for ever.

But here is an artist. He desires to paint you the dreamiest, shadiest, quietest, most enchanting bit of romantic landscape in all the valley of the Saco. What is the chief element he employs? There stand his trees, each with a hollow trunk, as if a hermit and a crucifix were within; and here sleeps his meadow, and there sleep his cattle; and up from yonder cottage goes a sleepy smoke. Deep into distant woodlands winds a mazy way, reaching to overlapping spurs of mountains bathed in their hill-side blue. But though the picture lies thus tranced, and though this pine-tree shakes down its sighs like leaves upon this shepherd’s head, yet all were vain, unless the shepherd’s eye were fixed upon the magic stream before him. Go visit the Prairies in June, when for scores on scores of miles you wade knee-deep among Tiger-lilies—what is the one charm wanting?—Water—there is not a drop of water there! Were Niagara but a cataract of sand, would you travel your thousand miles to see it? Why did the poor poet of Tennessee, upon suddenly receiving two handfuls of silver, deliberate whether to buy him a coat, which he sadly needed, or invest his money in a pedestrian trip to Rockaway Beach? Why is almost every robust healthy boy with a robust healthy soul in him, at some time or other crazy to go to sea? Why upon your first voyage as a passenger, did you yourself feel such a mystical vibration, when first told that you and your ship were now out of sight of land? Why did the old Persians hold the sea holy? Why did the Greeks give it a separate deity, and own brother of Jove? Surely all this is not without meaning. And still deeper the meaning of that story of Narcissus, who because he could not grasp the tormenting, mild image he saw in the fountain, plunged into it and was drowned. But that same image, we ourselves see in all rivers and oceans. It is the image of the ungraspable phantom of life; and this is the key to it all.

Now, when I say that I am in the habit of going to sea whenever I begin to grow hazy about the eyes, and begin to be over conscious of my lungs, I do not mean to have it inferred that I ever go to sea as a passenger. For to go as a passenger you must needs have a purse, and a purse is but a rag unless you have something in it. Besides, passengers get sea-sick—grow quarrelsome—don’t sleep of nights—do not enjoy themselves much, as a general thing;—no, I never go as a passenger; nor, though I am something of a salt, do I ever go to sea as a Commodore, or a Captain, or a Cook. I abandon the glory and distinction of such offices to those who like them. For my part, I abominate all honorable respectable toils, trials, and tribulations of every kind whatsoever. It is quite as much as I can do to take care of myself, without taking care of ships, barques, brigs, schooners, and what not. And as for going as cook,—though I confess there is considerable glory in that, a cook being a sort of officer on ship-board—yet, somehow, I never fancied broiling fowls;—though once broiled, judiciously buttered, and judgmatically salted and peppered, there is no one who will speak more respectfully, not to say reverentially, of a broiled fowl than I will. It is out of the idolatrous dotings of the old Egyptians upon broiled ibis and roasted river horse, that you see the mummies of those creatures in their huge bake-houses the pyramids.

No, when I go to sea, I go as a simple sailor, right before the mast, plumb down into the forecastle, aloft there to the royal mast-head. True, they rather order me about some, and make me jump from spar to spar, like a grasshopper in a May meadow. And at first, this sort of thing is unpleasant enough. It touches one’s sense of honor, particularly if you come of an old established family in the land, the Van Rensselaers, or Randolphs, or Hardicanutes. And more than all, if just previous to putting your hand into the tar-pot, you have been lording it as a country schoolmaster, making the tallest boys stand in awe of you. The transition is a keen one, I assure you, from a schoolmaster to a sailor, and requires a strong decoction of Seneca and the Stoics to enable you to grin and bear it. But even this wears off in time.

What of it, if some old hunks of a sea-captain orders me to get a broom and sweep down the decks? What does that indignity amount to, weighed, I mean, in the scales of the New Testament? Do you think the archangel Gabriel thinks anything the less of me, because I promptly and respectfully obey that old hunks in that particular instance? Who ain’t a slave? Tell me that. Well, then, however the old sea-captains may order me about—however they may thump and punch me about, I have the satisfaction of knowing that it is all right; that everybody else is one way or other served in much the same way—either in a physical or metaphysical point of view, that is; and so the universal thump is passed round, and all hands should rub each other’s shoulder-blades, and be content.

Again, I always go to sea as a sailor, because they make a point of paying me for my trouble, whereas they never pay passengers a single penny that I ever heard of. On the contrary, passengers themselves must pay. And there is all the difference in the world between paying and being paid. The act of paying is perhaps the most uncomfortable infliction that the two orchard thieves entailed upon us. But being paid,—what will compare with it? The urbane activity with which a man receives money is really marvellous, considering that we so earnestly believe money to be the root of all earthly ills, and that on no account can a monied man enter heaven. Ah! how cheerfully we consign ourselves to perdition!

Finally, I always go to sea as a sailor, because of the wholesome exercise and pure air of the fore-castle deck. For as in this world, head winds are far more prevalent than winds from astern (that is, if you never violate the Pythagorean maxim), so for the most part the Commodore on the quarter-deck gets his atmosphere at second hand from the sailors on the forecastle. He thinks he breathes it first; but not so. In much the same way do the commonalty lead their leaders in many other things, at the same time that the leaders little suspect it. But wherefore it was that after having repeatedly smelt the sea as a merchant sailor, I should now take it into my head to go on a whaling voyage; this the invisible police officer of the Fates, who has the constant surveillance of me, and secretly dogs me, and influences me in some unaccountable way—he can better answer than any one else. And, doubtless, my going on this whaling voyage, formed part of the grand programme of Providence that was drawn up a long time ago. It came in as a sort of brief interlude and solo between more extensive performances. I take it that this part of the bill must have run something like this:

“Grand Contested Election for the Presidency of the United States. “WHALING VOYAGE BY ONE ISHMAEL. “BLOODY BATTLE IN AFFGHANISTAN.”

Though I cannot tell why it was exactly that those stage managers, the Fates, put me down for this shabby part of a whaling voyage, when others were set down for magnificent parts in high tragedies, and short and easy parts in genteel comedies, and jolly parts in farces—though I cannot tell why this was exactly; yet, now that I recall all the circumstances, I think I can see a little into the springs and motives which being cunningly presented to me under various disguises, induced me to set about performing the part I did, besides cajoling me into the delusion that it was a choice resulting from my own unbiased freewill and discriminating judgment.

Chief among these motives was the overwhelming idea of the great whale himself. Such a portentous and mysterious monster roused all my curiosity. Then the wild and distant seas where he rolled his island bulk; the undeliverable, nameless perils of the whale; these, with all the attending marvels of a thousand Patagonian sights and sounds, helped to sway me to my wish. With other men, perhaps, such things would not have been inducements; but as for me, I am tormented with an everlasting itch for things remote. I love to sail forbidden seas, and land on barbarous coasts. Not ignoring what is good, I am quick to perceive a horror, and could still be social with it—would they let me—since it is but well to be on friendly terms with all the inmates of the place one lodges in.

By reason of these things, then, the whaling voyage was welcome; the great flood-gates of the wonder-world swung open, and in the wild conceits that swayed me to my purpose, two and two there floated into my inmost soul, endless processions of the whale, and, mid most of them all, one grand hooded phantom, like a snow hill in the air.";
}
