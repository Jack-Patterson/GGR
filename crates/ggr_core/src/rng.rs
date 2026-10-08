//! Deterministic randomness, bit-identical to GuildGameV2's `Core.Random`.
//!
//! One seeded root split into named streams, so consuming randomness in one system never
//! perturbs another. The goldens in the tests below are V2's own (`RngGoldenValues.cs`), which
//! is the proof this port draws the same numbers as the Unity build.

use serde::{Deserialize, Serialize};

use crate::hash::fnv1a64;

/// SplitMix64, used only to expand a seed into xoshiro state.
pub struct SplitMix64;

impl SplitMix64 {
    pub fn next(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// xoshiro256** — the generator behind every stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Xoshiro256StarStar {
    s: [u64; 4],
}

impl Xoshiro256StarStar {
    pub fn new(seed: u64) -> Self {
        let mut seeder = seed;
        let mut s = [0u64; 4];
        for slot in &mut s {
            *slot = SplitMix64::next(&mut seeder);
        }
        // The all-zero state emits zero forever; close it off rather than reason about it.
        if s == [0; 4] {
            s[0] = 0x9E37_79B9_7F4A_7C15;
        }
        Self { s }
    }

    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform in `[min, max_exclusive)` by rejection sampling. The rejection count changes how
    /// far the stream advances, so this is ported exactly rather than "improved".
    pub fn next_int(&mut self, min: i32, max_exclusive: i32) -> i32 {
        assert!(
            min < max_exclusive,
            "the range [{min}, {max_exclusive}) is empty"
        );
        let range = (i64::from(max_exclusive) - i64::from(min)) as u64;
        let zone = u64::MAX - ((u64::MAX % range) + 1) % range;
        let draw = loop {
            let d = self.next_u64();
            if d <= zone {
                break d;
            }
        };
        (i64::from(min) + (draw % range) as i64) as i32
    }

    /// `true` with probability `percent`/100, consuming exactly one `next_int` draw.
    pub fn chance(&mut self, percent: i32) -> bool {
        self.next_int(0, 100) < percent
    }

    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

/// The named streams. Fixed set, so stored as an array rather than a string-keyed map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamName {
    Dice,
    QuestGen,
    Names,
    Economy,
    Behaviour,
    CharGen,
}

impl StreamName {
    pub const ALL: [StreamName; 6] = [
        StreamName::Dice,
        StreamName::QuestGen,
        StreamName::Names,
        StreamName::Economy,
        StreamName::Behaviour,
        StreamName::CharGen,
    ];

    pub fn token(self) -> &'static str {
        match self {
            StreamName::Dice => "dice",
            StreamName::QuestGen => "questgen",
            StreamName::Names => "names",
            StreamName::Economy => "economy",
            StreamName::Behaviour => "behaviour",
            StreamName::CharGen => "chargen",
        }
    }
}

/// One root seed, six independent streams seeded `root ^ fnv1a64(name)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngStreams {
    root_seed: u64,
    streams: [Xoshiro256StarStar; 6],
}

impl RngStreams {
    pub fn new(root_seed: u64) -> Self {
        let streams =
            StreamName::ALL.map(|n| Xoshiro256StarStar::new(root_seed ^ fnv1a64(n.token())));
        Self { root_seed, streams }
    }

    pub fn root_seed(&self) -> u64 {
        self.root_seed
    }

    pub fn get(&mut self, name: StreamName) -> &mut Xoshiro256StarStar {
        &mut self.streams[name as usize]
    }

    pub fn dice(&mut self) -> &mut Xoshiro256StarStar {
        self.get(StreamName::Dice)
    }
    pub fn questgen(&mut self) -> &mut Xoshiro256StarStar {
        self.get(StreamName::QuestGen)
    }
    pub fn names(&mut self) -> &mut Xoshiro256StarStar {
        self.get(StreamName::Names)
    }
    pub fn economy(&mut self) -> &mut Xoshiro256StarStar {
        self.get(StreamName::Economy)
    }
    pub fn behaviour(&mut self) -> &mut Xoshiro256StarStar {
        self.get(StreamName::Behaviour)
    }
    pub fn chargen(&mut self) -> &mut Xoshiro256StarStar {
        self.get(StreamName::CharGen)
    }

    /// Hash of every stream's position, for the world state hash.
    pub fn state_words(&self) -> impl Iterator<Item = u64> + '_ {
        self.streams.iter().flat_map(|s| s.s.iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // V2's RngGoldenValues.cs, seed 42, first eight draws of each stream.
    const DICE: [u64; 8] = [
        370125584613700256,
        9519067535265815804,
        16682314838418835025,
        6177488575103173463,
        8196036403259994955,
        5252116993365827419,
        14338411498890985336,
        14432655133656922530,
    ];
    const QUESTGEN: [u64; 4] = [
        440610567199142304,
        4455087555737017769,
        14826821969438909537,
        2611714938215344001,
    ];
    const NAMES: [u64; 4] = [
        3849033897835239354,
        306152407235235104,
        15089695510384807211,
        15695559367903925348,
    ];
    const ECONOMY: [u64; 4] = [
        2477465821112062068,
        11829710158324357442,
        8301786118246061033,
        1925889564600543134,
    ];
    const BEHAVIOUR: [u64; 4] = [
        12737018656694457026,
        6261436392895822600,
        13298179362847964362,
        12450954882348079748,
    ];
    const CHARGEN: [u64; 4] = [
        17186749105907706368,
        10315249135863554556,
        13777772519832573558,
        2618830145473066275,
    ];

    fn check(name: StreamName, golden: &[u64]) {
        let mut streams = RngStreams::new(42);
        let s = streams.get(name);
        for (i, g) in golden.iter().enumerate() {
            assert_eq!(s.next_u64(), *g, "{} draw {i}", name.token());
        }
    }

    #[test]
    fn streams_match_v2_goldens() {
        check(StreamName::Dice, &DICE);
        check(StreamName::QuestGen, &QUESTGEN);
        check(StreamName::Names, &NAMES);
        check(StreamName::Economy, &ECONOMY);
        check(StreamName::Behaviour, &BEHAVIOUR);
        check(StreamName::CharGen, &CHARGEN);
    }

    #[test]
    fn next_int_stays_in_range_and_is_deterministic() {
        let mut a = Xoshiro256StarStar::new(7);
        let mut b = Xoshiro256StarStar::new(7);
        for _ in 0..10_000 {
            let x = a.next_int(1, 21);
            assert!((1..21).contains(&x));
            assert_eq!(x, b.next_int(1, 21));
        }
    }

    #[test]
    fn streams_are_independent() {
        let mut one = RngStreams::new(42);
        let mut two = RngStreams::new(42);
        for _ in 0..100 {
            one.dice().next_u64();
        }
        assert_eq!(one.chargen().next_u64(), two.chargen().next_u64());
    }
}
