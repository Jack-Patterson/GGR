/// FNV-1a 64-bit offset basis.
pub const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
/// FNV-1a 64-bit prime.
pub const FNV_PRIME: u64 = 1_099_511_628_211;

/// FNV-1a over the UTF-8 bytes of `text` — the same hash V2 seeds its RNG streams with.
pub fn fnv1a64(text: &str) -> u64 {
    let mut h = FNV_OFFSET;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// An incremental FNV-1a hasher over little-endian integers, used by the world's state hash.
/// Fixed-width and explicit so the same world hashes the same on every platform.
#[derive(Debug, Clone)]
pub struct StateHasher {
    h: u64,
}

impl Default for StateHasher {
    fn default() -> Self {
        Self { h: FNV_OFFSET }
    }
}

impl StateHasher {
    pub fn new() -> Self {
        Self::default()
    }

    fn byte(&mut self, b: u8) {
        self.h ^= u64::from(b);
        self.h = self.h.wrapping_mul(FNV_PRIME);
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        for b in v.to_le_bytes() {
            self.byte(b);
        }
        self
    }

    pub fn i64(&mut self, v: i64) -> &mut Self {
        self.u64(v as u64)
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.u64(u64::from(v))
    }

    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.i64(i64::from(v))
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.byte(u8::from(v));
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.u64(s.len() as u64);
        for b in s.as_bytes() {
            self.byte(*b);
        }
        self
    }

    pub fn finish(&self) -> u64 {
        self.h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_known_vectors() {
        assert_eq!(fnv1a64(""), FNV_OFFSET);
        // Reference value for "a" from the FNV specification.
        assert_eq!(fnv1a64("a"), 0xaf63_dc4c_8601_ec8c);
    }
}
