use std::{
    cell::UnsafeCell,
    fmt::Debug,
    hash::{Hash, Hasher},
    sync::{
        Condvar, Mutex,
        atomic::{
            AtomicBool, AtomicU64,
            Ordering::{Acquire, Relaxed, Release, SeqCst},
        },
    },
};
use thiserror::Error;

use crate::{
    ZddHolder,
    manager::{
        RawZddData,
        hashtable::slots::{REGION_SIZE, Slots},
    },
};

pub(crate) mod slots;

const N_BITS_FOR_HASH: u32 = 24;

///A u64 which contains the index and hash internally.
#[derive(Copy, Clone, PartialEq, Eq)]
struct HashEntry(u64);

impl std::fmt::Debug for HashEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HashEntry")
            .field("hash", &self.hash())
            .field("index", &self.index())
            .finish()
    }
}

const EMPTY: HashEntry = HashEntry(0);

impl HashEntry {
    fn new(index: usize, hash: u32) -> Self {
        debug_assert!(hash < 1_u32 << N_BITS_FOR_HASH);
        debug_assert!(u64::try_from(index).unwrap() < 1_u64 << (64 - N_BITS_FOR_HASH));
        HashEntry((u64::from(hash)) | (index as u64) << N_BITS_FOR_HASH)
    }

    fn index(self) -> usize {
        (self.0 >> N_BITS_FOR_HASH) as usize
    }

    fn hash(self) -> u32 {
        (self.0 & ((1 << N_BITS_FOR_HASH) - 1)) as u32
    }

    fn is_empty(self) -> bool {
        self == EMPTY
    }
}

///Iterate over `n_slot` ids starting at `s`, skipping 0, 1.
struct ProbeSlots {
    s: usize,
    n_slots: usize,
    left: usize,
}

impl Iterator for ProbeSlots {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        if self.left == 0 {
            return None;
        }
        let s = self.s;
        self.s = if s + 1 >= self.n_slots { 2 } else { s + 1 };
        self.left -= 1;
        Some(s)
    }
}

#[derive(Debug)]
pub(super) struct HashTable<V> {
    slots: Slots,
    hashes: UnsafeCell<Vec<AtomicU64>>,
    data: UnsafeCell<Vec<Option<RawZddData<V>>>>,
    counts: UnsafeCell<Vec<AtomicU64>>,
    reading: Vec<AtomicU64>,
    is_paused: AtomicBool,
    paused: Mutex<bool>,
    unpaused_cond_var: Condvar,
}

unsafe impl<V: Send + Sync> Sync for HashTable<V> {}

fn zeroed_atomic(n: usize) -> Vec<AtomicU64> {
    vec![0u64; n].into_iter().map(AtomicU64::new).collect()
}

impl<V: Hash + Eq> ZddHolder<V> {
    pub(super) fn inc_count(&self, i: usize) {
        self.uniq_table.inc_count(i);
    }

    pub(super) fn dec_count(&self, i: usize) {
        self.uniq_table.dec_count(i);
    }
}

impl<V: Hash + Eq> HashTable<V> {
    pub(super) fn inc_count(&self, i: usize) {
        unsafe { &(&*self.counts.get())[i] }.fetch_add(1, Relaxed);
    }

    fn dec_count(&self, i: usize) {
        unsafe { &(&*self.counts.get())[i] }.fetch_sub(1, Relaxed);
    }

    ///Whether index `i` is held by something that hasn't been dropped in the [`HashTable`]
    pub(super) fn is_held(&self, i: usize) -> bool {
        unsafe { &(&*self.counts.get())[i] }.load(Relaxed) != 0
    }

    pub(super) fn n_used(&self) -> usize {
        self.slots.n_used()
    }

    pub(super) fn n_claimed_regions(&self) -> usize {
        self.slots.n_claimed_regions()
    }
    pub(super) fn n_regions(&self) -> usize {
        self.slots.n_regions()
    }

    pub fn len(&self) -> usize {
        unsafe { &*self.data.get() }.len()
    }
}

#[derive(Debug, Error)]
#[error("The table is full, with {n_used}/{size} filled !")]
pub(crate) struct FullTable<V> {
    pub(crate) size: usize,
    pub(crate) n_used: usize,
    pub(crate) value: RawZddData<V>,
}
impl<V: Clone + Hash + Eq> HashTable<V> {
    pub fn new(size: usize, n_pools: usize) -> Self {
        //No point having more pools than regions, since each pool thread gets its own region.
        let n_pools = n_pools.min(size.div_ceil(REGION_SIZE)).max(1);
        //ensure size is multiple of region_size
        let size = size
            .max(n_pools * REGION_SIZE)
            .div_ceil(n_pools * REGION_SIZE)
            * n_pools
            * REGION_SIZE;
        let slots = Slots::new(size, n_pools);

        HashTable {
            slots,
            hashes: UnsafeCell::new(zeroed_atomic(size)),
            counts: UnsafeCell::new(zeroed_atomic(size)),
            data: UnsafeCell::new(vec![None; size]),
            is_paused: AtomicBool::from(false),
            reading: zeroed_atomic(n_pools),
            unpaused_cond_var: Condvar::new(),
            paused: Mutex::new(false),
        }
    }

    pub(super) fn start_gc(&self) -> bool {
        let mut paused = self.paused.lock().unwrap();
        if *paused {
            return false;
        }
        *paused = true;
        self.is_paused.store(true, SeqCst);
        drop(paused);
        while self.reading.iter().any(|x| x.load(Acquire) != 0) {
            std::hint::spin_loop();
        }
        true
    }

    pub fn end_gc(&self) {
        let mut paused = self.paused.lock().unwrap();
        *paused = false;
        self.is_paused.store(false, SeqCst);
        drop(paused);
        self.unpaused_cond_var.notify_all();
    }

    pub(crate) fn capacity(&self) -> usize {
        unsafe { &*self.data.get() }.len()
    }

    fn hash(data: &RawZddData<V>) -> u32 {
        let mut h = ahash::AHasher::default();
        data.hash(&mut h);
        let hash = h.finish();
        (hash & ((1_u64 << N_BITS_FOR_HASH) - 1)) as u32
    }
    #[must_use]
    fn bucket_start(&self, h: u32) -> usize {
        let s = ((u64::from(h) * self.capacity() as u64) >> N_BITS_FOR_HASH) as usize;
        s.max(2)
    }

    ///The slots to probe for a hash, in order, wrapping past the terminals.
    fn probe_slots(&self, h: u32) -> ProbeSlots {
        let n = self.capacity();
        ProbeSlots {
            s: self.bucket_start(h),
            n_slots: n,
            left: n,
        }
    }

    fn hashes(&self) -> &Vec<AtomicU64> {
        unsafe { &*self.hashes.get() }
    }

    fn equal_at_index(&self, i: usize, data: &RawZddData<V>) -> bool {
        unsafe { (&*self.data.get())[i].as_ref() == Some(data) }
    }

    pub(super) unsafe fn get_unchecked(&self, i: usize) -> Option<RawZddData<V>> {
        unsafe { (&*self.data.get())[i].clone() }
    }

    pub(crate) fn get(&self, i: usize) -> Option<RawZddData<V>> {
        let reading = self.read_table();
        let x = unsafe { (&*self.data.get())[i].clone() };
        drop(reading);
        x
    }

    #[expect(clippy::cast_precision_loss)]
    pub(crate) fn usage(&self) -> f64 {
        let count = self.n_used();
        (count) as f64 / unsafe { &*self.data.get() }.len() as f64
    }

    pub(crate) fn find_or_insert(
        &self,
        data: RawZddData<V>,
    ) -> Result<(usize, Option<usize>), FullTable<V>> {
        let read = self.read_table();
        let h = Self::hash(&data);
        let hashes = self.hashes();
        let mut index = 0;

        for (probe_length, s) in self.probe_slots(h).enumerate() {
            let mut v = HashEntry(hashes[s].load(Acquire));
            if v.is_empty() {
                if index == 0 {
                    //will only happen once so maybe there's a way to avoid the clone here
                    index = self.slots.reserve_bucket().ok_or_else(|| FullTable {
                        size: self.capacity(),
                        n_used: self.slots.n_used(),
                        value: data.clone(),
                    })?;
                    unsafe {
                        let h = &mut (&mut *self.data.get())[index];
                        *h = Some(data.clone());
                    }
                }
                match hashes[s].compare_exchange(0, HashEntry::new(index, h).0, Release, Relaxed) {
                    Ok(_) => {
                        return Ok((index, Some(probe_length)));
                    }
                    Err(new_v) => v = HashEntry(new_v),
                }
            }

            if v.hash() == h && self.equal_at_index(v.index(), &data) {
                if index != 0 {
                    self.slots.free_bucket(index);
                }
                return Ok((v.index(), None));
            }
        }
        let r = Err(FullTable {
            size: self.capacity(),
            n_used: self.n_used(),
            value: data,
        });
        drop(read);
        r
    }

    ///Empties the hashtable except for keys in `except`
    pub(super) fn clear(&self, except: &[usize], resize_to: Option<usize>) {
        let size = resize_to.unwrap_or(self.capacity());

        unsafe {
            let hashes = &mut *self.hashes.get();
            if hashes.len() < size {
                *hashes = zeroed_atomic(size);
            } else {
                for h in hashes.iter() {
                    h.store(0, Relaxed);
                }
            }
        }

        if let Some(size) = resize_to {
            unsafe {
                (&mut *self.data.get()).resize(size, None);
                (&mut *self.counts.get()).resize_with(size, || AtomicU64::from(0));
            }
        }
        for index in except {
            self.insert_known_data(*index);
        }

        self.slots.clear(except, resize_to);
    }

    ///Re-inserts the hash of the data at an index.
    ///Returns false if the data is already in the hash.
    fn insert_known_data(&self, index: usize) -> bool {
        if index == 0 || index == 1 {
            return false;
        }

        let data = unsafe {
            (&*self.data.get())
                .get(index)
                .expect("You have resized a table to be smaller than it was before!")
                .as_ref()
        }
        .expect("A marked value is None!");
        let h = Self::hash(data);
        let hash_entry = HashEntry::new(index, h);
        let hashes = self.hashes();
        for s in self.probe_slots(h) {
            match hashes[s].compare_exchange(0, hash_entry.0, Release, Relaxed) {
                Ok(_) => {
                    return true;
                }
                Err(new_v) => {
                    //We had a collision!
                    let v = HashEntry(new_v);
                    if v.hash() == h && self.equal_at_index(v.index(), data) {
                        //unlikely to happen unless except has duplicates
                        return false;
                    }
                }
            }
        }
        panic!("Full table!")
    }

    pub(crate) fn wait_until_unpaused(&self) {
        let mut paused = self.paused.lock().unwrap();
        while *paused {
            paused = self.unpaused_cond_var.wait(paused).unwrap();
        }
    }

    fn read_table(&self) -> ReadingTheTable<'_> {
        let thread_id = self.slots.thread_id();
        let reading_num = &self.reading[thread_id];
        loop {
            if self.is_paused.load(SeqCst) {
                self.wait_until_unpaused();
                continue;
            }
            reading_num.fetch_add(1, Release);
            if !self.is_paused.load(SeqCst) {
                return ReadingTheTable(reading_num);
            }
            reading_num.fetch_sub(1, Release);
            self.wait_until_unpaused();
        }
    }
}

struct ReadingTheTable<'a>(&'a AtomicU64);

impl Drop for ReadingTheTable<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Release);
    }
}

#[cfg(test)]
mod test {
    use crate::manager::ZddIndex;
    use rayon::prelude::*;
    use std::{
        collections::{HashMap, HashSet},
        sync::{Arc, atomic::Ordering::Release},
        time::Duration,
    };

    use super::*;

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

    #[test]
    fn wait_until_unpaused_blocks_while_paused() {
        let table = Arc::new(HashTable::<char>::new(100, 2));

        let read = table.read_table();

        let gc_table = Arc::clone(&table);
        let gc = std::thread::spawn(move || {
            assert!(gc_table.start_gc());
        });

        while !table.is_paused.load(Acquire) {
            std::hint::spin_loop();
        }

        let returned = Arc::new(AtomicBool::new(false));
        let waiter_table = Arc::clone(&table);
        let waiter_returned = Arc::clone(&returned);
        let waiter = std::thread::spawn(move || {
            waiter_table.wait_until_unpaused();
            waiter_returned.store(true, Release);
        });

        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !returned.load(std::sync::atomic::Ordering::Acquire),
            "wait_until_unpaused returned while the table was paused!"
        );

        drop(read);
        gc.join().unwrap();
        table.end_gc();
        waiter.join().unwrap();
        assert!(returned.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn no_reader_slips_past_gc_pause() {
        let table = Arc::new(HashTable::<char>::new(1 << 12, 2));
        let stop = Arc::new(AtomicBool::new(false));

        let gc_table = Arc::clone(&table);
        let gc_stop = Arc::clone(&stop);
        let gc = std::thread::spawn(move || {
            while !gc_stop.load(Relaxed) {
                std::thread::sleep(Duration::from_millis(1));
                if gc_table.start_gc() {
                    gc_table.clear(&[], None);
                    gc_table.end_gc();
                } else {
                    gc_table.wait_until_unpaused();
                }
            }
        });

        let vals: Vec<_> = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG"
            .chars()
            .collect();
        (0..4).into_par_iter().for_each(|_| {
            for _ in 0..2_000 {
                for &k in &vals {
                    table
                        .find_or_insert(RawZddData {
                            value: k,
                            lo: ZddIndex::from(k as usize),
                            hi: ZddIndex::from(0),
                        })
                        .expect("Table should not be full!");
                }
            }
        });

        stop.store(true, Relaxed);
        gc.join().unwrap();

        let mut seen: HashMap<RawZddData<char>, usize> = HashMap::new();
        for (s, h) in table.hashes().iter().enumerate() {
            let entry = HashEntry(h.load(Acquire));
            if entry.is_empty() {
                continue;
            }
            let data = table
                .get(entry.index())
                .expect("Hash entry points at missing data!");
            if let Some(old) = seen.insert(data, entry.index()) {
                assert!(
                    old != entry.index() && s != old,
                    "Two hash entries hold equal data: {old} and {}",
                    entry.index()
                );
            }
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn hash_table_with_other_pool() -> Result<(), FullTable<char>> {
        for _ in 0..20 {
            let hash_table = HashTable::<char>::new(100, 8);
            let used_chars = MOBY.chars().collect::<HashSet<_>>();

            let vals = MOBY
                .par_chars()
                .map(|k| {
                    hash_table
                        .find_or_insert(RawZddData {
                            value: k,
                            lo: ZddIndex::from(k as usize),
                            hi: ZddIndex::from(0),
                        })
                        .map(|(v, _)| (k, v))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut map = HashMap::new();
            for (k, v) in vals {
                let old_v = *map.entry(k).or_insert(v);
                assert!(old_v == v, "{k} cannot be at {old_v} and {v}");
            }
            assert_eq!(map.len(), used_chars.len());
        }
        Ok(())
    }

    #[test]
    fn full_table_returns_error_not_hang() {
        let table = HashTable::<u32>::new(REGION_SIZE, 1);
        let bound = table.capacity() + 1;
        let mut inserted = 0;
        let mut found_err = false;
        for i in 0..bound {
            let r = table.find_or_insert(RawZddData {
                value: u32::try_from(i).unwrap(),
                lo: ZddIndex::from(0),
                hi: ZddIndex::from(1),
            });
            if let Err(FullTable { size, .. }) = r {
                assert_eq!(size, table.capacity());
                found_err = true;
                break;
            }
            inserted += 1;
        }
        assert!(
            found_err,
            "Inserted {inserted} of {bound} without a FullTable error"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn hash_table() -> Result<(), FullTable<char>> {
        assert_eq!(size_of::<HashEntry>(), size_of::<usize>());
        let hash_table = HashTable::<char>::new(100, 8);
        let vals = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG"
            .par_chars()
            .map(|k| {
                hash_table
                    .find_or_insert(RawZddData {
                        value: k,
                        lo: ZddIndex::from(k as usize),
                        hi: ZddIndex::from(0),
                    })
                    .map(|(v, _)| (k, v))
            })
            .collect::<Result<Vec<_>, _>>()?;
        println!("{vals:?}");
        let mut map = HashMap::new();
        for (k, v) in vals {
            assert_eq!(*map.entry(k).or_insert(v), v);
        }
        assert_eq!(map.len(), 27);

        let hash_table = HashTable::<char>::new(100, 8);
        let used_chars = MOBY.chars().collect::<HashSet<_>>();

        let vals = MOBY
            .par_chars()
            .map(|k| {
                hash_table
                    .find_or_insert(RawZddData {
                        value: k,
                        lo: ZddIndex::from(k as usize),
                        hi: ZddIndex::from(0),
                    })
                    .map(|(v, _)| (k, v))
            })
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
    fn capacity_rounds_up_to_a_multiple_of_regions() {
        let table = HashTable::<char>::new(520, 1);
        assert_eq!(table.capacity(), 1024);
        let table = HashTable::<char>::new(1024, 1);
        assert_eq!(table.capacity(), 1024);
        let table = HashTable::<char>::new(1500, 2);
        assert_eq!(table.capacity(), 2048);

        for i in 0..600usize {
            let _ = table.find_or_insert(RawZddData {
                value: char::from_u32(0x4e00 + u32::try_from(i % 100).unwrap()).unwrap(),
                lo: ZddIndex::from(i),
                hi: ZddIndex::from(0),
            });
        }
    }
}
